use std::sync::Arc;
use std::time::{Duration, Instant};

use librqbit::TorrentStatsState;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::daemon::DaemonEvent;
use crate::daemon::session::SessionHandle;

const METADATA_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub fn spawn(
    session: Arc<SessionHandle>,
    inbox: mpsc::Sender<DaemonEvent>,
    info_hash: String,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let start = Instant::now();
        loop {
            if start.elapsed() > METADATA_TIMEOUT {
                let _ = inbox
                    .send(DaemonEvent::MetadataFailed {
                        info_hash,
                        error: format!(
                            "metadata not resolved within {}s",
                            METADATA_TIMEOUT.as_secs()
                        ),
                    })
                    .await;
                return;
            }
            if let Some(handle) = session.get(&info_hash) {
                match handle.stats().state {
                    TorrentStatsState::Initializing => {}
                    TorrentStatsState::Error => {
                        let _ = inbox
                            .send(DaemonEvent::MetadataFailed {
                                info_hash,
                                error: "torrent entered error state".into(),
                            })
                            .await;
                        return;
                    }
                    _ => {
                        if handle.with_metadata(|_| ()).is_ok() {
                            let _ = inbox
                                .send(DaemonEvent::MetadataResolved { info_hash })
                                .await;
                            return;
                        }
                    }
                }
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    })
}
