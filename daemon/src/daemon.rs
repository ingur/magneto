use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use magneto_core::config::Config;
use crate::metadata::MetadataStore;
use magneto_core::protocol::{DaemonInfo, Outbound, Request, SnapshotEvent};

pub mod bootstrap;
pub mod check;
pub mod commands;
pub mod control;
pub mod lan;
pub mod preflight;
pub mod removal;
pub mod session;
pub mod session_store;
pub mod stats;
pub mod stream;
pub mod upnp;

use session::SessionHandle;

pub type ClientId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownKind {
    Restart,
    Stop,
}

pub enum DaemonEvent {
    ClientConnected { id: ClientId, tx: mpsc::Sender<Outbound> },
    ClientDisconnected { id: ClientId },
    ClientMessage { id: ClientId, text: String },
    StatsReady(magneto_core::protocol::StatsEvent),
    TorrentCompletedTick { info_hash: String },
    // A torrent the engine put in the error state, seen by the stats tick.
    TorrentErrored { info_hash: String },
    // A torrent finished checking its files, so the engine side of finalizing
    // can run now. What to do with it is in `Daemon::checks`.
    CheckFinished { info_hash: String },
    // A spawned add_torrent finished (or timed out); carries everything needed
    // to finalize the torrent and answer the requesting client.
    AddCompleted {
        client: ClientId,
        request_id: String,
        source: String,
        kind: magneto_core::protocol::SourceKind,
        outcome: anyhow::Result<session::AddOutcome>,
    },
    // Stream-side file selection, routed through the loop so selection keeps
    // a single writer; carries the reply the HTTP handler is waiting on.
    SelectForStream {
        info_hash: String,
        index: u32,
        reply: oneshot::Sender<anyhow::Result<()>>,
    },
    RestartRequested,
    ShutdownRequested,
}

pub struct ClientHandle {
    pub tx: mpsc::Sender<Outbound>,
}

pub struct Daemon {
    pub config: Config,
    pub started: Config,
    pub config_path: PathBuf,
    pub data_dir: PathBuf,
    pub metadata: Arc<RwLock<MetadataStore>>,
    pub metadata_path: PathBuf,
    pub session: Arc<SessionHandle>,
    pub clients: HashMap<ClientId, ClientHandle>,
    // Torrents already re-checked after an engine error this run, so a failing
    // check cannot loop.
    pub rechecked: HashSet<String>,
    // Torrents whose file check is being waited on, and what to do when it ends.
    pub checks: HashMap<String, check::Pending>,
    pub control_task: Option<JoinHandle<()>>,
    pub lan_task: Option<JoinHandle<()>>,
    pub upnp_ssdp: Option<JoinHandle<()>>,
    pub stats_task: Option<JoinHandle<()>>,
    pub upnp_active: bool,
    pub inbox_tx: mpsc::Sender<DaemonEvent>,
    pub cancel: CancellationToken,
    // Live `config` feed for the stats task, which renders off the event loop:
    // set_config sends the hot-applied config so deltas don't drift on a stale
    // clone (media extensions, persist/share defaults).
    pub config_tx: watch::Sender<Config>,
    shutdown_kind: Option<ShutdownKind>,
}

impl Daemon {
    /// `config` is the desired config the UI reports; `started` is what the session
    /// and listeners are built from. They differ only on a last-known-good fallback.
    pub async fn new(
        config: Config,
        started: Config,
        config_path: PathBuf,
        data_dir: PathBuf,
        metadata_path: PathBuf,
        inbox_tx: mpsc::Sender<DaemonEvent>,
        cancel: CancellationToken,
    ) -> Result<Self> {
        started.ensure_dirs().context("ensuring downloads dir")?;
        let metadata = Arc::new(RwLock::new(MetadataStore::load_or_create(&metadata_path)?));

        // Before the engine opens the session: it restores torrents from the
        // files in there and stalls the whole boot on anything unreadable.
        session_store::repair(&data_dir);
        let session = Arc::new(
            SessionHandle::new(
                started.downloads.dir.clone(),
                session_store::session_dir(&data_dir),
                data_dir.join("dht.json"),
                cancel.clone(),
            )
            .await?,
        );

        let (config_tx, _) = watch::channel(config.clone());

        Ok(Self {
            config,
            started,
            config_path,
            data_dir,
            metadata,
            metadata_path,
            session,
            clients: HashMap::new(),
            rechecked: HashSet::new(),
            checks: HashMap::new(),
            control_task: None,
            lan_task: None,
            upnp_ssdp: None,
            stats_task: None,
            upnp_active: false,
            inbox_tx,
            cancel,
            config_tx,
            shutdown_kind: None,
        })
    }

    pub async fn run(mut self, mut inbox_rx: mpsc::Receiver<DaemonEvent>) -> Result<ShutdownKind> {
        info!("daemon event loop started");
        let kind = loop {
            tokio::select! {
                Some(ev) = inbox_rx.recv() => {
                    self.handle_event(ev).await;
                    if let Some(kind) = self.shutdown_kind {
                        break kind;
                    }
                }
                else => break ShutdownKind::Stop,
            }
        };
        self.teardown(kind).await;
        Ok(kind)
    }

    async fn handle_event(&mut self, ev: DaemonEvent) {
        match ev {
            DaemonEvent::ClientConnected { id, tx } => {
                let snapshot = self.snapshot().await;
                let _ = tx.try_send(Outbound::Snapshot(snapshot));
                self.clients.insert(id, ClientHandle { tx });
                debug!(client = id, "client connected");
            }
            DaemonEvent::ClientDisconnected { id } => {
                self.clients.remove(&id);
                debug!(client = id, "client disconnected");
            }
            DaemonEvent::ClientMessage { id, text } => {
                let outbound = match serde_json::from_str::<Request>(&text) {
                    Ok(req) => commands::dispatch(self, id, req).await,
                    Err(e) => {
                        Some(Outbound::error(String::new(), format!("malformed request: {e}")))
                    }
                };
                if let Some(outbound) = outbound {
                    self.send_to(id, outbound);
                }
            }
            DaemonEvent::StatsReady(payload) => {
                if !payload.is_empty() {
                    self.broadcast(Outbound::Stats(payload));
                }
            }
            DaemonEvent::TorrentCompletedTick { info_hash } => {
                self.broadcast(Outbound::TorrentComplete { info_hash });
            }
            DaemonEvent::AddCompleted { client, request_id, source, kind, outcome } => {
                let resp = commands::finish_add(self, request_id, source, kind, outcome).await;
                self.send_to(client, resp);
            }
            DaemonEvent::TorrentErrored { info_hash } => {
                // A record that is not applied yet belongs to an add or to
                // reconcile, and both recover it themselves with the right
                // intent. Stepping in here would race them.
                let finalized =
                    self.metadata.read().get(&info_hash).is_some_and(|e| e.finalized);
                if finalized {
                    commands::recover_errored(self, &info_hash, commands::Finalize::Restore).await;
                }
            }
            DaemonEvent::CheckFinished { info_hash } => {
                let Some(pending) = self.checks.remove(&info_hash) else { return };
                commands::finalize_torrent(self, &info_hash, pending.from).await;
                session_store::sync_bitfield(&self.data_dir, &info_hash);
                if pending.repause
                    && let Err(e) = self.session.pause(&info_hash).await
                {
                    warn!(hash = %short(&info_hash), error = %e, "restoring pause after check failed");
                }
            }
            DaemonEvent::SelectForStream { info_hash, index, reply } => {
                let result =
                    commands::select_for_resume(&self.session, &self.metadata, &info_hash, &[index])
                        .await;
                if result.is_ok() {
                    let _ = self.save_metadata();
                }
                let _ = reply.send(result);
            }
            DaemonEvent::RestartRequested => {
                info!("restart requested");
                self.shutdown_kind = Some(ShutdownKind::Restart);
            }
            DaemonEvent::ShutdownRequested => {
                info!("shutdown requested");
                self.shutdown_kind = Some(ShutdownKind::Stop);
            }
        }
    }

    /// Ask the event loop to exit after the current event. For command
    /// handlers, which already run ON the loop: setting the flag directly,
    /// instead of sending an event into the loop's own bounded inbox, means
    /// the loop can never block awaiting itself. Off-loop tasks (the signal
    /// listeners) keep using the inbox events.
    pub(crate) fn request_shutdown(&mut self, kind: ShutdownKind) {
        self.shutdown_kind = Some(kind);
    }

    /// Publish the metadata store, logging a failure. It is the only record of
    /// what the user asked to keep, and `removal::disposable` reads those flags,
    /// so callers that answer a client pass the error on.
    pub fn save_metadata(&self) -> Result<()> {
        let result = self.metadata.read().save(&self.metadata_path);
        if let Err(e) = &result {
            warn!(error = %e, "failed to save metadata");
        }
        result
    }

    /// Fan an event out to every client without blocking the event loop. Uses
    /// `try_send`: a client whose bounded channel is full is not draining (a
    /// wedged or frozen socket), so its event is dropped rather than stalling
    /// every other client and shutdown behind it. The ping timeout reaps the
    /// dead client and a reconnect snapshot re-syncs it. `Closed` is a normal
    /// just-disconnected client and is ignored.
    pub fn broadcast(&self, event: Outbound) {
        for (id, client) in &self.clients {
            if let Err(mpsc::error::TrySendError::Full(_)) = client.tx.try_send(event.clone()) {
                warn!(client = id, "control channel full; dropping event for slow client");
            }
        }
    }

    pub fn send_to(&self, client_id: ClientId, msg: Outbound) {
        if let Some(client) = self.clients.get(&client_id)
            && let Err(mpsc::error::TrySendError::Full(_)) = client.tx.try_send(msg)
        {
            warn!(client = client_id, "control channel full; dropping reply for slow client");
        }
    }

    pub async fn snapshot(&self) -> SnapshotEvent {
        SnapshotEvent {
            daemon: DaemonInfo {
                version: env!("CARGO_PKG_VERSION"),
                status: "ready",
                control_port: self.started.network.control_port,
                lan_port: self.started.network.lan_port,
                upnp_active: self.upnp_active,
                pending_restart: magneto_core::config::pending_restart(&self.started, &self.config),
            },
            config: self.config.clone(),
            torrents: commands::list_summaries(self).await,
        }
    }

    async fn teardown(&mut self, kind: ShutdownKind) {
        match kind {
            ShutdownKind::Restart => self.broadcast(Outbound::DaemonRestarting),
            ShutdownKind::Stop => {
                self.broadcast(Outbound::DaemonShutdown);
                // Only a deliberate stop drops what the user did not keep. A
                // crash leaves everything in place: the next boot cannot tell
                // the difference, so it must not delete on the guess.
                let cleanup = commands::cleanup_unpersisted(self);
                if tokio::time::timeout(Duration::from_secs(10), cleanup).await.is_err() {
                    warn!("cleanup timed out after 10s; the next stop finishes it");
                }
                magneto_core::supervisor::remove_descriptor(&self.data_dir);
            }
        }
        let _ = self.save_metadata();
        self.cancel.cancel();
        self.clients.clear();
        let joins: Vec<JoinHandle<()>> = [
            self.control_task.take(),
            self.stats_task.take(),
            self.lan_task.take(),
            self.upnp_ssdp.take(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let join_all = futures_util::future::join_all(joins);
        if tokio::time::timeout(Duration::from_secs(5), join_all).await.is_err() {
            warn!("background tasks did not join within 5s; exiting anyway");
        }
    }
}

pub fn short(info_hash: &str) -> &str {
    info_hash.get(..8).unwrap_or(info_hash)
}

pub(crate) fn install_shutdown_listeners(inbox: mpsc::Sender<DaemonEvent>) {
    {
        let i = inbox.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = i.send(DaemonEvent::ShutdownRequested).await;
        });
    }

    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        for (kind, label) in [
            (SignalKind::terminate(), "SIGTERM"),
            (SignalKind::hangup(), "SIGHUP"),
        ] {
            let i = inbox.clone();
            tokio::spawn(async move {
                match signal(kind) {
                    Ok(mut s) => {
                        s.recv().await;
                        let _ = i.send(DaemonEvent::ShutdownRequested).await;
                    }
                    Err(e) => warn!(signal = label, error = %e, "failed to install signal listener"),
                }
            });
        }
    }

    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_close, ctrl_logoff, ctrl_shutdown};
        macro_rules! listen {
            ($factory:ident, $label:expr) => {{
                let i = inbox.clone();
                tokio::spawn(async move {
                    match $factory() {
                        Ok(mut s) => {
                            s.recv().await;
                            let _ = i.send(DaemonEvent::ShutdownRequested).await;
                        }
                        Err(e) => warn!(
                            signal = $label,
                            error = %e,
                            "failed to install signal listener"
                        ),
                    }
                });
            }};
        }
        listen!(ctrl_break, "Ctrl-Break");
        listen!(ctrl_close, "Ctrl-Close");
        listen!(ctrl_shutdown, "Ctrl-Shutdown");
        listen!(ctrl_logoff, "Ctrl-Logoff");
    }
}
