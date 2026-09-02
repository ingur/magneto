use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::http::header::{
    ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE, RETRY_AFTER,
};
use axum::response::Response;
use axum::routing::get;
use parking_lot::RwLock;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::{mpsc, oneshot};
use tokio_util::io::ReaderStream;
use tracing::warn;

use crate::daemon::commands;
use crate::daemon::commands::Unservable;
use crate::daemon::DaemonEvent;
use crate::daemon::session::SessionHandle;
use crate::daemon::short;
use crate::media;
use crate::metadata::MetadataStore;

const STREAM_BUFFER: usize = 256 * 1024;
// How long a request waits out a file check before telling the player to retry.
const CHECK_WAIT: Duration = Duration::from_secs(10);
const CHECK_POLL: Duration = Duration::from_millis(250);

#[derive(Clone)]
struct StreamState {
    inbox: mpsc::Sender<DaemonEvent>,
    session: Arc<SessionHandle>,
    metadata: Arc<RwLock<MetadataStore>>,
    // LAN-bound router (true): only files marked `shared` are reachable, so a
    // LAN peer can't stream non-shared media. Loopback router (false): any
    // tracked media file is served.
    require_shared: bool,
}

pub fn router(
    inbox: mpsc::Sender<DaemonEvent>,
    session: Arc<SessionHandle>,
    metadata: Arc<RwLock<MetadataStore>>,
    require_shared: bool,
) -> Router {
    Router::new()
        .route(
            "/stream/{info_hash}/{file_index}/{filename}",
            get(handle_stream),
        )
        .with_state(StreamState { inbox, session, metadata, require_shared })
}

async fn handle_stream(
    State(state): State<StreamState>,
    Path((info_hash, file_index, filename)): Path<(String, usize, String)>,
    headers: HeaderMap,
) -> Response {
    // Only files magneto tracks as media are streamable. Opening a stream
    // selects the file in the engine, so a foreign index (e.g. a stale player
    // URL for a dropped file whose bytes were deleted but whose have-bits the
    // engine still holds) must be refused, not silently re-selected.
    let Ok(idx) = u32::try_from(file_index) else {
        return text_response(StatusCode::NOT_FOUND, "file not found");
    };
    let known = {
        let meta = state.metadata.read();
        match meta.get(&info_hash).and_then(|e| e.files.get(&idx)) {
            Some(fm) => !state.require_shared || fm.shared,
            None => false,
        }
    };
    if !known {
        return text_response(StatusCode::NOT_FOUND, "file not found");
    }
    // A torrent that is checking files cannot serve anything, and the check is
    // usually seconds, so hold the request briefly rather than failing a player
    // that will not retry.
    if let Some(response) = wait_until_servable(&state, &info_hash).await {
        return response;
    }
    // Playing implies starting: the selection goes through the event loop
    // (the single writer for selection state) and runs the same path resume
    // uses, so a paused torrent wakes up for exactly this file. A loop that
    // no longer answers (daemon tearing down) reads as not found.
    let (reply_tx, reply_rx) = oneshot::channel();
    let event = DaemonEvent::SelectForStream {
        info_hash: info_hash.clone(),
        index: idx,
        reply: reply_tx,
    };
    let selected = match state.inbox.send(event).await {
        Ok(()) => reply_rx
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("daemon is shutting down"))),
        Err(_) => Err(anyhow::anyhow!("daemon is shutting down")),
    };
    if let Err(e) = selected {
        warn!(hash = %short(&info_hash), error = %e, "stream selection failed");
        return text_response(StatusCode::CONFLICT, format!("cannot start this file: {e}"));
    }
    let mime = media::mime_for(&filename);
    let open = match state.session.stream(&info_hash, file_index).await {
        Ok(o) => o,
        Err(e) => {
            warn!(hash = %short(&info_hash), error = %e, "stream open failed");
            return text_response(StatusCode::CONFLICT, format!("cannot open this file: {e}"));
        }
    };
    let length = open.length;
    let range_header = headers.get(RANGE).and_then(|v| v.to_str().ok());
    match parse_range(range_header, length) {
        RangeOutcome::None => full_response(open.reader, length, mime),
        RangeOutcome::Partial { start, end } => {
            partial_response(open.reader, start, end, length, mime).await
        }
        RangeOutcome::Invalid => unsatisfiable_response(length, mime),
    }
}

fn full_response(
    reader: Box<dyn crate::daemon::session::SeekableReader + 'static>,
    length: u64,
    mime: &str,
) -> Response {
    let body = Body::from_stream(ReaderStream::with_capacity(reader, STREAM_BUFFER));
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, mime)
        .header(CONTENT_LENGTH, length)
        .header(ACCEPT_RANGES, "bytes")
        .body(body)
        .unwrap()
}

async fn partial_response(
    mut reader: Box<dyn crate::daemon::session::SeekableReader + 'static>,
    start: u64,
    end: u64,
    total: u64,
    mime: &str,
) -> Response {
    if reader.seek(std::io::SeekFrom::Start(start)).await.is_err() {
        return unsatisfiable_response(total, mime);
    }
    let len = end - start + 1;
    let limited = reader.take(len);
    let body = Body::from_stream(ReaderStream::with_capacity(limited, STREAM_BUFFER));
    Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(CONTENT_TYPE, mime)
        .header(CONTENT_LENGTH, len)
        .header(CONTENT_RANGE, format!("bytes {start}-{end}/{total}"))
        .header(ACCEPT_RANGES, "bytes")
        .body(body)
        .unwrap()
}

fn unsatisfiable_response(total: u64, mime: &str) -> Response {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(CONTENT_TYPE, mime)
        .header(CONTENT_RANGE, format!("bytes */{total}"))
        .body(Body::empty())
        .unwrap()
}

fn text_response(status: StatusCode, msg: impl Into<String>) -> Response {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(msg.into()))
        .unwrap()
}

fn retry_response(reason: String) -> Response {
    Response::builder()
        .status(StatusCode::SERVICE_UNAVAILABLE)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(RETRY_AFTER, "5")
        .body(Body::from(reason))
        .unwrap()
}

/// Hold a request out while the engine checks files, which is usually seconds,
/// so a player that does not retry still gets its bytes. `None` means the
/// torrent can be served now; anything else is the response to send.
async fn wait_until_servable(state: &StreamState, info_hash: &str) -> Option<Response> {
    let deadline = tokio::time::Instant::now() + CHECK_WAIT;
    loop {
        let Some(handle) = state.session.get(info_hash) else {
            return Some(text_response(StatusCode::NOT_FOUND, "file not found"));
        };
        match commands::unservable(&handle) {
            None => return None,
            Some(Unservable::Failed(reason)) => {
                return Some(text_response(StatusCode::CONFLICT, reason));
            }
            Some(Unservable::Checking(reason)) => {
                if tokio::time::Instant::now() >= deadline {
                    return Some(retry_response(reason));
                }
                tokio::time::sleep(CHECK_POLL).await;
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum RangeOutcome {
    None,
    Partial { start: u64, end: u64 },
    Invalid,
}

fn parse_range(value: Option<&str>, file_size: u64) -> RangeOutcome {
    let Some(v) = value else { return RangeOutcome::None };
    let trimmed = v.trim();
    let Some(spec) = trimmed.strip_prefix("bytes=") else {
        return RangeOutcome::Invalid;
    };
    if spec.contains(',') {
        return RangeOutcome::Invalid;
    }
    let Some((a, b)) = spec.split_once('-') else {
        return RangeOutcome::Invalid;
    };
    let (a, b) = (a.trim(), b.trim());

    if a.is_empty() && b.is_empty() {
        return RangeOutcome::Invalid;
    }

    if a.is_empty() {
        let suffix: u64 = match b.parse() {
            Ok(s) if s > 0 => s,
            _ => return RangeOutcome::Invalid,
        };
        if file_size == 0 {
            return RangeOutcome::Invalid;
        }
        let suffix = suffix.min(file_size);
        return RangeOutcome::Partial {
            start: file_size - suffix,
            end: file_size - 1,
        };
    }

    let start: u64 = match a.parse() {
        Ok(s) => s,
        Err(_) => return RangeOutcome::Invalid,
    };
    if start >= file_size {
        return RangeOutcome::Invalid;
    }

    let end = if b.is_empty() {
        file_size - 1
    } else {
        // An end at/beyond EOF clamps to the last byte (RFC 7233); players send this.
        match b.parse::<u64>() {
            Ok(e) if e >= start => e.min(file_size - 1),
            _ => return RangeOutcome::Invalid,
        }
    };

    RangeOutcome::Partial { start, end }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_none_when_header_absent() {
        assert_eq!(parse_range(None, 1000), RangeOutcome::None);
    }

    #[test]
    fn parse_range_full_form() {
        assert_eq!(
            parse_range(Some("bytes=100-200"), 1000),
            RangeOutcome::Partial { start: 100, end: 200 }
        );
    }

    #[test]
    fn parse_range_open_ended() {
        assert_eq!(
            parse_range(Some("bytes=100-"), 1000),
            RangeOutcome::Partial { start: 100, end: 999 }
        );
    }

    #[test]
    fn parse_range_suffix() {
        assert_eq!(
            parse_range(Some("bytes=-200"), 1000),
            RangeOutcome::Partial { start: 800, end: 999 }
        );
    }

    #[test]
    fn parse_range_suffix_clamped_to_file_size() {
        assert_eq!(
            parse_range(Some("bytes=-5000"), 1000),
            RangeOutcome::Partial { start: 0, end: 999 }
        );
    }

    #[test]
    fn parse_range_multi_range_rejected() {
        assert_eq!(parse_range(Some("bytes=0-100,200-300"), 1000), RangeOutcome::Invalid);
    }

    #[test]
    fn parse_range_beyond_eof_rejected() {
        assert_eq!(parse_range(Some("bytes=1000-"), 1000), RangeOutcome::Invalid);
        assert_eq!(parse_range(Some("bytes=2000-3000"), 1000), RangeOutcome::Invalid);
    }

    #[test]
    fn parse_range_end_beyond_eof_clamps() {
        assert_eq!(
            parse_range(Some("bytes=0-999999"), 1000),
            RangeOutcome::Partial { start: 0, end: 999 }
        );
        assert_eq!(
            parse_range(Some("bytes=500-999999"), 1000),
            RangeOutcome::Partial { start: 500, end: 999 }
        );
    }

    #[test]
    fn parse_range_empty_file_any_range_invalid() {
        assert_eq!(parse_range(Some("bytes=-100"), 0), RangeOutcome::Invalid);
    }

    #[test]
    fn parse_range_invalid_syntax() {
        assert_eq!(parse_range(Some("foo=0-100"), 1000), RangeOutcome::Invalid);
        assert_eq!(parse_range(Some("bytes=abc-def"), 1000), RangeOutcome::Invalid);
        assert_eq!(parse_range(Some("bytes=-"), 1000), RangeOutcome::Invalid);
    }
}
