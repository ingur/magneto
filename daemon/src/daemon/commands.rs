use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Context;
use base64::Engine;
use librqbit::{TorrentStats, TorrentStatsState};
use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use tracing::{error, info, warn};

use magneto_core::config::Config;
use crate::daemon::preflight;
use crate::daemon::session::{AddOutcome, SessionHandle, TorrentHandle};
use crate::daemon::{ClientId, Daemon, DaemonEvent, check, removal, session, short};
use crate::media;
use crate::metadata::{FileMetadata, MetadataStore, TorrentMetadata};
use crate::player;
use magneto_core::protocol::{
    AddTorrentReq, AddTorrentResp, AffectedResp, ConfigChangedEvent, FallbackReason, FallbackReq,
    FallbackResp, FileEntry, FileState, GetTorrentReq, ListTorrentsResp, OkResp, Outbound,
    PathKind, PingResp, PlayItem, PlayResp, PlayerLaunchFailedEvent, PlayerLaunchKind,
    RemovalReason, RemoveTorrentReq, Request, ResolveLocalPathReq, ResolveLocalPathResp,
    SetConfigResp, SetPersistReq, SetSharedReq, SourceKind, Target, TargetsReq, TorrentAddedEvent,
    TorrentDetail, TorrentState, TorrentSummary,
};

/// Handle one client request. `None` means the reply is deferred: add_torrent
/// resolves magnet metadata inside librqbit with no upper bound, so it runs
/// off the event loop and answers later via [`DaemonEvent::AddCompleted`].
pub async fn dispatch(daemon: &mut Daemon, client: ClientId, req: Request) -> Option<Outbound> {
    let Request { kind, id, payload } = req;
    if kind == "add_torrent" {
        return match serde_json::from_value::<AddTorrentReq>(payload) {
            Ok(p) => start_add_torrent(daemon, client, id, p),
            Err(e) => Some(Outbound::error(id, format!("invalid payload: {e}"))),
        };
    }
    Some(match kind.as_str() {
        "ping" => Outbound::response(id, PingResp::TRUE),
        "list_torrents" => handle_list_torrents(daemon, id).await,
        "get_torrent" => with_payload(id, payload, |id, p| handle_get_torrent(daemon, id, p)).await,
        "get_config" => Outbound::response(id, daemon.config.clone()),
        "remove_torrent" => {
            with_payload(id, payload, |id, p| handle_remove_torrent(daemon, id, p)).await
        }
        "fallback" => with_payload(id, payload, |id, p| handle_fallback(daemon, id, p)).await,
        "pause" => with_payload(id, payload, |id, p| handle_pause(daemon, id, p)).await,
        "resume" => with_payload(id, payload, |id, p| handle_resume(daemon, id, p)).await,
        "drop_targets" => {
            with_payload(id, payload, |id, p| handle_drop_targets(daemon, id, p)).await
        }
        "set_persist" => {
            with_payload(id, payload, |id, p| handle_set_persist(daemon, id, p)).await
        }
        "set_shared" => with_payload(id, payload, |id, p| handle_set_shared(daemon, id, p)).await,
        "play" => with_payload(id, payload, |id, p| handle_play(daemon, id, p)).await,
        "resolve_local_path" => {
            with_payload(id, payload, |id, p| handle_resolve_local_path(daemon, id, p)).await
        }
        "set_config" => handle_set_config(daemon, id, payload).await,
        "restart" => handle_restart(daemon, id).await,
        "shutdown" => handle_shutdown(daemon, id).await,
        _ => Outbound::error(id, format!("unknown command: {kind}")),
    })
}

pub async fn list_summaries(daemon: &Daemon) -> Vec<TorrentSummary> {
    let infohashes = daemon.session.list_infohashes();
    let meta = daemon.metadata.read();
    infohashes
        .iter()
        .filter_map(|h| {
            let handle = daemon.session.get(h)?;
            Some(render_torrent_summary(&handle, meta.get(h), &daemon.config))
        })
        .collect()
}

async fn handle_list_torrents(daemon: &Daemon, id: String) -> Outbound {
    let torrents = list_summaries(daemon).await;
    Outbound::response(id, ListTorrentsResp { torrents })
}

async fn handle_get_torrent(daemon: &Daemon, id: String, req: GetTorrentReq) -> Outbound {
    let Some(handle) = daemon.session.get(&req.info_hash) else {
        return Outbound::error(id, format!("no torrent with info_hash {}", req.info_hash));
    };
    let meta_guard = daemon.metadata.read();
    let detail = render_torrent_detail(&handle, meta_guard.get(&req.info_hash), &daemon.config);
    Outbound::response(id, detail)
}

// ---- add_torrent + finalize ----

/// An add must not run on the event loop: librqbit resolves magnet metadata
/// inside add_torrent and a dead swarm never resolves, which would freeze
/// every other command (and shutdown) behind it. The session call runs in its
/// own task under a watchdog; the reply reaches the client via AddCompleted.
const ADD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

fn start_add_torrent(
    daemon: &Daemon,
    client: ClientId,
    id: String,
    req: AddTorrentReq,
) -> Option<Outbound> {
    let source_input = req.source.trim().to_string();
    let kind = detect_source_kind(&source_input);
    let bytes = match kind {
        SourceKind::File => match base64::engine::general_purpose::STANDARD.decode(&source_input) {
            Ok(b) => Some(b),
            Err(e) => {
                return Some(Outbound::error(id, format!("invalid base64 torrent bytes: {e}")));
            }
        },
        SourceKind::Magnet | SourceKind::Url => None,
    };
    let session = daemon.session.clone();
    let inbox = daemon.inbox_tx.clone();
    let cancel = daemon.cancel.clone();
    tokio::spawn(async move {
        let add = async {
            match bytes {
                Some(b) => session.add_bytes(b).await,
                None => session.add_url(&source_input).await,
            }
        };
        let outcome = tokio::select! {
            _ = cancel.cancelled() => return,
            res = tokio::time::timeout(ADD_TIMEOUT, add) => match res {
                Ok(r) => r,
                Err(_) => Err(anyhow::anyhow!(
                    "metadata not resolved within {}s",
                    ADD_TIMEOUT.as_secs()
                )),
            },
        };
        let _ = inbox
            .send(DaemonEvent::AddCompleted {
                client,
                request_id: id,
                source: source_input,
                kind,
                outcome,
            })
            .await;
    });
    None
}

pub(crate) async fn finish_add(
    daemon: &mut Daemon,
    id: String,
    source_input: String,
    kind: SourceKind,
    outcome: anyhow::Result<AddOutcome>,
) -> Outbound {
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => return Outbound::error(id, format!("add_torrent failed: {e}")),
    };
    let info_hash = outcome.info_hash;
    let handle = outcome.handle;
    let already_existed = outcome.already_existed;

    // Without a record nothing can finalize, reclaim or remove this torrent
    // later, so a failed write fails the add.
    if !daemon.metadata.read().contains(&info_hash)
        && let Err(e) = persist_source(daemon, &info_hash, &source_input, kind).await
    {
        warn!(hash = %short(&info_hash), error = %e, "failed to record torrent source");
        if let Err(e) = daemon.session.delete(&info_hash, false).await {
            warn!(hash = %short(&info_hash), error = %e, "failed to drop unrecorded torrent");
        }
        return Outbound::error(id, format!("could not record torrent: {e}"));
    }
    // Re-adding a torrent is the user's repair gesture, so it gets a fresh
    // recovery attempt even if the automatic one is spent.
    daemon.rechecked.remove(&info_hash);

    let (state, source) = {
        let meta = daemon.metadata.read();
        let entry = meta.get(&info_hash);
        (
            render_torrent_summary(&handle, entry, &daemon.config).state,
            entry.map(|e| e.source.clone()).unwrap_or_default(),
        )
    };
    daemon.broadcast(Outbound::TorrentAdded(TorrentAddedEvent {
        info_hash: info_hash.clone(),
        source,
        state,
        already_existed,
    }));

    let outcome = finalize_torrent(daemon, &info_hash, Finalize::Add).await;
    let resp = AddTorrentResp {
        info_hash,
        name: handle.name(),
        state: outcome.state,
        files: outcome.files,
        media: outcome.media,
        already_existed,
        fallback_launched: outcome.fallback_launched,
        fallback_reason: outcome.fallback_reason,
    };
    if already_existed {
        try_autoplay_on_readd(daemon, &handle, &resp).await;
    }
    Outbound::response(id, resp)
}

async fn try_autoplay_on_readd(daemon: &Daemon, handle: &TorrentHandle, resp: &AddTorrentResp) {
    if !daemon.config.downloads.autoplay
        || daemon.config.player.command.trim().is_empty()
        || resp.files.as_ref().map(|f| f.is_empty()).unwrap_or(true)
    {
        return;
    }
    let media: Vec<u32> = daemon
        .metadata
        .read()
        .get(&resp.info_hash)
        .map(|m| m.files.keys().copied().collect())
        .unwrap_or_default();
    if media.is_empty() {
        return;
    }
    let items = build_play_items(handle, &media, &daemon.started);
    if items.is_empty() {
        return;
    }
    let uris: Vec<String> = items.into_iter().map(|i| i.uri).collect();
    if let Err(e) = player::launch_player(&daemon.config.player, &uris) {
        daemon.broadcast(Outbound::PlayerLaunchFailed(PlayerLaunchFailedEvent {
            info_hash: Some(resp.info_hash.clone()),
            kind: PlayerLaunchKind::Autoplay,
            error: e.to_string(),
        }));
    }
}

/// Where a finalize came from. An add follows the auto_download setting. A
/// torrent the daemon re-added at boot lost its engine-side state, so it comes
/// back paused with nothing selected: only the user knows whether it should be
/// downloading again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finalize {
    Add,
    Restore,
}

/// Bring the engine and magneto's record in step for one torrent, then tell the
/// clients. Safe to call more than once: the engine side runs only while the
/// record says it has not been applied yet.
pub(crate) async fn finalize_torrent(
    daemon: &mut Daemon,
    info_hash: &str,
    from: Finalize,
) -> FinalizeOutcome {
    let outcome = finalize_inner(daemon, info_hash, from).await;
    if outcome.media == Some(true)
        && let Some(handle) = daemon.session.get(info_hash)
    {
        let detail = {
            let meta = daemon.metadata.read();
            render_torrent_detail(&handle, meta.get(info_hash), &daemon.config)
        };
        daemon.broadcast(Outbound::TorrentReady(detail));
    }
    outcome
}

pub(crate) struct FinalizeOutcome {
    state: Option<TorrentState>,
    files: Option<Vec<FileEntry>>,
    media: Option<bool>,
    fallback_launched: bool,
    fallback_reason: Option<FallbackReason>,
}

impl FinalizeOutcome {
    fn pending(state: TorrentState) -> Self {
        Self {
            state: Some(state),
            files: None,
            media: None,
            fallback_launched: false,
            fallback_reason: None,
        }
    }
}

async fn finalize_inner(daemon: &mut Daemon, info_hash: &str, from: Finalize) -> FinalizeOutcome {
    let Some(handle) = daemon.session.get(info_hash) else {
        return FinalizeOutcome::pending(TorrentState::Error);
    };
    match handle.stats().state {
        // The engine refuses selection changes while it checks files, and an
        // unpause landing in that window strands the torrent, so the engine
        // side of finalizing waits for the check to end.
        TorrentStatsState::Initializing { .. } => {
            check::spawn(daemon, info_hash, from, false);
            return FinalizeOutcome::pending(TorrentState::Initializing);
        }
        TorrentStatsState::Error => {
            classify(daemon, info_hash).await;
            recover_errored(daemon, info_hash, from).await;
            return FinalizeOutcome::pending(TorrentState::Error);
        }
        _ => {}
    }
    let Some(subs) = classify(daemon, info_hash).await else {
        return FinalizeOutcome::pending(TorrentState::Initializing);
    };
    let (media, applied) = {
        let meta = daemon.metadata.read();
        match meta.get(info_hash) {
            Some(entry) => (entry.files.keys().copied().collect::<Vec<u32>>(), entry.finalized),
            None => (Vec::new(), false),
        }
    };
    if media.is_empty() {
        return drop_without_media(daemon, info_hash).await;
    }
    if !applied {
        apply_engine_policy(daemon, info_hash, &media, &subs, from).await;
    }
    let meta = daemon.metadata.read();
    let detail = render_torrent_detail(&handle, meta.get(info_hash), &daemon.config);
    FinalizeOutcome {
        state: Some(detail.summary.state),
        files: Some(detail.files),
        media: Some(true),
        fallback_launched: false,
        fallback_reason: None,
    }
}

/// Record what a torrent contains and hand back its subtitle indices. An
/// existing file map is the user's record and is left alone, so changing
/// media.extensions later cannot rewrite or empty it.
async fn classify(daemon: &mut Daemon, info_hash: &str) -> Option<Vec<u32>> {
    let handle = daemon.session.get(info_hash)?;
    let classified = handle.with_metadata(|m| {
        let media: Vec<u32> = m
            .file_infos
            .iter()
            .enumerate()
            .filter(|(_, fi)| {
                media::is_media(
                    &fi.relative_filename.to_string_lossy(),
                    &daemon.config.media.extensions,
                )
            })
            .map(|(i, _)| i as u32)
            .collect();
        let subs: Vec<u32> = m
            .file_infos
            .iter()
            .enumerate()
            .filter(|(_, fi)| media::is_subtitle(&fi.relative_filename.to_string_lossy()))
            .map(|(i, _)| i as u32)
            .collect();
        (media, subs, m.torrent_bytes.clone())
    });
    let (media, subs, torrent_bytes) = match classified {
        Ok(t) => t,
        Err(e) => {
            warn!(hash = %short(info_hash), error = %e, "metadata not resolved");
            return None;
        }
    };
    // This copy is what reconcile re-adds from, and a magnet add has none until
    // its metadata resolves here.
    let saved = ensure_metainfo_copy(daemon, info_hash, &torrent_bytes);
    let unknown = !daemon.metadata.read().contains(info_hash);
    if unknown {
        record_unknown_torrent(daemon, info_hash, saved);
    }
    let defaults = FileMetadata {
        // A torrent the engine holds but magneto has no record of comes from an
        // older store or a bad read. It is kept, never treated as spare.
        persisted: unknown || daemon.config.downloads.persist_by_default,
        shared: daemon.config.downloads.share_by_default,
        paused: false,
    };
    let mapped = {
        let mut meta = daemon.metadata.write();
        match meta.get_mut(info_hash) {
            Some(entry) if entry.files.is_empty() && !media.is_empty() => {
                entry.files = media.iter().map(|idx| (*idx, defaults)).collect();
                true
            }
            _ => false,
        }
    };
    if unknown || mapped {
        save_metadata(daemon);
    }
    Some(subs)
}

/// Keep magneto's own copy of the metainfo, writing it if it is missing or was
/// left empty by a torn write.
fn ensure_metainfo_copy(daemon: &Daemon, info_hash: &str, torrent_bytes: &[u8]) -> Option<PathBuf> {
    let path = crate::metadata::torrent_file_path(&daemon.data_dir, info_hash);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 0) {
        return Some(path);
    }
    match crate::metadata::save_torrent_bytes(&daemon.data_dir, info_hash, torrent_bytes) {
        Ok(path) => Some(path),
        Err(e) => {
            warn!(hash = %short(info_hash), error = %e, "saving the torrent file failed");
            None
        }
    }
}

fn record_unknown_torrent(daemon: &Daemon, info_hash: &str, saved: Option<PathBuf>) {
    daemon.metadata.write().insert(
        info_hash.to_string(),
        TorrentMetadata {
            source: saved.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            source_kind: SourceKind::File,
            added_at: chrono::Utc::now(),
            files: BTreeMap::new(),
            finalized: false,
        },
    );
}

/// Hand the engine the selection an added torrent should work on, then mark the
/// record applied. A restored torrent keeps whatever the engine already has: it
/// came back with nothing selected, or the user selected something before the
/// check ran, and either way that intent is not ours to overwrite.
async fn apply_engine_policy(
    daemon: &mut Daemon,
    info_hash: &str,
    media: &[u32],
    subs: &[u32],
    from: Finalize,
) {
    if from == Finalize::Add {
        let select: HashSet<usize> = if daemon.config.downloads.auto_download {
            media.iter().chain(subs.iter()).map(|i| *i as usize).collect()
        } else {
            HashSet::new()
        };
        if let Err(e) = daemon.session.update_only_files(info_hash, &select).await {
            // Leaving the record unapplied keeps it off the cleanup path and
            // makes the next finalize try again.
            warn!(hash = %short(info_hash), error = %e, "selection failed during finalize");
            return;
        }
        if !select.is_empty()
            && let Err(e) = daemon.session.unpause_if_paused(info_hash).await
        {
            warn!(hash = %short(info_hash), error = %e, "unpause failed during finalize");
        }
    }
    {
        let mut meta = daemon.metadata.write();
        if let Some(entry) = meta.get_mut(info_hash) {
            entry.finalized = true;
        }
    }
    save_metadata(daemon);

    if from == Finalize::Add
        && daemon.config.downloads.autoplay
        && !daemon.config.player.command.trim().is_empty()
        && let Some(handle) = daemon.session.get(info_hash)
    {
        let items = build_play_items(&handle, media, &daemon.started);
        let uris: Vec<String> = items.into_iter().map(|i| i.uri).collect();
        if let Err(e) = player::launch_player(&daemon.config.player, &uris) {
            daemon.broadcast(Outbound::PlayerLaunchFailed(PlayerLaunchFailedEvent {
                info_hash: Some(info_hash.to_string()),
                kind: PlayerLaunchKind::Autoplay,
                error: e.to_string(),
            }));
        }
    }
}

/// A torrent with no media files is not magneto's to keep: hand its source to
/// the fallback app and drop it, deleting only bytes magneto wrote.
async fn drop_without_media(daemon: &mut Daemon, info_hash: &str) -> FinalizeOutcome {
    let source = daemon
        .metadata
        .read()
        .get(info_hash)
        .map(|m| m.source.clone())
        .unwrap_or_default();
    let (launched, reason) = try_launch_fallback(&daemon.config, &source).await;
    if !launched && matches!(reason, Some(FallbackReason::SpawnFailed)) {
        daemon.broadcast(Outbound::PlayerLaunchFailed(PlayerLaunchFailedEvent {
            info_hash: Some(info_hash.to_string()),
            kind: PlayerLaunchKind::Fallback,
            error: format!("fallback spawn failed for {}", daemon.config.downloads.fallback_app),
        }));
    }
    if let Err(e) = removal::remove(
        daemon,
        info_hash,
        RemovalReason::NoMedia,
        removal::Files::Managed,
        launched,
    )
    .await
    {
        warn!(hash = %short(info_hash), error = %e, "removal after no media failed");
    }
    FinalizeOutcome {
        state: None,
        files: None,
        media: Some(false),
        fallback_launched: launched,
        fallback_reason: reason,
    }
}

/// Re-check a torrent the engine put in the error state, once per daemon run. A
/// failed check sticks: the state stays errored across restarts even when every
/// byte is on disk, and unpausing is what makes the engine drop the fastresume
/// bitfield and check again.
pub(crate) async fn recover_errored(daemon: &mut Daemon, info_hash: &str, from: Finalize) {
    let handle = daemon.session.get(info_hash);
    let error = handle.as_ref().and_then(|h| h.stats().error);
    if !daemon.rechecked.insert(info_hash.to_string()) {
        if let Some(error) = error {
            daemon.broadcast(Outbound::TorrentError { info_hash: info_hash.to_string(), error });
        }
        return;
    }
    let repause = keep_paused(from, handle.is_some_and(|h| h.is_paused()));
    warn!(hash = %short(info_hash), error = ?error, repause, "torrent errored; re-checking files");
    if let Err(e) = daemon.session.unpause(info_hash).await {
        daemon.broadcast(Outbound::TorrentError {
            info_hash: info_hash.to_string(),
            error: format!("{e:#}"),
        });
        return;
    }
    check::spawn(daemon, info_hash, from, repause);
}

/// Whether a recovered torrent goes back to paused once its check finishes.
/// Every add starts paused so its selection lands first, so an add's pause flag
/// says nothing about intent. Only a restore carries intent worth keeping.
fn keep_paused(from: Finalize, engine_paused: bool) -> bool {
    from == Finalize::Restore && engine_paused
}

fn save_metadata(daemon: &Daemon) {
    if let Err(e) = daemon.metadata.read().save(&daemon.metadata_path) {
        error!(error = %e, "failed to save metadata");
    }
}

// ---- reconcile ----

/// Line the engine up with magneto's record at startup. The record decides
/// which torrents exist: the engine's own database is rewritten without fsync,
/// so a torrent missing from it is re-added from magneto's saved torrent file
/// rather than forgotten.
pub async fn reconcile(daemon: &mut Daemon) {
    let in_session: HashSet<String> = daemon.session.list_infohashes().into_iter().collect();
    let recorded: Vec<String> = daemon.metadata.read().torrents.keys().cloned().collect();

    for hash in recorded {
        if in_session.contains(&hash) {
            continue;
        }
        match restore_torrent(daemon, &hash).await {
            Ok(()) => info!(hash = %short(&hash), "re-added torrent missing from the engine"),
            Err(e) => {
                // Nothing left to re-add from, so the record goes. The data on
                // disk stays: it is not ours to delete without a record saying
                // so.
                warn!(hash = %short(&hash), error = %e, "forgetting torrent with no usable torrent file");
                daemon.metadata.write().remove(&hash);
                crate::metadata::delete_torrent_bytes(&daemon.data_dir, &hash);
            }
        }
    }

    // An entry with no file map never finished its add, so the engine side of
    // it was never applied.
    {
        let mut meta = daemon.metadata.write();
        for entry in meta.torrents.values_mut() {
            if entry.files.is_empty() {
                entry.finalized = false;
            }
        }
    }
    let hashes: Vec<String> = daemon.session.list_infohashes();
    for hash in hashes {
        finalize_torrent(daemon, &hash, Finalize::Restore).await;
    }
    save_metadata(daemon);
    info!("reconciliation complete");
}

/// Put a recorded torrent back into the engine from magneto's own saved torrent
/// file, paused and with nothing selected.
async fn restore_torrent(daemon: &mut Daemon, info_hash: &str) -> anyhow::Result<()> {
    let path = crate::metadata::torrent_file_path(&daemon.data_dir, info_hash);
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let outcome = daemon.session.add_bytes(bytes).await?;
    if outcome.info_hash != info_hash {
        anyhow::bail!("saved torrent file holds {} instead", short(&outcome.info_hash));
    }
    if let Some(entry) = daemon.metadata.write().get_mut(info_hash) {
        entry.finalized = false;
    }
    Ok(())
}

/// Drop what the user did not ask to keep. Deleting needs a record that says so
/// (see `removal::disposable`), so a torrent whose add never finished, or whose
/// record was rebuilt after a bad read, is left alone.
pub async fn cleanup_unpersisted(daemon: &mut Daemon) {
    let recorded: Vec<String> = daemon.metadata.read().torrents.keys().cloned().collect();
    for hash in recorded {
        let Some(entry) = daemon.metadata.read().get(&hash).cloned() else { continue };
        if !entry.finalized {
            info!(hash = %short(&hash), outcome = "kept: add not finished", "cleanup");
            continue;
        }
        if removal::disposable(&entry) {
            if let Err(e) =
                removal::remove(daemon, &hash, RemovalReason::Cleanup, removal::Files::Managed, false)
                    .await
            {
                warn!(hash = %short(&hash), error = %e, "cleanup: removal failed");
            }
            continue;
        }
        let keep_media: Vec<u32> =
            entry.files.iter().filter(|(_, f)| f.persisted).map(|(idx, _)| *idx).collect();
        let dropped: Vec<u32> =
            entry.files.iter().filter(|(_, f)| !f.persisted).map(|(idx, _)| *idx).collect();
        if dropped.is_empty() {
            continue;
        }
        let Some(handle) = daemon.session.get(&hash) else { continue };
        let mut keep: HashSet<usize> = keep_media.iter().map(|i| *i as usize).collect();
        keep.extend(subtitle_indices(&handle).iter().map(|i| *i as usize));
        if let Err(e) = daemon.session.update_only_files(&hash, &keep).await {
            // Deleting bytes the engine still selects would re-download them
            // later, so leave this torrent whole.
            warn!(hash = %short(&hash), error = %e, "cleanup: deselect failed; skipping");
            continue;
        }
        removal::reclaim(&handle, &dropped, &daemon.started.downloads.dir);
        {
            let mut meta = daemon.metadata.write();
            if let Some(entry) = meta.get_mut(&hash) {
                entry.files.retain(|idx, _| keep_media.contains(idx));
            }
        }
        info!(hash = %short(&hash), dropped = dropped.len(), "cleanup: reclaimed files");
    }
    save_metadata(daemon);
}

// ---- mutation handlers ----

async fn handle_remove_torrent(
    daemon: &mut Daemon,
    id: String,
    req: RemoveTorrentReq,
) -> Outbound {
    let files = if req.delete_files { removal::Files::All } else { removal::Files::Keep };
    match removal::remove(daemon, &req.info_hash, RemovalReason::User, files, false).await {
        Ok(()) => Outbound::response(id, OkResp::TRUE),
        Err(e) => Outbound::error(id, format!("remove failed: {e}")),
    }
}

async fn handle_fallback(daemon: &mut Daemon, id: String, req: FallbackReq) -> Outbound {
    let source = daemon
        .metadata
        .read()
        .get(&req.info_hash)
        .map(|m| m.source.clone());
    let Some(source) = source else {
        return Outbound::error(id, format!("no torrent with info_hash {}", req.info_hash));
    };
    let (launched, reason) = try_launch_fallback(&daemon.config, &source).await;
    if !launched && matches!(reason, Some(FallbackReason::SpawnFailed)) {
        daemon
            .broadcast(Outbound::PlayerLaunchFailed(PlayerLaunchFailedEvent {
                info_hash: Some(req.info_hash.clone()),
                kind: PlayerLaunchKind::Fallback,
                error: format!(
                    "fallback spawn failed for {}",
                    daemon.config.downloads.fallback_app
                ),
            }));
    }
    if !launched {
        return Outbound::response(
            id,
            FallbackResp { launched: false, removed: false, reason },
        );
    }
    if let Err(e) = removal::remove(
        daemon,
        &req.info_hash,
        RemovalReason::Fallback,
        removal::Files::Managed,
        true,
    )
    .await
    {
        warn!(hash = %short(&req.info_hash), error = %e, "removal after fallback failed");
    }
    Outbound::response(
        id,
        FallbackResp { launched: true, removed: true, reason: None },
    )
}

fn split_targets(targets: Vec<Target>) -> (Vec<String>, Vec<Target>) {
    let mut torrents = Vec::new();
    let mut rest = Vec::new();
    for t in targets {
        match t {
            Target::Torrent { info_hash } => torrents.push(info_hash),
            other => rest.push(other),
        }
    }
    (torrents, rest)
}

async fn handle_pause(daemon: &mut Daemon, id: String, req: TargetsReq) -> Outbound {
    let (torrent_hashes, file_folder) = split_targets(req.targets);
    let mut affected = 0u32;
    for hash in &torrent_hashes {
        if let Err(e) = daemon.session.pause(hash).await {
            warn!(hash = %short(hash), error = %e, "pause failed");
            continue;
        }
        // affected counts media files uniformly across handlers, so a torrent
        // target reports its file count.
        affected += media_indices(&daemon.metadata, hash).len() as u32;
    }
    if !file_folder.is_empty() {
        let grouped = match group_targets(daemon, &file_folder) {
            Ok(g) => g,
            Err(e) => return Outbound::error(id, e),
        };
        for (info_hash, expanded) in grouped {
            let Some(handle) = daemon.session.get(&info_hash) else { continue };
            let media = media_indices(&daemon.metadata, &info_hash);
            let subs = subtitle_indices(&handle);
            let current = current_selection(&handle);
            // Pause = deselect and KEEP the bytes on disk. drop_targets is the
            // only path that deletes data.
            let (next, _freed) = compute_deselect(&media, &subs, &current, &expanded);
            if let Err(e) = daemon.session.update_only_files(&info_hash, &next).await {
                warn!(hash = %short(&info_hash), error = %e, "pause update_only_files failed");
                continue;
            }
            // Record the pause intent so the file reads as Paused rather than
            // Idle while it sits deselected with its bytes on disk.
            set_paused_flag(&daemon.metadata, &daemon.metadata_path, &info_hash, &expanded, true);
            affected += expanded.len() as u32;
        }
    }
    Outbound::response(id, AffectedResp { affected })
}

/// Set or clear the per-file pause intent for `indices`, then persist. A
/// missing torrent or file entry is skipped silently. The only effect is on
/// the displayed Paused-vs-Idle state, never on download behavior.
fn set_paused_flag(
    metadata: &RwLock<MetadataStore>,
    metadata_path: &Path,
    info_hash: &str,
    indices: &[u32],
    paused: bool,
) {
    let mut meta = metadata.write();
    if let Some(entry) = meta.get_mut(info_hash) {
        for idx in indices {
            if let Some(f) = entry.files.get_mut(idx) {
                f.paused = paused;
            }
        }
    }
    let _ = meta.save(metadata_path);
}

/// Next `only_files` after removing `expanded` from a torrent's current
/// selection, plus the indices actually freed. When removing the targets leaves
/// no media selected, the torrent's subtitles are dropped with them. Shared by
/// pause (deselect, keep data) and drop_targets (deselect + reclaim).
fn compute_deselect(
    media: &[u32],
    subs: &[u32],
    current: &HashSet<usize>,
    expanded: &[u32],
) -> (HashSet<usize>, Vec<u32>) {
    let removed: HashSet<usize> = expanded.iter().map(|i| *i as usize).collect();
    let mut next: HashSet<usize> = current.difference(&removed).copied().collect();
    let any_media_left = media.iter().any(|i| next.contains(&(*i as usize)));
    let mut freed = expanded.to_vec();
    if !any_media_left {
        for s in subs {
            next.remove(&(*s as usize));
        }
        freed.extend(subs.iter().copied());
    }
    (next, freed)
}

async fn handle_drop_targets(daemon: &mut Daemon, id: String, req: TargetsReq) -> Outbound {
    let (torrent_hashes, file_folder) = split_targets(req.targets);
    if !torrent_hashes.is_empty() {
        return Outbound::error(
            id,
            "drop_targets handles files and folders; use remove_torrent for a torrent".to_string(),
        );
    }
    let grouped = match group_targets(daemon, &file_folder) {
        Ok(g) => g,
        Err(e) => return Outbound::error(id, e),
    };
    let mut affected = 0u32;
    for (info_hash, expanded) in grouped {
        let Some(handle) = daemon.session.get(&info_hash) else { continue };
        let media = media_indices(&daemon.metadata, &info_hash);
        let subs = subtitle_indices(&handle);
        let current = current_selection(&handle);
        let (next, freed) = compute_deselect(&media, &subs, &current, &expanded);
        if let Err(e) = daemon.session.update_only_files(&info_hash, &next).await {
            warn!(hash = %short(&info_hash), error = %e, "drop update_only_files failed");
            continue;
        }
        // Deselected above; now free the bytes, the only difference from pause.
        removal::reclaim(&handle, &freed, &daemon.started.downloads.dir);
        let now_empty = {
            let mut meta = daemon.metadata.write();
            if let Some(entry) = meta.get_mut(&info_hash) {
                entry.files.retain(|idx, _| !freed.contains(idx));
            }
            meta.get(&info_hash).map(|e| e.files.is_empty()).unwrap_or(true)
        };
        save_metadata(daemon);
        affected += expanded.len() as u32;
        if now_empty {
            // Dropping the last media file empties the torrent, so it goes
            // rather than lingering as a husk. The bytes magneto wrote for the
            // rest of it go with it.
            if let Err(e) = removal::remove(
                daemon,
                &info_hash,
                RemovalReason::User,
                removal::Files::Managed,
                false,
            )
            .await
            {
                warn!(hash = %short(&info_hash), error = %e, "drop-empty removal failed");
            }
        }
    }
    Outbound::response(id, AffectedResp { affected })
}

async fn handle_resume(daemon: &mut Daemon, id: String, req: TargetsReq) -> Outbound {
    // Resume is select-then-unpause, nothing more: update_only_files on a
    // live torrent already re-queues new pieces and reconnects parked peers.
    let grouped = match resume_groups(daemon, req.targets) {
        Ok(g) => g,
        Err(e) => return Outbound::error(id, e),
    };
    let mut affected = 0u32;
    let mut initializing = 0u32;
    for (info_hash, expanded) in grouped {
        let Some(handle) = daemon.session.get(&info_hash) else { continue };
        let engine_state = handle.stats().state;
        if matches!(engine_state, TorrentStatsState::Initializing { .. }) {
            // The engine rejects selection changes mid-check, and starting it
            // here would race the in-flight init task. Report instead of
            // silently doing nothing.
            initializing += 1;
            continue;
        }
        if let Err(e) = select_for_resume(
            &daemon.session,
            &daemon.metadata,
            &daemon.metadata_path,
            &info_hash,
            &expanded,
        )
        .await
        {
            warn!(hash = %short(&info_hash), error = %e, "resume selection failed");
            continue;
        }
        let ensure_live = if matches!(engine_state, TorrentStatsState::Error) {
            // Errors are recoverable: a fresh start re-checks the files and
            // honors the selection recorded above.
            daemon.rechecked.insert(info_hash.clone());
            daemon.session.unpause(&info_hash).await
        } else {
            daemon.session.unpause_if_paused(&info_hash).await
        };
        if let Err(e) = ensure_live {
            warn!(hash = %short(&info_hash), error = %e, "resume unpause failed");
            continue;
        }
        if matches!(engine_state, TorrentStatsState::Error) {
            // The re-check runs now; finalize once it lands.
            check::spawn(daemon, &info_hash, Finalize::Restore, false);
        }
        affected += expanded.len() as u32;
    }
    if affected == 0 && initializing > 0 {
        return Outbound::error(id, "torrent is still checking files; try again shortly");
    }
    Outbound::response(id, AffectedResp { affected })
}

/// Resume target expansion. A torrent resumes its current selection
/// ("continue what was wanted") and falls back to all media only when that
/// selection has nothing left to do (empty, or every selected file already
/// complete). Files and folders expand to media indices as usual.
fn resume_groups(
    daemon: &Daemon,
    targets: Vec<Target>,
) -> Result<Vec<(String, Vec<u32>)>, String> {
    let had_targets = !targets.is_empty();
    let (torrent_hashes, file_folder) = split_targets(targets);
    let mut by_hash: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for hash in torrent_hashes {
        let media = media_indices(&daemon.metadata, &hash);
        if media.is_empty() {
            continue;
        }
        let Some(handle) = daemon.session.get(&hash) else { continue };
        let current = current_selection(&handle);
        let selected: Vec<u32> = media
            .iter()
            .copied()
            .filter(|i| current.contains(&(*i as usize)))
            .collect();
        let progress = handle.stats().file_progress;
        let sizes: Vec<u64> = handle
            .with_metadata(|m| m.file_infos.iter().map(|fi| fi.len).collect())
            .unwrap_or_default();
        let pending = selected.iter().any(|i| {
            let i = *i as usize;
            progress.get(i).copied().unwrap_or(0) < sizes.get(i).copied().unwrap_or(0)
        });
        let expanded = if selected.is_empty() || !pending { media } else { selected };
        by_hash.entry(hash).or_default().extend(expanded);
    }
    if !file_folder.is_empty() {
        match group_targets(daemon, &file_folder) {
            Ok(groups) => {
                for (hash, expanded) in groups {
                    by_hash.entry(hash).or_default().extend(expanded);
                }
            }
            // Unresolvable file/folder targets only fail the request when no
            // torrent target resolved either.
            Err(e) if by_hash.is_empty() => return Err(e),
            Err(_) => {}
        }
    }
    if by_hash.is_empty() && had_targets {
        return Err("targets do not resolve to any media file".into());
    }
    Ok(by_hash.into_iter().collect())
}

/// Make `targets` (+ subtitles) selected for download and clear their pause
/// flags. When the torrent is engine-paused and other media are selected,
/// those convert to per-file pauses instead, so only the targets resume once
/// the torrent goes live. Shared by resume and the stream endpoint: playing a
/// file starts exactly that file.
pub(crate) async fn select_for_resume(
    session: &SessionHandle,
    metadata: &RwLock<MetadataStore>,
    metadata_path: &Path,
    info_hash: &str,
    targets: &[u32],
) -> anyhow::Result<()> {
    let handle = session
        .get(info_hash)
        .with_context(|| format!("no torrent with info_hash {info_hash}"))?;
    let media = media_indices(metadata, info_hash);
    let subs = subtitle_indices(&handle);
    let current = current_selection(&handle);
    let stats = handle.stats();
    let added: HashSet<usize> = targets.iter().map(|i| *i as usize).collect();
    let rest_paused: Vec<u32> = media
        .iter()
        .copied()
        .filter(|i| current.contains(&(*i as usize)) && !targets.contains(i))
        // A file already on disk costs nothing to leave selected, and pausing
        // it would only make its row lie.
        .filter(|i| !file_complete(&handle, &stats.file_progress, *i))
        .collect();
    let mut next: HashSet<usize> =
        if session::engine_paused(&stats.state) && !rest_paused.is_empty() {
            set_paused_flag(metadata, metadata_path, info_hash, &rest_paused, true);
            added
        } else {
            current.union(&added).copied().collect()
        };
    next.extend(subs.iter().map(|i| *i as usize));
    session.update_only_files(info_hash, &next).await?;
    set_paused_flag(metadata, metadata_path, info_hash, targets, false);
    Ok(())
}

async fn handle_set_persist(
    daemon: &mut Daemon,
    id: String,
    req: SetPersistReq,
) -> Outbound {
    let grouped = match group_targets(daemon, &req.targets) {
        Ok(g) => g,
        Err(e) => return Outbound::error(id, e),
    };
    let mut affected = 0u32;
    {
        let mut meta = daemon.metadata.write();
        for (info_hash, indices) in &grouped {
            let Some(entry) = meta.get_mut(info_hash) else { continue };
            for idx in indices {
                if let Some(f) = entry.files.get_mut(idx) {
                    f.persisted = req.persisted;
                    affected += 1;
                }
            }
        }
        let _ = meta.save(&daemon.metadata_path);
    }
    Outbound::response(id, AffectedResp { affected })
}

async fn handle_set_shared(daemon: &mut Daemon, id: String, req: SetSharedReq) -> Outbound {
    let grouped = match group_targets(daemon, &req.targets) {
        Ok(g) => g,
        Err(e) => return Outbound::error(id, e),
    };
    let mut affected = 0u32;
    {
        let mut meta = daemon.metadata.write();
        for (info_hash, indices) in &grouped {
            let Some(entry) = meta.get_mut(info_hash) else { continue };
            for idx in indices {
                if let Some(f) = entry.files.get_mut(idx) {
                    f.shared = req.shared;
                    affected += 1;
                }
            }
        }
        let _ = meta.save(&daemon.metadata_path);
    }
    Outbound::response(id, AffectedResp { affected })
}

async fn handle_play(daemon: &mut Daemon, id: String, req: TargetsReq) -> Outbound {
    if daemon.config.player.command.trim().is_empty() {
        return Outbound::error(id, "player command is not configured");
    }
    let grouped = match group_targets(daemon, &req.targets) {
        Ok(g) => g,
        Err(e) => return Outbound::error(id, e),
    };
    let mut items: Vec<PlayItem> = Vec::new();
    for (info_hash, indices) in &grouped {
        let Some(handle) = daemon.session.get(info_hash) else { continue };
        items.extend(build_play_items(&handle, indices, &daemon.started));
    }
    if items.is_empty() {
        return Outbound::error(id, "no playable files in targets");
    }
    let uris: Vec<String> = items.iter().map(|i| i.uri.clone()).collect();
    if let Err(e) = player::launch_player(&daemon.config.player, &uris) {
        daemon
            .broadcast(Outbound::PlayerLaunchFailed(PlayerLaunchFailedEvent {
                info_hash: None,
                kind: PlayerLaunchKind::Play,
                error: e.to_string(),
            }));
        return Outbound::error(id, format!("player launch failed: {e}"));
    }
    Outbound::response(id, PlayResp { items })
}

async fn handle_resolve_local_path(
    daemon: &mut Daemon,
    id: String,
    req: ResolveLocalPathReq,
) -> Outbound {
    let info_hash = req.target.info_hash().to_string();
    let Some(handle) = daemon.session.get(&info_hash) else {
        return Outbound::error(id, format!("no torrent with info_hash {info_hash}"));
    };
    let (path, kind) = match req.target {
        Target::Torrent { .. } => {
            (torrent_local_root(&handle, &daemon.started.downloads.dir), PathKind::Folder)
        }
        Target::Folder { path, .. } => {
            let normalized = path.trim_matches('/').replace('\\', "/");
            let candidate = std::path::Path::new(&normalized);
            if candidate.is_absolute()
                || candidate
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Outbound::error(id, "path must be relative without parent components");
            }
            let root = torrent_local_root(&handle, &daemon.started.downloads.dir);
            let p = if normalized.is_empty() { root } else { root.join(&normalized) };
            (p, PathKind::Folder)
        }
        Target::File { file_index, .. } => match file_local_path(&handle, file_index as usize, &daemon.started.downloads.dir) {
            Some(p) => (p, PathKind::File),
            None => return Outbound::error(id, "could not resolve file path"),
        },
    };
    let exists = path.exists();
    Outbound::response(
        id,
        ResolveLocalPathResp { path: path.to_string_lossy().into_owned(), kind, exists },
    )
}

async fn handle_set_config(
    daemon: &mut Daemon,
    id: String,
    payload: serde_json::Value,
) -> Outbound {
    let diff = match daemon.config.diff(&payload) {
        Ok(d) => d,
        Err(e) => return Outbound::error(id, format!("invalid config: {e}")),
    };
    if let Err(e) = preflight_set_config(daemon, &diff.merged) {
        return Outbound::error(id, format!("config rejected: {e}"));
    }
    if let Err(e) = diff.merged.save(&daemon.config_path) {
        return Outbound::error(id, format!("config save failed: {e}"));
    }
    daemon.config = diff.merged.clone();
    // Feed the stats task the hot-applied config so its deltas stop using the
    // clone captured at spawn.
    let _ = daemon.config_tx.send(daemon.config.clone());
    let pending = magneto_core::config::pending_restart(&daemon.started, &daemon.config);
    let restart_required = !pending.is_empty();
    let event = ConfigChangedEvent {
        config: diff.merged.clone(),
        restart_required,
        pending_restart: pending.clone(),
    };
    daemon.broadcast(Outbound::ConfigChanged(event));
    Outbound::response(
        id,
        SetConfigResp { config: diff.merged, restart_required, pending_restart: pending },
    )
}

/// Probe a restart-required field only when the patch changes it AND it differs
/// from the running process, so re-saving an in-effect value is never rejected.
fn preflight_set_config(daemon: &Daemon, merged: &Config) -> anyhow::Result<()> {
    let pre = &daemon.config;
    let running = &daemon.started;
    if merged.network.control_port != pre.network.control_port
        && merged.network.control_port != running.network.control_port
    {
        preflight::probe_bind([127, 0, 0, 1], merged.network.control_port)
            .with_context(|| format!("control_port {} unavailable", merged.network.control_port))?;
    }
    if merged.network.upnp_enabled
        && merged.network.lan_port != pre.network.lan_port
        && merged.network.lan_port != running.network.lan_port
    {
        preflight::probe_bind([0, 0, 0, 0], merged.network.lan_port)
            .with_context(|| format!("lan_port {} unavailable", merged.network.lan_port))?;
    }
    if merged.downloads.dir != pre.downloads.dir && merged.downloads.dir != running.downloads.dir {
        preflight::probe_dir(&merged.downloads.dir).with_context(|| {
            format!("downloads dir {} not writable", merged.downloads.dir.display())
        })?;
    }
    Ok(())
}

async fn handle_restart(daemon: &mut Daemon, id: String) -> Outbound {
    daemon.request_shutdown(crate::daemon::ShutdownKind::Restart);
    Outbound::response(id, OkResp::TRUE)
}

async fn handle_shutdown(daemon: &mut Daemon, id: String) -> Outbound {
    daemon.request_shutdown(crate::daemon::ShutdownKind::Stop);
    Outbound::response(id, OkResp::TRUE)
}

// ---- target expansion helpers ----

fn group_targets(
    daemon: &Daemon,
    targets: &[Target],
) -> Result<Vec<(String, Vec<u32>)>, String> {
    let mut pairs: Vec<(String, Vec<u32>)> = Vec::new();
    for target in targets {
        let info_hash = target.info_hash().to_string();
        let media = media_indices(&daemon.metadata, &info_hash);
        if media.is_empty() {
            continue;
        }
        let expanded = match target {
            Target::Torrent { .. } => media.clone(),
            Target::File { file_index, .. } => {
                if media.contains(file_index) {
                    vec![*file_index]
                } else {
                    continue;
                }
            }
            Target::Folder { path, .. } => {
                let Some(handle) = daemon.session.get(&info_hash) else { continue };
                let paths: Vec<(u32, std::path::PathBuf)> = handle
                    .with_metadata(|m| {
                        media
                            .iter()
                            .filter_map(|idx| {
                                m.file_infos
                                    .get(*idx as usize)
                                    .map(|fi| (*idx, fi.relative_filename.clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let prefix = path.trim_matches('/').to_string();
                paths
                    .into_iter()
                    .filter(|(_, p)| {
                        let s = p.to_string_lossy().replace('\\', "/");
                        s.starts_with(&format!("{prefix}/"))
                    })
                    .map(|(idx, _)| idx)
                    .collect()
            }
        };
        // A target that resolves to nothing (e.g. a folder prefix matching no
        // media) must not create an empty group. That would suppress the
        // "does not resolve" error below and report a silent success.
        if !expanded.is_empty() {
            pairs.push((info_hash, expanded));
        }
    }
    let groups = merge_target_groups(pairs);
    if groups.is_empty() && !targets.is_empty() {
        return Err("targets do not resolve to any media file".into());
    }
    Ok(groups)
}

/// Collapse per-target (info_hash, indices) pairs into one group per torrent.
/// Torrents keep first-seen order across the list, and indices keep first-seen
/// order within each torrent, so the resulting sequence follows the order the
/// targets arrived in. A repeated index (a whole torrent plus one of its files)
/// collapses to its first occurrence.
fn merge_target_groups(pairs: Vec<(String, Vec<u32>)>) -> Vec<(String, Vec<u32>)> {
    let mut groups: Vec<(String, Vec<u32>)> = Vec::new();
    let mut slot: HashMap<String, usize> = HashMap::new();
    for (info_hash, indices) in pairs {
        let i = match slot.get(&info_hash) {
            Some(&i) => i,
            None => {
                let i = groups.len();
                slot.insert(info_hash.clone(), i);
                groups.push((info_hash, Vec::new()));
                i
            }
        };
        let bucket = &mut groups[i].1;
        for idx in indices {
            if !bucket.contains(&idx) {
                bucket.push(idx);
            }
        }
    }
    groups
}

fn media_indices(metadata: &RwLock<MetadataStore>, info_hash: &str) -> Vec<u32> {
    metadata
        .read()
        .get(info_hash)
        .map(|e| e.files.keys().copied().collect())
        .unwrap_or_default()
}

/// The torrent's current selection as a set of file indices. librqbit reports
/// `None` for "all files selected", a state magneto's own adds never produce,
/// but a foreign or legacy session dir can. Expanding it here keeps the
/// mutation handlers consistent with rendering (`file_is_selected`), so a
/// None-armed torrent can never be mass-deselected by a single-file operation.
pub(crate) fn current_selection(handle: &TorrentHandle) -> HashSet<usize> {
    match handle.only_files() {
        Some(v) => v.into_iter().collect(),
        None => {
            let count = handle.with_metadata(|m| m.file_infos.len()).unwrap_or(0);
            (0..count).collect()
        }
    }
}

pub(crate) fn subtitle_indices(handle: &TorrentHandle) -> Vec<u32> {
    handle
        .with_metadata(|m| {
            m.file_infos
                .iter()
                .enumerate()
                .filter(|(_, fi)| media::is_subtitle(&fi.relative_filename.to_string_lossy()))
                .map(|(i, _)| i as u32)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the engine reports every byte of this file present. While a torrent
/// checks files or sits in error the engine reports no per-file progress at all,
/// so this reads false.
fn file_complete(handle: &TorrentHandle, progress: &[u64], index: u32) -> bool {
    let downloaded = progress.get(index as usize).copied().unwrap_or(0);
    let len = handle
        .with_metadata(|m| m.file_infos.get(index as usize).map(|fi| fi.len))
        .ok()
        .flatten()
        .unwrap_or(0);
    len > 0 && downloaded >= len
}

// ---- helpers ----

async fn persist_source(
    daemon: &mut Daemon,
    info_hash: &str,
    raw_source: &str,
    kind: SourceKind,
) -> anyhow::Result<String> {
    let (source, source_kind) = match kind {
        SourceKind::Magnet | SourceKind::Url => (raw_source.to_string(), kind),
        SourceKind::File => {
            let bytes = base64::engine::general_purpose::STANDARD.decode(raw_source)?;
            let path = crate::metadata::save_torrent_bytes(&daemon.data_dir, info_hash, &bytes)?;
            (path.to_string_lossy().into_owned(), SourceKind::File)
        }
    };
    let entry = TorrentMetadata {
        source: source.clone(),
        source_kind,
        added_at: chrono::Utc::now(),
        files: BTreeMap::new(),
        // The file map arrives when the check finishes. Until then nothing may
        // read this entry as "the user kept nothing".
        finalized: false,
    };
    daemon.metadata.write().insert(info_hash.to_string(), entry);
    daemon
        .metadata
        .read()
        .save(&daemon.metadata_path)
        .context("recording the torrent")?;
    Ok(source)
}

fn detect_source_kind(s: &str) -> SourceKind {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("magnet:") {
        SourceKind::Magnet
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        SourceKind::Url
    } else {
        SourceKind::File
    }
}

async fn try_launch_fallback(
    config: &Config,
    source: &str,
) -> (bool, Option<FallbackReason>) {
    if config.downloads.fallback_app.trim().is_empty() {
        return (false, Some(FallbackReason::NotConfigured));
    }
    let mut args = config.downloads.fallback_args.clone();
    args.push(source.to_string());
    match player::spawn(&config.downloads.fallback_app, &args) {
        Ok(()) => (true, None),
        Err(_) => (false, Some(FallbackReason::SpawnFailed)),
    }
}

pub fn build_play_items(
    handle: &TorrentHandle,
    media_indices: &[u32],
    config: &Config,
) -> Vec<PlayItem> {
    let stats = handle.stats();
    let file_info: Vec<(u64, String)> = handle
        .with_metadata(|m| {
            m.file_infos
                .iter()
                .enumerate()
                .map(|(i, fi)| {
                    let name = fi
                        .relative_filename
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| format!("file-{i}"));
                    (fi.len, name)
                })
                .collect()
        })
        .unwrap_or_default();
    let info_hash = handle.info_hash().as_string();
    media_indices
        .iter()
        .filter_map(|&idx| {
            let i = idx as usize;
            let (size, filename) = file_info.get(i)?;
            let downloaded = stats.file_progress.get(i).copied().unwrap_or(0);
            let uri = hybrid_uri(handle, i, *size, downloaded, filename, &info_hash, config);
            Some(PlayItem { name: filename.clone(), uri })
        })
        .collect()
}

pub fn hybrid_uri(
    handle: &TorrentHandle,
    file_index: usize,
    size: u64,
    downloaded: u64,
    filename: &str,
    info_hash: &str,
    config: &Config,
) -> String {
    if downloaded >= size
        && let Some(path) = file_local_path(handle, file_index, &config.downloads.dir)
        && path.exists()
    {
        return path.to_string_lossy().into_owned();
    }
    stream_url(info_hash, file_index, filename, config.network.control_port)
}

pub fn stream_url(info_hash: &str, file_index: usize, filename: &str, port: u16) -> String {
    let encoded = urlencoding::encode(filename);
    format!("http://127.0.0.1:{port}/stream/{info_hash}/{file_index}/{encoded}")
}

// ---- rendering ----

// One assembly per torrent: media_file_progress reads the engine once into
// FileProgress; render_file_entries turns those into wire FileEntry rows (the
// `recently_active` set decides Downloading vs Queued: None on a cold snapshot,
// the tick-history set in the stats loop); summarize aggregates the rows. The
// stats tick and the request handlers share this path so the summary and the
// per-file rows can never disagree on a torrent's state.

pub fn render_torrent_detail(
    handle: &TorrentHandle,
    meta: Option<&TorrentMetadata>,
    config: &Config,
) -> TorrentDetail {
    let stats = handle.stats();
    let progress = media_file_progress(handle, &stats, meta, config);
    let files = render_file_entries(stats.state, &progress, meta, config, None);
    let summary = summarize(handle, &stats, meta, &files);
    TorrentDetail { summary, files }
}

pub fn render_torrent_summary(
    handle: &TorrentHandle,
    meta: Option<&TorrentMetadata>,
    config: &Config,
) -> TorrentSummary {
    let stats = handle.stats();
    let progress = media_file_progress(handle, &stats, meta, config);
    let files = render_file_entries(stats.state, &progress, meta, config, None);
    summarize(handle, &stats, meta, &files)
}

/// The torrent's media files as raw progress rows, the shared input to both
/// rendering and the stats tick's active-window tracking.
pub fn media_file_progress(
    handle: &TorrentHandle,
    stats: &TorrentStats,
    meta: Option<&TorrentMetadata>,
    config: &Config,
) -> Vec<FileProgress> {
    let only_files = handle.only_files();
    let Ok(file_data) = handle.with_metadata(|m| {
        m.file_infos
            .iter()
            .map(|fi| (fi.len, path_to_string(&fi.relative_filename)))
            .collect::<Vec<_>>()
    }) else {
        return Vec::new();
    };
    let media_indices: Vec<u32> = if let Some(m) = meta {
        m.files.keys().copied().collect()
    } else {
        file_data
            .iter()
            .enumerate()
            .filter(|(_, (_, name))| media::is_media(name, &config.media.extensions))
            .map(|(i, _)| i as u32)
            .collect()
    };
    media_indices
        .iter()
        .filter_map(|idx| {
            let (size, path) = file_data.get(*idx as usize)?;
            Some(FileProgress {
                index: *idx,
                path: path.clone(),
                selected: file_is_selected(&only_files, *idx as usize),
                downloaded: stats.file_progress.get(*idx as usize).copied().unwrap_or(0),
                size: *size,
            })
        })
        .collect()
}

/// Wire FileEntry rows for the media files. `recently_active` is the set counted
/// as actively downloading (None = cold head-only).
pub fn render_file_entries(
    torrent_state: TorrentStatsState,
    files: &[FileProgress],
    meta: Option<&TorrentMetadata>,
    config: &Config,
    recently_active: Option<&HashSet<u32>>,
) -> Vec<FileEntry> {
    let active = active_indices(files, recently_active);
    files
        .iter()
        .map(|f| {
            let file_meta = meta.and_then(|m| m.files.get(&f.index));
            let persisted =
                file_meta.map(|m| m.persisted).unwrap_or(config.downloads.persist_by_default);
            let shared = file_meta.map(|m| m.shared).unwrap_or(config.downloads.share_by_default);
            let paused = file_meta.map(|m| m.paused).unwrap_or(false);
            FileEntry {
                index: f.index,
                path: f.path.clone(),
                size: f.size,
                downloaded_bytes: f.downloaded,
                selected: f.selected,
                state: per_file_state(
                    torrent_state,
                    f.selected,
                    f.downloaded,
                    f.size,
                    paused,
                    active.contains(&f.index),
                ),
                persisted,
                shared,
            }
        })
        .collect()
}

/// Aggregate the rendered file rows into the torrent summary.
pub fn summarize(
    handle: &TorrentHandle,
    stats: &TorrentStats,
    meta: Option<&TorrentMetadata>,
    files: &[FileEntry],
) -> TorrentSummary {
    let added_at = meta
        .map(|m| m.added_at.to_rfc3339())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let state = aggregate_torrent_state(stats.state, files);
    let total_bytes_all: u64 = files.iter().map(|f| f.size).sum();
    let total_bytes_selected: u64 = files.iter().filter(|f| f.selected).map(|f| f.size).sum();
    let downloaded_bytes: u64 =
        files.iter().filter(|f| f.selected).map(|f| f.downloaded_bytes).sum();
    let live_running = stats.live.is_some();
    // librqbit reports speed in MiB/s (Speed.mbps); the wire contract is bytes
    // per second. Convert at this single boundary.
    let (download_speed, upload_speed) = stats
        .live
        .as_ref()
        .map(|l| (l.download_speed.mbps * 1024.0 * 1024.0, l.upload_speed.mbps * 1024.0 * 1024.0))
        .unwrap_or((0.0, 0.0));
    let complete_count = files.iter().filter(|f| matches!(f.state, FileState::Complete)).count() as u32;
    let selected_count = files.iter().filter(|f| f.selected).count() as u32;
    let persisted_count = files.iter().filter(|f| f.persisted).count() as u32;
    let shared_count = files.iter().filter(|f| f.shared).count() as u32;
    TorrentSummary {
        info_hash: handle.info_hash().as_string(),
        name: handle.name(),
        source: meta.map(|m| m.source.clone()),
        source_kind: meta.map(|m| m.source_kind),
        state,
        total_bytes_all,
        total_bytes_selected,
        downloaded_bytes,
        download_speed,
        upload_speed,
        file_count: files.len() as u32,
        complete_count,
        selected_count,
        persisted_count,
        shared_count,
        is_initializing: matches!(state, TorrentState::Initializing),
        is_complete: matches!(state, TorrentState::Complete),
        is_seeding: matches!(state, TorrentState::Complete) && live_running,
        is_paused: session::engine_paused(&stats.state),
        added_at,
    }
}

pub fn file_is_selected(only_files: &Option<Vec<usize>>, file_index: usize) -> bool {
    match only_files {
        None => true,
        Some(v) => v.contains(&file_index),
    }
}

pub fn per_file_state(
    torrent_state: TorrentStatsState,
    selected: bool,
    downloaded: u64,
    size: u64,
    paused_intent: bool,
    active: bool,
) -> FileState {
    if downloaded >= size {
        return FileState::Complete;
    }
    if matches!(torrent_state, TorrentStatsState::Error) {
        return FileState::Error;
    }
    if !selected {
        // Deselected: Paused if the user paused it (bytes kept on disk),
        // otherwise Idle (never wanted).
        return if paused_intent { FileState::Paused } else { FileState::Idle };
    }
    if matches!(torrent_state, TorrentStatsState::Paused) {
        return FileState::Paused;
    }
    if matches!(torrent_state, TorrentStatsState::Initializing { .. }) {
        // The engine is checking files, so nothing downloads: a selected file
        // is waiting, not active.
        return FileState::Queued;
    }
    // Selected, incomplete, torrent live: Downloading only if the file is
    // actually receiving data; Queued if it's waiting its turn behind an
    // earlier file in the engine's order.
    if active { FileState::Downloading } else { FileState::Queued }
}

/// One media file's progress, the input to [`active_indices`].
pub struct FileProgress {
    pub index: u32,
    pub path: String,
    pub selected: bool,
    pub downloaded: u64,
    pub size: u64,
}

/// The media files currently receiving data. librqbit downloads files one at
/// a time in relative-path order, compared as paths (component by component),
/// not as strings, which differs around separators ("Show - Extras/x" sorts
/// after "Show/y" as a path but before it as a string). So the "head", the
/// path-first selected-incomplete file, is always active (nothing is ahead of
/// it). `recently_active` additionally marks files that gained bytes in the
/// recent past: a file being streamed out of order, or the next file as the
/// head finishes. It is `None` on a cold snapshot (no progress history yet),
/// where only the head is known.
pub fn active_indices(
    files: &[FileProgress],
    recently_active: Option<&HashSet<u32>>,
) -> HashSet<u32> {
    let mut active = HashSet::new();
    let pending = || files.iter().filter(|f| f.selected && f.downloaded < f.size);
    if let Some(head) = pending().min_by(|a, b| Path::new(&a.path).cmp(Path::new(&b.path))) {
        active.insert(head.index);
    }
    if let Some(recent) = recently_active {
        for f in pending() {
            if recent.contains(&f.index) {
                active.insert(f.index);
            }
        }
    }
    active
}

pub fn aggregate_torrent_state(
    torrent_stats_state: TorrentStatsState,
    files: &[FileEntry],
) -> TorrentState {
    match torrent_stats_state {
        TorrentStatsState::Initializing { .. } => return TorrentState::Initializing,
        TorrentStatsState::Error => return TorrentState::Error,
        _ => {}
    }
    if files.is_empty() {
        return TorrentState::Idle;
    }
    // Complete = everything the user asked for is on disk: the whole torrent,
    // or a non-empty selection that is fully downloaded. Without the selection
    // clause the common select-a-few flow would finish into "idle" and never
    // read (or announce) complete.
    let all_complete = files.iter().all(|f| matches!(f.state, FileState::Complete));
    let selection_complete = files.iter().any(|f| f.selected)
        && files
            .iter()
            .filter(|f| f.selected)
            .all(|f| matches!(f.state, FileState::Complete));
    if all_complete || selection_complete {
        return TorrentState::Complete;
    }
    if files.iter().any(|f| matches!(f.state, FileState::Downloading)) {
        return TorrentState::Downloading;
    }
    if files.iter().any(|f| matches!(f.state, FileState::Paused)) {
        return TorrentState::Paused;
    }
    TorrentState::Idle
}

// Reconstructs a torrent's on-disk root. Single-file torrents live directly in
// the downloads dir; multi-file torrents live in a subfolder named after the
// torrent, matching how the torrent engine derives that subfolder for the common
// case. For unnamed or non-UTF-8 torrents the engine's chosen subfolder can
// differ, so a path produced from this root is best-effort: callers must treat a
// non-existent path as "unavailable" rather than authoritative.
pub fn torrent_local_root(handle: &TorrentHandle, download_dir: &Path) -> PathBuf {
    let multi_file = handle
        .with_metadata(|m| m.file_infos.len() >= 2)
        .unwrap_or(false);
    if !multi_file {
        return download_dir.to_path_buf();
    }
    match handle.name() {
        Some(n) if !n.is_empty() => download_dir.join(n),
        _ => download_dir.to_path_buf(),
    }
}

pub fn file_local_path(
    handle: &TorrentHandle,
    file_index: usize,
    download_dir: &Path,
) -> Option<PathBuf> {
    let root = torrent_local_root(handle, download_dir);
    handle
        .with_metadata(|m| {
            m.file_infos
                .get(file_index)
                .map(|fi| root.join(&fi.relative_filename))
        })
        .ok()
        .flatten()
}

async fn with_payload<T, F, Fut>(id: String, payload: serde_json::Value, f: F) -> Outbound
where
    T: DeserializeOwned,
    F: FnOnce(String, T) -> Fut,
    Fut: std::future::Future<Output = Outbound>,
{
    match serde_json::from_value::<T>(payload) {
        Ok(parsed) => f(id, parsed).await,
        Err(e) => Outbound::error(id, format!("invalid payload: {e}")),
    }
}

fn path_to_string(p: &std::path::Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(state: FileState) -> FileEntry {
        file_sel(state, true)
    }

    fn file_sel(state: FileState, selected: bool) -> FileEntry {
        FileEntry {
            index: 0,
            path: "f".into(),
            size: 100,
            downloaded_bytes: 0,
            selected,
            state,
            persisted: false,
            shared: false,
        }
    }

    #[test]
    fn per_file_state_zero_byte_file_is_complete() {
        assert_eq!(per_file_state(TorrentStatsState::Live, true, 0, 0, false, false), FileState::Complete);
    }

    #[test]
    fn per_file_state_error_for_incomplete_file() {
        // Error short-circuits only when the file is incomplete; downloaded < size.
        assert_eq!(per_file_state(TorrentStatsState::Error, true, 50, 100, false, false), FileState::Error);
        assert_eq!(per_file_state(TorrentStatsState::Error, false, 50, 100, false, false), FileState::Error);
    }

    #[test]
    fn per_file_state_complete_wins_over_error() {
        // A file fully on disk stays Complete even if the torrent reports Error.
        // The bytes are real; UI can still play via local path.
        assert_eq!(per_file_state(TorrentStatsState::Error, true, 100, 100, false, false), FileState::Complete);
        assert_eq!(per_file_state(TorrentStatsState::Error, false, 100, 100, false, false), FileState::Complete);
    }

    #[test]
    fn per_file_state_not_selected_is_idle_or_paused() {
        // Deselected with no pause intent = never wanted = Idle.
        assert_eq!(per_file_state(TorrentStatsState::Live, false, 50, 100, false, false), FileState::Idle);
        // Deselected because the user paused it = Paused (bytes kept on disk).
        assert_eq!(per_file_state(TorrentStatsState::Live, false, 50, 100, true, false), FileState::Paused);
    }

    #[test]
    fn per_file_state_complete_wins_over_paused() {
        assert_eq!(per_file_state(TorrentStatsState::Paused, true, 100, 100, false, false), FileState::Complete);
    }

    #[test]
    fn per_file_state_selected_paused_incomplete() {
        // Whole-torrent pause: every selected-incomplete file reads Paused.
        assert_eq!(per_file_state(TorrentStatsState::Paused, true, 50, 100, false, false), FileState::Paused);
    }

    #[test]
    fn per_file_state_active_downloads_inactive_queues() {
        // Selected + incomplete + live splits on the active flag: the file
        // actually receiving data downloads; the rest wait their turn.
        assert_eq!(per_file_state(TorrentStatsState::Live, true, 50, 100, false, true), FileState::Downloading);
        assert_eq!(per_file_state(TorrentStatsState::Live, true, 50, 100, false, false), FileState::Queued);
    }

    #[test]
    fn per_file_state_initializing_selected_is_queued() {
        // Nothing downloads during init, so a selected file waits regardless of
        // the active flag.
        assert_eq!(
            per_file_state(TorrentStatsState::Initializing { paused: false }, true, 0, 100, false, true),
            FileState::Queued
        );
    }

    fn fp(index: u32, path: &str, selected: bool, downloaded: u64, size: u64) -> FileProgress {
        FileProgress { index, path: path.to_string(), selected, downloaded, size }
    }

    #[test]
    fn active_indices_head_only_without_history() {
        // Cold snapshot: only the filename-first selected-incomplete file is active.
        let files = vec![fp(0, "a.mkv", true, 10, 100), fp(1, "b.mkv", true, 0, 100)];
        assert_eq!(active_indices(&files, None), HashSet::from([0]));
    }

    #[test]
    fn active_indices_orders_paths_by_components_not_strings() {
        // "Show - Extras/…" sorts before "Show/…" as a string (' ' < '/') but
        // after it as a path; the head must follow the engine's path order.
        let files = vec![
            fp(0, "Show - Extras/a.mkv", true, 0, 100),
            fp(1, "Show/b.mkv", true, 0, 100),
        ];
        assert_eq!(active_indices(&files, None), HashSet::from([1]));
    }

    #[test]
    fn active_indices_adds_recently_active_out_of_order_file() {
        // A non-head file that recently gained bytes (e.g. being streamed) is
        // active alongside the head.
        let files = vec![fp(0, "a.mkv", true, 10, 100), fp(1, "b.mkv", true, 20, 100)];
        let recent = HashSet::from([1]);
        assert_eq!(active_indices(&files, Some(&recent)), HashSet::from([0, 1]));
    }

    #[test]
    fn active_indices_ignores_complete_and_unselected() {
        // The head skips complete/unselected files, and a stale recent entry for
        // one of them can't make it active.
        let files = vec![
            fp(0, "a.mkv", true, 100, 100),
            fp(1, "b.mkv", false, 0, 100),
            fp(2, "c.mkv", true, 0, 100),
        ];
        let recent = HashSet::from([0, 1]);
        assert_eq!(active_indices(&files, Some(&recent)), HashSet::from([2]));
    }

    #[test]
    fn merge_target_groups_preserves_first_seen_order_across_torrents() {
        // Arrival order, not the sorted-by-hash order a BTreeMap would impose.
        let pairs = vec![
            ("c".to_string(), vec![0]),
            ("a".to_string(), vec![0]),
            ("b".to_string(), vec![0]),
        ];
        let hashes: Vec<String> = merge_target_groups(pairs).into_iter().map(|(h, _)| h).collect();
        assert_eq!(hashes, ["c", "a", "b"]);
    }

    #[test]
    fn merge_target_groups_dedups_indices_keeping_first_order() {
        let pairs = vec![("a".to_string(), vec![2, 0, 1]), ("a".to_string(), vec![1, 3])];
        assert_eq!(merge_target_groups(pairs), vec![("a".to_string(), vec![2, 0, 1, 3])]);
    }

    #[test]
    fn aggregate_empty_is_idle() {
        assert_eq!(aggregate_torrent_state(TorrentStatsState::Live, &[]), TorrentState::Idle);
    }

    #[test]
    fn aggregate_all_complete() {
        let files = [file(FileState::Complete), file(FileState::Complete)];
        assert_eq!(aggregate_torrent_state(TorrentStatsState::Live, &files), TorrentState::Complete);
    }

    #[test]
    fn aggregate_selection_complete_with_unselected_remainder() {
        // The user selected one file and it finished: the torrent is complete
        // even though other (never-wanted) media remain.
        let files = [file_sel(FileState::Complete, true), file_sel(FileState::Idle, false)];
        assert_eq!(aggregate_torrent_state(TorrentStatsState::Live, &files), TorrentState::Complete);
    }

    #[test]
    fn aggregate_all_complete_even_when_unselected() {
        // Everything is on disk: complete regardless of selection (e.g. all
        // files paused after the download finished).
        let files = [file_sel(FileState::Complete, false), file_sel(FileState::Complete, false)];
        assert_eq!(aggregate_torrent_state(TorrentStatsState::Live, &files), TorrentState::Complete);
    }

    #[test]
    fn aggregate_empty_selection_with_incomplete_files_is_idle() {
        let files = [file_sel(FileState::Idle, false), file_sel(FileState::Idle, false)];
        assert_eq!(aggregate_torrent_state(TorrentStatsState::Live, &files), TorrentState::Idle);
    }

    #[test]
    fn aggregate_selection_partially_complete_is_downloading() {
        let files = [file_sel(FileState::Complete, true), file_sel(FileState::Downloading, true)];
        assert_eq!(
            aggregate_torrent_state(TorrentStatsState::Live, &files),
            TorrentState::Downloading
        );
    }

    #[test]
    fn aggregate_mixed_downloading_paused_is_downloading() {
        let files = [file(FileState::Downloading), file(FileState::Paused)];
        assert_eq!(aggregate_torrent_state(TorrentStatsState::Live, &files), TorrentState::Downloading);
    }

    #[test]
    fn aggregate_only_paused_is_paused() {
        let files = [file(FileState::Paused), file(FileState::Idle)];
        assert_eq!(aggregate_torrent_state(TorrentStatsState::Paused, &files), TorrentState::Paused);
    }

    #[test]
    fn aggregate_initializing_short_circuits() {
        let files = [file(FileState::Complete)];
        assert_eq!(
            aggregate_torrent_state(TorrentStatsState::Initializing { paused: false }, &files),
            TorrentState::Initializing
        );
    }

    #[test]
    fn deselect_removes_target_keeps_other_media_and_subs() {
        // media 0,1 + subtitle 2 selected; deselect media 0 -> 1 and 2 stay.
        let current: HashSet<usize> = [0, 1, 2].into_iter().collect();
        let (next, freed) = compute_deselect(&[0, 1], &[2], &current, &[0]);
        let expected: HashSet<usize> = [1, 2].into_iter().collect();
        assert_eq!(next, expected);
        assert_eq!(freed, vec![0]);
    }

    #[test]
    fn deselect_last_media_drops_subtitles_too() {
        // media 0 + subtitle 1 selected; deselect the only media -> the subtitle
        // goes with it.
        let current: HashSet<usize> = [0, 1].into_iter().collect();
        let (next, freed) = compute_deselect(&[0], &[1], &current, &[0]);
        assert!(next.is_empty());
        assert_eq!(freed, vec![0, 1]);
    }

    #[test]
    fn deselect_of_unselected_index_leaves_selection_unchanged() {
        let current: HashSet<usize> = [0].into_iter().collect();
        let no_subs: [u32; 0] = [];
        let (next, freed) = compute_deselect(&[0, 1], &no_subs, &current, &[1]);
        let expected: HashSet<usize> = [0].into_iter().collect();
        assert_eq!(next, expected);
        assert_eq!(freed, vec![1]);
    }

    #[test]
    fn only_a_restore_keeps_its_pause_intent() {
        // An add is paused only so its selection lands before the engine starts,
        // so recovering one must never hand that flag back as user intent.
        assert!(!keep_paused(Finalize::Add, true));
        assert!(!keep_paused(Finalize::Add, false));
        assert!(keep_paused(Finalize::Restore, true));
        assert!(!keep_paused(Finalize::Restore, false));
    }
}
