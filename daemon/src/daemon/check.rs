//! Waits for a torrent's file check to finish.
//!
//! Nothing may touch a torrent while the engine checks its files: selection
//! changes are refused outright, and an unpause landing in that window finishes
//! with the engine's pause flag out of step with its state, which strands the
//! torrent. The engine side of finalizing waits here instead.

use std::time::Duration;

use librqbit::TorrentStatsState;
use tracing::debug;

use crate::daemon::commands::Finalize;
use crate::daemon::{Daemon, DaemonEvent, short};

const POLL: Duration = Duration::from_millis(500);

/// Hand `info_hash` back to the event loop once it stops checking files, in
/// whatever state that leaves it. Ends quietly if the torrent is removed or the
/// daemon shuts down. A check the user pauses stays Initializing with no check
/// running, and this keeps waiting: resuming restarts it, and finalizing then is
/// exactly right.
pub fn spawn(daemon: &Daemon, info_hash: &str, from: Finalize, repause: bool) {
    let session = daemon.session.clone();
    let inbox = daemon.inbox_tx.clone();
    let cancel = daemon.cancel.clone();
    let info_hash = info_hash.to_string();
    tokio::spawn(async move {
        loop {
            let Some(handle) = session.get(&info_hash) else { return };
            if !matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
                break;
            }
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(POLL) => {}
            }
        }
        debug!(hash = %short(&info_hash), "file check finished");
        let _ = inbox.send(DaemonEvent::CheckFinished { info_hash, from, repause }).await;
    });
}
