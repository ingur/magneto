use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::de::DeserializeOwned;
use tokio_tungstenite::tungstenite::Message;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub async fn run_command<T: DeserializeOwned>(
    port: u16,
    command: &str,
    payload: serde_json::Value,
    token: Option<&str>,
) -> Result<T> {
    let value = run_raw(port, command, payload, token).await?;
    serde_json::from_value(value).context("deserializing daemon response")
}

pub async fn run_raw(
    port: u16,
    command: &str,
    payload: serde_json::Value,
    token: Option<&str>,
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

    tokio::time::timeout(REQUEST_TIMEOUT, fut)
        .await
        .context("daemon did not respond within 15s")?
}
