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
use magneto_core::protocol::{
    FileState, FileStatsDelta, StatsEvent, TorrentState, TorrentStatsDelta, TorrentSummary,
};

// Suppress speed deltas below this change (~0.05 MiB/s). download_speed and
// upload_speed are bytes per second on the wire (see protocol.rs).
const SPEED_DELTA_BYTES_PER_SEC: f64 = 50_000.0;
const BYTES_DELTA_FRACTION: f64 = 0.005;
// A file LIGHTS UP as actively downloading only after gaining bytes on two
// consecutive 1s ticks. A single piece of boundary/prefetch spillover near a
// file handoff must not flash the file-after-next as Downloading. Once lit, it
// STAYS lit until this many gainless ticks pass, so a bursty but real transfer
// (an out-of-order stream) doesn't strobe between Queued and Downloading.
const ACTIVE_GRACE_TICKS: u32 = 4;

#[derive(Clone)]
struct LastSent {
    // The summary as last SENT, not as last observed: comparing against the
    // previous tick would let a steady below-threshold trickle drift
    // unboundedly without ever flushing (a download slower than the threshold
    // per tick would freeze the client's progress bar forever).
    summary: TorrentSummary,
    // Last SENT per-file bytes (wire threshold), and the raw previous-tick
    // bytes (gain detection needs true tick-over-tick deltas).
    file_progress: HashMap<u32, u64>,
    file_progress_tick: HashMap<u32, u64>,
    file_state: HashMap<u32, FileState>,
    // Ticks since each file last gained bytes, driving the recently-active
    // window that decides Downloading vs Queued.
    ticks_since_gain: HashMap<u32, u32>,
}

/// What changed since the last patch went out, and the summary to remember as
/// sent. A torrent with no history yet gets every field, because the event that
/// announced it was rendered before its add finished. Destructuring `next` in
/// full is deliberate: adding a summary field stops this compiling until it is
/// handled here, so the wire can never carry a field a patch cannot refresh.
fn summary_delta(
    sent: Option<&TorrentSummary>,
    next: &TorrentSummary,
) -> (TorrentStatsDelta, TorrentSummary) {
    let TorrentSummary {
        info_hash,
        name,
        source,
        source_kind,
        state,
        error,
        check_progress,
        total_bytes_all,
        total_bytes_selected,
        downloaded_bytes,
        download_speed,
        upload_speed,
        file_count,
        complete_count,
        selected_count,
        persisted_count,
        shared_count,
        is_paused,
        added_at,
    } = next;

    // A field with nothing to compare against is a field the client needs.
    let fresh = sent.is_none();
    let state_changed = fresh || sent.is_some_and(|s| s.state != *state);
    // Downloading bypasses the byte threshold (the client animates between
    // ticks, so per-tick gains must reach the wire), and a state transition
    // flushes any suppressed remainder so the bar lands where the new state
    // says. Other states keep threshold quiescence.
    let bytes_threshold =
        (((*total_bytes_selected as f64) * BYTES_DELTA_FRACTION).ceil() as u64).max(1);
    let progress_diff = sent.map_or(0, |s| downloaded_bytes.abs_diff(s.downloaded_bytes));
    let progress_changed = fresh
        || progress_diff >= bytes_threshold
        || (progress_diff > 0
            && (matches!(state, TorrentState::Downloading | TorrentState::Stalled)
                || state_changed));
    let dl_changed = fresh
        || sent.is_some_and(|s| (download_speed - s.download_speed).abs() >= SPEED_DELTA_BYTES_PER_SEC);
    let ul_changed = fresh
        || sent.is_some_and(|s| (upload_speed - s.upload_speed).abs() >= SPEED_DELTA_BYTES_PER_SEC);

    let delta = TorrentStatsDelta {
        info_hash: info_hash.clone(),
        name: changed(sent.map(|s| &s.name), name).then(|| name.clone()),
        source: changed(sent.map(|s| &s.source), source).then(|| source.clone()),
        source_kind: changed(sent.map(|s| &s.source_kind), source_kind).then_some(*source_kind),
        state: state_changed.then_some(*state),
        error: changed(sent.map(|s| &s.error), error).then(|| error.clone()),
        check_progress: changed(sent.map(|s| &s.check_progress), check_progress).then_some(*check_progress),
        total_bytes_all: changed(sent.map(|s| &s.total_bytes_all), total_bytes_all).then_some(*total_bytes_all),
        total_bytes_selected: changed(sent.map(|s| &s.total_bytes_selected), total_bytes_selected)
            .then_some(*total_bytes_selected),
        downloaded_bytes: progress_changed.then_some(*downloaded_bytes),
        download_speed: dl_changed.then_some(*download_speed),
        upload_speed: ul_changed.then_some(*upload_speed),
        file_count: changed(sent.map(|s| &s.file_count), file_count).then_some(*file_count),
        complete_count: changed(sent.map(|s| &s.complete_count), complete_count).then_some(*complete_count),
        selected_count: changed(sent.map(|s| &s.selected_count), selected_count).then_some(*selected_count),
        persisted_count: changed(sent.map(|s| &s.persisted_count), persisted_count).then_some(*persisted_count),
        shared_count: changed(sent.map(|s| &s.shared_count), shared_count).then_some(*shared_count),
        is_paused: changed(sent.map(|s| &s.is_paused), is_paused).then_some(*is_paused),
        added_at: changed(sent.map(|s| &s.added_at), added_at).then(|| added_at.clone()),
    };

    // Thresholded fields keep their last sent value, so suppressed drift keeps
    // accumulating against the threshold instead of resetting every tick.
    let remembered = TorrentSummary {
        downloaded_bytes: pick(progress_changed, *downloaded_bytes, sent.map(|s| s.downloaded_bytes)),
        download_speed: pick(dl_changed, *download_speed, sent.map(|s| s.download_speed)),
        upload_speed: pick(ul_changed, *upload_speed, sent.map(|s| s.upload_speed)),
        ..next.clone()
    };
    (delta, remembered)
}

fn changed<T: PartialEq>(sent: Option<&T>, next: &T) -> bool {
    sent != Some(next)
}

fn pick<T>(sent_now: bool, next: T, sent: Option<T>) -> T {
    if sent_now { next } else { sent.unwrap_or(next) }
}

pub fn spawn(
    cancel: CancellationToken,
    inbox: mpsc::Sender<DaemonEvent>,
    session: Arc<SessionHandle>,
    metadata: Arc<RwLock<MetadataStore>>,
    active: Arc<RwLock<HashMap<String, HashSet<u32>>>>,
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
            let tick = build_tick(&session, &metadata, &active, &config, &mut last_sent);
            for info_hash in tick.completed {
                if inbox.send(DaemonEvent::TorrentCompletedTick { info_hash }).await.is_err() {
                    return;
                }
            }
            for info_hash in tick.errored {
                if inbox.send(DaemonEvent::TorrentErrored { info_hash }).await.is_err() {
                    return;
                }
            }
            let event = tick.event;
            if !event.is_empty()
                && inbox.send(DaemonEvent::StatsReady(event)).await.is_err()
            {
                return;
            }
        }
    })
}

/// What one tick produced: the torrents that just finished, the ones the engine
/// just put in the error state, and the deltas for every client.
struct Tick {
    completed: Vec<String>,
    errored: Vec<String>,
    event: StatsEvent,
}

fn build_tick(
    session: &Arc<SessionHandle>,
    metadata: &Arc<RwLock<MetadataStore>>,
    active: &RwLock<HashMap<String, HashSet<u32>>>,
    config: &Config,
    last_sent: &mut HashMap<String, LastSent>,
) -> Tick {
    let mut completed = Vec::new();
    let mut errored = Vec::new();
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
        // Per-file bytes only move when a whole piece verifies, which can be
        // slower than a tick, so a healthy download would read stalled. The
        // engine's own speed sees the chunks, and it works through files in
        // path order, so the head is what is receiving.
        if recently_active.is_empty()
            && stats.live.as_ref().is_some_and(|l| l.download_speed.mbps > 0.0)
            && let Some(head) = commands::head_pending(&progress)
        {
            recently_active.insert(head);
        }

        // Published so a render on request sees the same active files.
        active.write().insert(hash.clone(), recently_active.clone());
        // One assembly: the file rows (warm active set) and the summary derived
        // from them, so summary state can't diverge from the per-file deltas.
        let files =
            commands::render_file_entries(stats.state, &progress, entry, config, &recently_active);
        let summary = commands::summarize(&handle, &stats, entry, &files);

        // Complete is announced only on an observed transition out of a live
        // state: a torrent first seen complete, or seen Initializing while a
        // fresh daemon re-validates fastresume, finished in an earlier life, and
        // re-announcing those would flood every client on each daemon start. The
        // summary patch below also fires; the UI treats state as latest-wins, so
        // the duplicate is harmless.
        let was_live = last.is_some_and(|l| {
            matches!(l.summary.state, TorrentState::Downloading | TorrentState::Stalled)
        });
        if summary.state == TorrentState::Complete && was_live {
            completed.push(hash.clone());
        }
        // A torrent found in error needs a re-check, whether it errored just now
        // or was already errored when this daemon started.
        if summary.state == TorrentState::Error
            && last.map(|l| l.summary.state != TorrentState::Error).unwrap_or(true)
        {
            errored.push(hash.clone());
        }

        let (delta, remembered) = summary_delta(last.map(|l| &l.summary), &summary);
        if !delta.is_empty() {
            event.torrents.push(delta);
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
                summary: remembered,
                file_progress: new_file_progress,
                file_progress_tick: new_file_progress_tick,
                file_state: new_file_state,
                ticks_since_gain: new_ticks_since_gain,
            },
        );
    }

    let still_alive: HashSet<&String> = infohashes.iter().collect();
    last_sent.retain(|hash, _| still_alive.contains(hash));
    active.write().retain(|hash, _| still_alive.contains(hash));

    Tick { completed, errored, event }
}
