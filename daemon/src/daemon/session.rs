use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use librqbit::api::TorrentIdOrHash;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ManagedTorrent, Session, SessionOptions,
    SessionPersistenceConfig,
};
use tokio::io::{AsyncRead, AsyncSeek};
use tokio_util::bytes::Bytes;

pub type TorrentHandle = Arc<ManagedTorrent>;

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
    pub async fn new(downloads_dir: PathBuf, session_dir: PathBuf) -> Result<Self> {
        let opts = SessionOptions {
            persistence: Some(SessionPersistenceConfig::Json { folder: Some(session_dir) }),
            fastresume: true,
            ..Default::default()
        };
        let inner = Session::new_with_opts(downloads_dir, opts)
            .await
            .context("constructing librqbit session")?;
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

    /// Unpause only if the torrent is paused. librqbit's `unpause` hard-errors
    /// "torrent is already live" on a running torrent, so callers that just want
    /// to ensure it is live route through here.
    pub async fn unpause_if_paused(&self, info_hash: &str) -> Result<()> {
        let handle = self.require(info_hash)?;
        if !handle.is_paused() {
            return Ok(());
        }
        self.inner.unpause(&handle).await
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
        // librqbit's delete unwraps the torrent metadata (`load_full().expect`),
        // so deleting an in-session torrent whose metadata isn't resolved (e.g. a
        // persisted magnet still re-resolving just after a restart, see the
        // reconcile watcher path) would panic the whole daemon on the event loop.
        // Refuse cleanly instead; it becomes deletable once metadata resolves.
        let handle = self.require(info_hash)?;
        if handle.with_metadata(|_| ()).is_err() {
            bail!("torrent is still resolving and cannot be removed yet");
        }
        let id = TorrentIdOrHash::parse(info_hash).context("invalid info_hash")?;
        self.inner.delete(id, delete_files).await
    }

    /// Open a file for streaming and ensure the torrent is live. Selection is
    /// the caller's responsibility (the stream endpoint routes it through the
    /// same selection path as resume).
    pub async fn stream(&self, info_hash: &str, file_index: usize) -> Result<OpenStream> {
        let handle = self.require(info_hash)?;
        self.ensure_active(&handle).await?;
        let stream = handle.stream(file_index).context("opening file stream")?;
        let length = stream.len();
        Ok(OpenStream { reader: Box::new(stream), length })
    }

    async fn ensure_active(&self, handle: &TorrentHandle) -> Result<()> {
        if !handle.is_paused() {
            return Ok(());
        }
        self.inner.unpause(handle).await
    }

    fn require(&self, info_hash: &str) -> Result<TorrentHandle> {
        self.get(info_hash)
            .with_context(|| format!("no torrent with info_hash {info_hash}"))
    }
}
