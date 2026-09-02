use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

/// How long the daemon gives add_torrent to resolve a source's metadata
/// before it answers with an error. Clients wait past it so the daemon's own
/// verdict is what they report.
pub const ADD_TIMEOUT: Duration = Duration::from_secs(120);
pub const ADD_REQUEST_TIMEOUT: Duration = Duration::from_secs(ADD_TIMEOUT.as_secs() + 5);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn run_raw(
    port: u16,
    command: &str,
    payload: serde_json::Value,
    token: Option<&str>,
    timeout: Duration,
) -> Result<serde_json::Value> {
    // The control token is a fixed-length hex string (URL-safe), so it is
    // interpolated directly rather than percent-encoded.
    let url = match token {
        Some(t) => format!("ws://127.0.0.1:{port}/ws?token={t}"),
        None => format!("ws://127.0.0.1:{port}/ws"),
    };
    // Token-free form for error context: the connect error propagates to logs,
    // the CLI, and the Tauri frontend, so the token must never appear in it.
    let display_url = format!("ws://127.0.0.1:{port}/ws");
    let request = serde_json::json!({ "type": command, "id": "1", "payload": payload });

    let fut = async move {
        let (mut ws, _) = tokio_tungstenite::connect_async(&url)
            .await
            .with_context(|| format!("connecting to {display_url}"))?;
        ws.send(Message::Text(request.to_string().into()))
            .await
            .context("sending request")?;
        loop {
            let Some(frame) = ws.next().await else {
                bail!("daemon closed the connection before responding");
            };
            let frame = frame.context("reading from daemon")?;
            let text = match frame {
                Message::Text(t) => t.to_string(),
                Message::Close(_) => bail!("daemon closed the connection"),
                _ => continue,
            };
            let value: serde_json::Value =
                serde_json::from_str(&text).context("parsing daemon message")?;
            let kind = value.get("type").and_then(|t| t.as_str());
            match kind {
                Some("response") if value.get("id") == Some(&serde_json::json!("1")) => {
                    let _ = ws.close(None).await;
                    return Ok(value.get("result").cloned().unwrap_or(serde_json::Value::Null));
                }
                Some("error") if value.get("id") == Some(&serde_json::json!("1")) => {
                    let err = value
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let _ = ws.close(None).await;
                    bail!("daemon error: {err}");
                }
                _ => continue,
            }
        }
    };

    tokio::time::timeout(timeout, fut)
        .await
        .with_context(|| format!("daemon did not respond within {}s", timeout.as_secs()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients_outlast_the_daemon_add_budget() {
        assert!(ADD_REQUEST_TIMEOUT > ADD_TIMEOUT);
        assert!(REQUEST_TIMEOUT < ADD_TIMEOUT);
    }

    #[tokio::test]
    async fn run_raw_gives_up_at_the_deadline_it_was_given() {
        // A listener that accepts and then never answers the handshake.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let err = run_raw(port, "ping", serde_json::json!({}), None, Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "daemon did not respond within 1s");
    }
}
