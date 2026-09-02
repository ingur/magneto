//! Waits for a torrent's file check to finish.
//!
//! Nothing may touch a torrent while the engine checks its files: selection
//! changes are refused outright, and an unpause landing in that window finishes
//! with the engine's pause flag out of step with its state, which strands the
//! torrent. The engine side of finalizing waits here instead.

use std::time::Duration;

use librqbit::TorrentStatsState;

use crate::daemon::commands::Finalize;
use crate::daemon::{Daemon, DaemonEvent};

const POLL: Duration = Duration::from_millis(500);

/// What to do with a torrent once its check ends. Held by the daemon so a
/// second waiter is never spawned for the same torrent, and so the decision
/// survives a waiter that outlives the torrent it was started for.
#[derive(Debug, Clone, Copy)]
pub struct Pending {
    pub from: Finalize,
    pub repause: bool,
}

/// Hand `info_hash` back to the event loop once it stops checking files, in
/// whatever state that leaves it. A check the user pauses stays Initializing
/// with no check running, so this keeps waiting: resuming restarts it, and
/// finalizing then is exactly right.
///
/// Calling this again for a torrent already being waited on replaces the
/// decision only with a more deliberate one, so a re-add during a boot check
/// applies the add policy and does not inherit the boot's pause intent.
pub fn spawn(daemon: &mut Daemon, info_hash: &str, from: Finalize, repause: bool) {
    if let Some(pending) = daemon.checks.get_mut(info_hash) {
        if deliberateness(from) > deliberateness(pending.from) {
            *pending = Pending { from, repause };
        }
        return;
    }
    daemon.checks.insert(info_hash.to_string(), Pending { from, repause });

    let session = daemon.session.clone();
    let inbox = daemon.inbox_tx.clone();
    let cancel = daemon.cancel.clone();
    let info_hash = info_hash.to_string();
    tokio::spawn(async move {
        let Some(id) = session.get(&info_hash).map(|h| h.id()) else { return };
        loop {
            // A torrent removed and re-added under the same hash is a different
            // torrent, and its own finalize owns it.
            let Some(handle) = session.get(&info_hash).filter(|h| h.id() == id) else {
                return;
            };
            if !matches!(handle.stats().state, TorrentStatsState::Initializing { .. }) {
                break;
            }
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(POLL) => {}
            }
        }
        let _ = inbox.send(DaemonEvent::CheckFinished { info_hash }).await;
    });
}

/// The user asking beats startup, which beats the engine's own state.
fn deliberateness(from: Finalize) -> u8 {
    match from {
        Finalize::Restore => 0,
        Finalize::Boot => 1,
        Finalize::Add => 2,
    }
}
