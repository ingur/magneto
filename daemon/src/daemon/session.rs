use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, DhtSessionConfig, ManagedTorrent, Session,
    SessionOptions, SessionPersistenceConfig, TorrentStatsState,
};
use tokio::io::{AsyncRead, AsyncSeek};
use tokio_util::bytes::Bytes;
use tokio_util::sync::CancellationToken;

pub type TorrentHandle = Arc<ManagedTorrent>;

/// A persisted torrent whose `.torrent` sidecar is unreadable falls back to a
/// magnet, and librqbit resolves it inside session construction with no bound of
/// its own. Unbounded, that means the control port never binds.
const OPEN_TIMEOUT: Duration = Duration::from_secs(120);

pub struct SessionHandle {
    inner: Arc<Session>,
}

pub struct AddOutcome {
    pub info_hash: String,
    pub handle: TorrentHandle,
    pub already_existed: bool,
}

pub trait SeekableReader: AsyncRead + AsyncSeek + Send + Unpin {}
impl<T: AsyncRead + AsyncSeek + Send + Unpin> SeekableReader for T {}

pub struct OpenStream {
    pub reader: Box<dyn SeekableReader + 'static>,
    pub length: u64,
}

impl SessionHandle {
    pub async fn new(
        downloads_dir: PathBuf,
        session_dir: PathBuf,
        dht_state: PathBuf,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let opts = SessionOptions {
            persistence: Some(SessionPersistenceConfig::Json { folder: Some(session_dir) }),
            fastresume: true,
            // Each open file stream holds one blocking permit for its whole life
            // and the same pool does disk writes, so the default 8 starves.
            runtime_worker_threads: Some(32),
            disable_local_service_discovery: true,
            // Keep the routing table in our data dir, not in a cache dir shared
            // with every other rqbit-based process.
            dht: Some(DhtSessionConfig {
                persistence: Some(DhtPersistenceConfig {
                    config_filename: Some(dht_state),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            cancellation_token: Some(cancel.clone()),
            ..Default::default()
        };
        let open = Session::new_with_opts(downloads_dir, opts);
        let inner = match tokio::time::timeout(OPEN_TIMEOUT, open).await {
            Ok(session) => session.context("constructing librqbit session")?,
            Err(_) => {
                cancel.cancel();
                bail!("librqbit session did not open within {}s", OPEN_TIMEOUT.as_secs());
            }
        };
        Ok(Self { inner })
    }

    pub fn list_infohashes(&self) -> Vec<String> {
        self.inner
            .with_torrents(|iter| iter.map(|(_, h)| h.info_hash().as_string()).collect())
    }

    pub fn get(&self, info_hash: &str) -> Option<TorrentHandle> {
        let id = TorrentIdOrHash::parse(info_hash).ok()?;
        self.inner.get(id)
    }

    pub async fn add_url(&self, url: &str) -> Result<AddOutcome> {
        self.add(AddTorrent::from_url(url)).await
    }

    pub async fn add_bytes(&self, bytes: Vec<u8>) -> Result<AddOutcome> {
        self.add(AddTorrent::from_bytes(Bytes::from(bytes))).await
    }

    async fn add(&self, source: AddTorrent<'_>) -> Result<AddOutcome> {
        let opts = AddTorrentOptions {
            paused: true,
            only_files: Some(Vec::new()),
            // Open existing files on disk in place so a re-add resumes them
            // instead of failing because the files already exist.
            overwrite: true,
            ..Default::default()
        };
        let resp = self
            .inner
            .add_torrent(source, Some(opts))
            .await
            .context("adding torrent")?;
        match resp {
            AddTorrentResponse::Added(_, handle) => Ok(AddOutcome {
                info_hash: handle.info_hash().as_string(),
                handle,
                already_existed: false,
            }),
            AddTorrentResponse::AlreadyManaged(_, handle) => Ok(AddOutcome {
                info_hash: handle.info_hash().as_string(),
                handle,
                already_existed: true,
            }),
            AddTorrentResponse::ListOnly(_) => bail!("unexpected ListOnly response"),
        }
    }

    pub async fn pause(&self, info_hash: &str) -> Result<()> {
        let handle = self.require(info_hash)?;
        self.inner.pause(&handle).await
    }

    pub async fn unpause(&self, info_hash: &str) -> Result<()> {
        let handle = self.require(info_hash)?;
        self.inner.unpause(&handle).await
    }

    /// Unpause only when the engine is holding the torrent still: paused, or
    /// checking files with a pause pending (which `pause` leaves behind and
    /// only a fresh start clears). librqbit's `unpause` hard-errors on a live
    /// torrent, and an unpause landing mid-check strands it.
    pub async fn unpause_if_paused(&self, info_hash: &str) -> Result<()> {
        let handle = self.require(info_hash)?;
        self.ensure_active(&handle).await
    }

    pub async fn update_only_files(
        &self,
        info_hash: &str,
        indices: &HashSet<usize>,
    ) -> Result<()> {
        let handle = self.require(info_hash)?;
        self.inner.update_only_files(&handle, indices).await
    }

    pub async fn delete(&self, info_hash: &str, delete_files: bool) -> Result<()> {
        let id = TorrentIdOrHash::parse(info_hash).context("invalid info_hash")?;
        self.inner.delete(id, delete_files).await
    }

    /// Open a file for streaming and ensure the torrent is live. Selection is
    /// the caller's responsibility (the stream endpoint routes it through the
    /// same selection path as resume).
    pub async fn stream(&self, info_hash: &str, file_index: usize) -> Result<OpenStream> {
        let handle = self.require(info_hash)?;
        self.ensure_active(&handle).await?;
        let stream = handle.stream(file_index).await.context("opening file stream")?;
        let length = stream.len();
        Ok(OpenStream { reader: Box::new(stream), length })
    }

    async fn ensure_active(&self, handle: &TorrentHandle) -> Result<()> {
        if !engine_paused(&handle.stats().state) {
            return Ok(());
        }
        self.inner.unpause(handle).await
    }

    fn require(&self, info_hash: &str) -> Result<TorrentHandle> {
        self.get(info_hash)
            .with_context(|| format!("no torrent with info_hash {info_hash}"))
    }
}

/// Whether the engine is holding this torrent still. `ManagedTorrent::is_paused`
/// reports the persisted intent instead, which drifts from the state machine
/// when a pause or unpause lands while files are being checked.
pub fn engine_paused(state: &TorrentStatsState) -> bool {
    matches!(state, TorrentStatsState::Paused | TorrentStatsState::Initializing { paused: true })
}
