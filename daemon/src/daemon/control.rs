use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use std::collections::HashMap;

use anyhow::{Context, Result};
use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use parking_lot::RwLock;

use crate::daemon::session::SessionHandle;
use crate::daemon::{ClientId, DaemonEvent, stream};
use crate::metadata::MetadataStore;
use magneto_core::protocol::Outbound;

#[derive(Clone)]
struct ControlState {
    inbox: mpsc::Sender<DaemonEvent>,
    next_id: Arc<AtomicU64>,
    token: Arc<String>,
}

pub async fn spawn(
    cancel: CancellationToken,
    inbox: mpsc::Sender<DaemonEvent>,
    session: Arc<SessionHandle>,
    metadata: Arc<RwLock<MetadataStore>>,
    port: u16,
    token: String,
) -> Result<JoinHandle<()>> {
    let state = ControlState {
        inbox: inbox.clone(),
        next_id: Arc::new(AtomicU64::new(1)),
        token: Arc::new(token),
    };
    let app = Router::new()
        .route("/ws", get(ws_upgrade))
        .with_state(state)
        .merge(stream::router(inbox, session, metadata, false));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding control listener to {addr}"))?;
    info!(%addr, "control listener bound");

    let shutdown = async move { cancel.cancelled().await };
    Ok(tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(shutdown).await {
            warn!(error = %e, "control listener exited with error");
        }
        debug!("control listener task ended");
    }))
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<ControlState>,
) -> Response {
    // Gate the control plane on the per-run token from daemon.json. Only a
    // local process that can read that file (the app, the CLI) can connect;
    // a browser page pointed at the loopback port cannot obtain it.
    let provided = params.get("token").map(String::as_str).unwrap_or("");
    if !constant_time_eq(provided.as_bytes(), state.token.as_bytes()) {
        return (StatusCode::UNAUTHORIZED, "invalid control token").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Length-aware constant-time comparison so a wrong token can't be recovered by
/// timing. The token length is fixed and not secret, so an early length check
/// is fine.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

async fn handle_socket(socket: WebSocket, state: ControlState) {
    let id: ClientId = state.next_id.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<Outbound>(64);

    if state
        .inbox
        .send(DaemonEvent::ClientConnected { id, tx })
        .await
        .is_err()
    {
        return;
    }

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = match serde_json::to_string(&msg) {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "outbound serialize failed");
                    continue;
                }
            };
            if sink.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    while let Some(frame) = stream.next().await {
        match frame {
            Ok(Message::Text(text)) => {
                if state
                    .inbox
                    .send(DaemonEvent::ClientMessage { id, text: text.to_string() })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {}
        }
    }

    let _ = state
        .inbox
        .send(DaemonEvent::ClientDisconnected { id })
        .await;
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn constant_time_eq_matches_only_identical() {
        assert!(constant_time_eq(b"token", b"token"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"token", b"toker"));
        assert!(!constant_time_eq(b"token", b"token-longer"));
        assert!(!constant_time_eq(b"", b"x"));
    }
}
