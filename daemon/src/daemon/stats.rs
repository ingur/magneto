use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use magneto_core::config::Config;
use crate::daemon::session::SessionHandle;
use crate::daemon::{DaemonEvent, commands};
use crate::metadata::MetadataStore;
use magneto_core::protocol::{FileState, FileStatsDelta, StatsEvent, TorrentState, TorrentStatsDelta};

// Suppress speed deltas below this change (~0.05 MiB/s). download_speed and
// upload_speed are bytes per second on the wire (see protocol.rs).
const SPEED_DELTA_BYTES_PER_SEC: f64 = 50_000.0;
const BYTES_DELTA_FRACTION: f64 = 0.005;
// A non-head file LIGHTS UP as actively downloading only after gaining bytes
// on two consecutive 1s ticks. A single piece of boundary/prefetch spillover
// near a file handoff must not flash the file-after-next as Downloading.
// Once lit, it STAYS lit until this many gainless ticks pass, so a bursty but
// real transfer (an out-of-order stream) doesn't strobe between Queued and
// Downloading.
const ACTIVE_GRACE_TICKS: u32 = 4;

#[derive(Clone)]
struct LastSent {
    state: TorrentState,
    // Thresholded fields hold the last value actually SENT, not the last
    // observed: comparing against the previous tick would let a steady
    // below-threshold trickle drift unboundedly without ever flushing (a
    // download slower than the threshold per tick would freeze the client's
    // progress bar forever).
    progress_bytes: u64,
    total_bytes_selected: u64,
    download_speed: f64,
    upload_speed: f64,
    is_paused: bool,
    is_seeding: bool,
    complete_count: u32,
    selected_count: u32,
    persisted_count: u32,
    shared_count: u32,
    // Last SENT per-file bytes (wire threshold), and the raw previous-tick
    // bytes (gain detection needs true tick-over-tick deltas).
    file_progress: HashMap<u32, u64>,
    file_progress_tick: HashMap<u32, u64>,
    file_state: HashMap<u32, FileState>,
    // Ticks since each file last gained bytes, driving the recently-active
    // window that decides Downloading vs Queued.
    ticks_since_gain: HashMap<u32, u32>,
}

pub fn spawn(
    cancel: CancellationToken,
    inbox: mpsc::Sender<DaemonEvent>,
    session: Arc<SessionHandle>,
    metadata: Arc<RwLock<MetadataStore>>,
    config_rx: watch::Receiver<Config>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_sent: HashMap<String, LastSent> = HashMap::new();
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                _ = ticker.tick() => {}
            }
            // Clone the latest config each tick so a hot-applied set_config (media
            // extensions, persist/share defaults) is reflected without a restart.
            let config = config_rx.borrow().clone();
            let (completes, event) = build_tick(&session, &metadata, &config, &mut last_sent);
            for hash in completes {
                if inbox
                    .send(DaemonEvent::TorrentCompletedTick { info_hash: hash })
                    .await
                    .is_err()
                {
                    return;
                }
            }
            if !event.is_empty()
                && inbox.send(DaemonEvent::StatsReady(event)).await.is_err()
            {
                return;
            }
        }
    })
}

fn build_tick(
    session: &Arc<SessionHandle>,
    metadata: &Arc<RwLock<MetadataStore>>,
    config: &Config,
    last_sent: &mut HashMap<String, LastSent>,
) -> (Vec<String>, StatsEvent) {
    let mut completes = Vec::new();
    let mut event = StatsEvent::default();
    let infohashes = session.list_infohashes();
    let meta = metadata.read();

    for hash in &infohashes {
        let Some(handle) = session.get(hash) else { continue };
        let entry = meta.get(hash);
        let stats = handle.stats();
        let progress = commands::media_file_progress(&handle, &stats, entry, config);
        let last = last_sent.get(hash);

        // Recently-active window from tick history: a file joins only after gains
        // on two consecutive ticks (prev_ticks == Some(0) = the previous tick also
        // gained); an already-Downloading file keeps the slot while its gainless
        // streak stays inside the grace window. History-less ticks (restart, fresh
        // torrent) light nothing.
        let mut recently_active: HashSet<u32> = HashSet::new();
        let mut new_ticks_since_gain: HashMap<u32, u32> = HashMap::new();
        for f in &progress {
            let prev_down =
                last.and_then(|l| l.file_progress_tick.get(&f.index).copied()).unwrap_or(0);
            let gained = f.downloaded > prev_down;
            let prev_ticks = last.and_then(|l| l.ticks_since_gain.get(&f.index).copied());
            let ticks = if gained {
                0
            } else {
                prev_ticks.map_or(ACTIVE_GRACE_TICKS, |t| t.saturating_add(1))
            };
            let was_lit = last.and_then(|l| l.file_state.get(&f.index).copied())
                == Some(FileState::Downloading);
            let lit = if was_lit {
                ticks < ACTIVE_GRACE_TICKS
            } else {
                gained && prev_ticks == Some(0)
            };
            if lit {
                recently_active.insert(f.index);
            }
            new_ticks_since_gain.insert(f.index, ticks);
        }

        // One assembly: the file rows (warm active set) and the summary derived
        // from them, so summary state can't diverge from the per-file deltas.
        let files =
            commands::render_file_entries(stats.state, &progress, entry, config, Some(&recently_active));
        let summary = commands::summarize(&handle, &stats, entry, &files);

        // Complete is announced only on an observed Downloading -> Complete
        // transition: a torrent first seen complete, or seen Initializing while a
        // fresh daemon re-validates fastresume, finished in an earlier life, and
        // re-announcing those would flood every client on each daemon start. The
        // summary delta below also fires (state_changed); both are intentional,
        // the UI treats state as latest-wins, so the duplicate is harmless.
        let was_downloading =
            last.map(|l| l.state == TorrentState::Downloading).unwrap_or(false);
        let now_complete = summary.state == TorrentState::Complete;
        if now_complete && was_downloading {
            completes.push(hash.clone());
        }

        let total_bytes_selected = summary.total_bytes_selected;
        let bytes_threshold =
            (((total_bytes_selected as f64) * BYTES_DELTA_FRACTION).ceil() as u64).max(1);
        let state_changed = last.map(|l| l.state != summary.state).unwrap_or(true);
        // Downloading bypasses the byte threshold (the client animates between 1s
        // ticks, so per-tick gains must reach the wire), and a state transition
        // flushes any suppressed remainder so the bar lands exactly where the new
        // state says (complete = full). Other states keep threshold quiescence.
        let progress_changed = last
            .map(|l| {
                let diff = summary.downloaded_bytes.abs_diff(l.progress_bytes);
                diff >= bytes_threshold
                    || (diff > 0 && (summary.state == TorrentState::Downloading || state_changed))
            })
            .unwrap_or(true);
        let selected_bytes_changed = last
            .map(|l| l.total_bytes_selected != total_bytes_selected)
            .unwrap_or(true);
        let dl_changed = last
            .map(|l| (summary.download_speed - l.download_speed).abs() >= SPEED_DELTA_BYTES_PER_SEC)
            .unwrap_or(true);
        let ul_changed = last
            .map(|l| (summary.upload_speed - l.upload_speed).abs() >= SPEED_DELTA_BYTES_PER_SEC)
            .unwrap_or(true);
        let is_paused_changed = last.map(|l| l.is_paused != summary.is_paused).unwrap_or(true);
        let is_seeding_changed = last.map(|l| l.is_seeding != summary.is_seeding).unwrap_or(true);
        // What the cache will say after this tick: current when (re)sent, the
        // previously-sent value otherwise, so suppressed drift keeps
        // accumulating against the threshold instead of resetting every tick.
        let sent_progress_bytes = if progress_changed {
            summary.downloaded_bytes
        } else {
            last.map(|l| l.progress_bytes).unwrap_or(summary.downloaded_bytes)
        };
        let sent_download_speed = if dl_changed {
            summary.download_speed
        } else {
            last.map(|l| l.download_speed).unwrap_or(summary.download_speed)
        };
        let sent_upload_speed = if ul_changed {
            summary.upload_speed
        } else {
            last.map(|l| l.upload_speed).unwrap_or(summary.upload_speed)
        };
        let complete_count_changed = last
            .map(|l| l.complete_count != summary.complete_count)
            .unwrap_or(true);
        let selected_count_changed = last
            .map(|l| l.selected_count != summary.selected_count)
            .unwrap_or(true);
        let persisted_count_changed = last
            .map(|l| l.persisted_count != summary.persisted_count)
            .unwrap_or(true);
        let shared_count_changed = last
            .map(|l| l.shared_count != summary.shared_count)
            .unwrap_or(true);

        let include_torrent = state_changed
            || progress_changed
            || selected_bytes_changed
            || dl_changed
            || ul_changed
            || is_paused_changed
            || is_seeding_changed
            || complete_count_changed
            || selected_count_changed
            || persisted_count_changed
            || shared_count_changed;

        if include_torrent {
            event.torrents.push(TorrentStatsDelta {
                info_hash: hash.clone(),
                state: state_changed.then_some(summary.state),
                downloaded_bytes: progress_changed.then_some(summary.downloaded_bytes),
                total_bytes_selected: selected_bytes_changed.then_some(total_bytes_selected),
                download_speed: dl_changed.then_some(summary.download_speed),
                upload_speed: ul_changed.then_some(summary.upload_speed),
                is_paused: is_paused_changed.then_some(summary.is_paused),
                is_seeding: is_seeding_changed.then_some(summary.is_seeding),
                complete_count: complete_count_changed.then_some(summary.complete_count),
                selected_count: selected_count_changed.then_some(summary.selected_count),
                persisted_count: persisted_count_changed.then_some(summary.persisted_count),
                shared_count: shared_count_changed.then_some(summary.shared_count),
            });
        }

        // Per-file deltas from the rendered rows; the threshold suppresses
        // unchanged files, so a sustained-complete torrent emits nothing while a
        // complete-transition tick still flushes the final state=Complete row.
        let mut new_file_progress: HashMap<u32, u64> = HashMap::new();
        let mut new_file_progress_tick: HashMap<u32, u64> = HashMap::new();
        let mut new_file_state: HashMap<u32, FileState> = HashMap::new();
        for f in &files {
            new_file_state.insert(f.index, f.state);
            new_file_progress_tick.insert(f.index, f.downloaded_bytes);
            let last_down =
                last.and_then(|l| l.file_progress.get(&f.index).copied()).unwrap_or(0);
            let prev_state = last.and_then(|l| l.file_state.get(&f.index).copied());
            let file_threshold = (((f.size as f64) * BYTES_DELTA_FRACTION).ceil() as u64).max(1);
            let diff = f.downloaded_bytes.abs_diff(last_down);
            let bytes_changed =
                diff >= file_threshold || (diff > 0 && f.state == FileState::Downloading);
            let state_changed = prev_state != Some(f.state);
            if bytes_changed || state_changed {
                new_file_progress.insert(f.index, f.downloaded_bytes);
                event.files.push(FileStatsDelta {
                    info_hash: hash.clone(),
                    file_index: f.index,
                    downloaded_bytes: f.downloaded_bytes,
                    state: f.state,
                });
            } else {
                new_file_progress.insert(f.index, last_down);
            }
        }

        last_sent.insert(
            hash.clone(),
            LastSent {
                state: summary.state,
                progress_bytes: sent_progress_bytes,
                total_bytes_selected: summary.total_bytes_selected,
                download_speed: sent_download_speed,
                upload_speed: sent_upload_speed,
                is_paused: summary.is_paused,
                is_seeding: summary.is_seeding,
                complete_count: summary.complete_count,
                selected_count: summary.selected_count,
                persisted_count: summary.persisted_count,
                shared_count: summary.shared_count,
                file_progress: new_file_progress,
                file_progress_tick: new_file_progress_tick,
                file_state: new_file_state,
                ticks_since_gain: new_ticks_since_gain,
            },
        );
    }

    let still_alive: std::collections::HashSet<&String> = infohashes.iter().collect();
    last_sent.retain(|hash, _| still_alive.contains(hash));

    (completes, event)
}
