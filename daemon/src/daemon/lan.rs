use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use parking_lot::RwLock;

use crate::daemon::session::SessionHandle;
use crate::daemon::{DaemonEvent, stream};
use crate::metadata::MetadataStore;

pub async fn spawn(
    cancel: CancellationToken,
    inbox: mpsc::Sender<DaemonEvent>,
    session: Arc<SessionHandle>,
    metadata: Arc<RwLock<MetadataStore>>,
    upnp_router: Router,
    port: u16,
) -> Result<JoinHandle<()>> {
    let app = stream::router(inbox, session, metadata, true).nest("/upnp", upnp_router);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding lan listener to {addr}"))?;
    info!(%addr, "lan listener bound");
    let shutdown = async move { cancel.cancelled().await };
    Ok(tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown).await {
            warn!(error = %e, "lan listener exited with error");
        }
        debug!("lan listener task ended");
    }))
}
