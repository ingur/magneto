//! The one place magneto stops tracking a torrent or deletes its bytes.
//!
//! Deleting files needs affirmative evidence that they are magneto's to drop: an
//! entry that finished its file check, has a file map, and has no file the user
//! marked persisted. Absence of evidence never authorizes a delete, so a pending
//! add, a store rebuilt after a bad read, and a torrent the engine never
//! restored all keep their data.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use magneto_core::protocol::{Outbound, RemovalReason, TorrentRemovedEvent};

use crate::daemon::commands::{current_selection, file_local_path, subtitle_indices};
use crate::daemon::session::TorrentHandle;
use crate::daemon::{Daemon, short};
use crate::metadata::TorrentMetadata;

/// What happens to the bytes on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Files {
    /// Leave every byte where it is.
    Keep,
    /// Delete the files magneto selected, plus the empty ones the engine created
    /// for the rest. Bytes that were on disk before the add stay.
    Managed,
    /// Delete every file in the torrent.
    All,
}

/// Whether policy allows magneto to drop this torrent's bytes.
pub fn disposable(entry: &TorrentMetadata) -> bool {
    entry.finalized && !entry.files.is_empty() && !entry.files.values().any(|f| f.persisted)
}

/// Drop a torrent from the session and from everything magneto tracks.
pub async fn remove(
    daemon: &mut Daemon,
    info_hash: &str,
    reason: RemovalReason,
    files: Files,
    fallback_launched: bool,
) -> anyhow::Result<()> {
    let Some(handle) = daemon.session.get(info_hash) else {
        if matches!(reason, RemovalReason::User) {
            anyhow::bail!("no torrent with info_hash {info_hash}");
        }
        // A torrent the engine did not restore keeps its record: the data is
        // still on disk and reconcile re-adds it from the saved torrent file.
        info!(hash = %short(info_hash), ?reason, outcome = "kept: not in session", "removal");
        return Ok(());
    };
    let doomed = match files {
        Files::Managed => managed_indices(daemon, &handle, info_hash),
        Files::Keep | Files::All => Vec::new(),
    };

    // The saved torrent file is what reconcile re-adds from, so it goes first: a
    // crash after the engine delete must never resurrect a torrent whose bytes
    // are gone. If the delete fails instead, boot repair restores this copy from
    // the engine's own sidecar.
    crate::metadata::delete_torrent_bytes(&daemon.data_dir, info_hash);
    info!(hash = %short(info_hash), ?reason, ?files, "removing torrent");
    if let Err(e) = daemon.session.delete(info_hash, files == Files::All).await {
        if daemon.session.get(info_hash).is_some() {
            return Err(e.context("removing torrent from the engine"));
        }
        warn!(hash = %short(info_hash), error = %e, "engine delete failed after the torrent left the session");
    }
    crate::daemon::session_store::forget(&daemon.data_dir, info_hash);
    {
        let mut meta = daemon.metadata.write();
        meta.remove(info_hash);
        if let Err(e) = meta.save(&daemon.metadata_path) {
            warn!(hash = %short(info_hash), error = %e, "metadata save failed after removal");
        }
    }
    daemon.rechecked.remove(info_hash);
    if files == Files::Managed {
        reclaim(&handle, &doomed, &daemon.started.downloads.dir);
    }
    daemon.broadcast(Outbound::TorrentRemoved(TorrentRemovedEvent {
        info_hash: info_hash.to_string(),
        reason,
        fallback_launched,
    }));
    Ok(())
}

/// Delete the on-disk bytes of these file indices, then drop the directories
/// they leave empty. Best-effort: a missing file is skipped, an error is logged.
/// Truncating frees the blocks even while the file is still held open; the
/// unlink then drops the directory entry.
pub fn reclaim(handle: &TorrentHandle, indices: &[u32], download_dir: &Path) {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for idx in indices {
        let Some(path) = file_local_path(handle, *idx as usize, download_dir) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = f.set_len(0);
        }
        if let Err(e) = std::fs::remove_file(&path) {
            warn!(index = idx, path = %path.display(), error = %e, "failed to delete file");
            continue;
        }
        let mut dir = path.parent();
        while let Some(d) = dir {
            if d == download_dir || !d.starts_with(download_dir) {
                break;
            }
            dirs.push(d.to_path_buf());
            dir = d.parent();
        }
    }
    // Deepest first, and only ever empty ones: `remove_dir` refuses the rest.
    dirs.sort_unstable_by_key(|d| std::cmp::Reverse(d.as_os_str().len()));
    dirs.dedup();
    for dir in dirs {
        let _ = std::fs::remove_dir(&dir);
    }
}

/// The files magneto is answerable for: the ones in its file map, the subtitles
/// it selected alongside them, and any file the engine created and left empty.
fn managed_indices(daemon: &Daemon, handle: &TorrentHandle, info_hash: &str) -> Vec<u32> {
    let mut managed: HashSet<u32> = daemon
        .metadata
        .read()
        .get(info_hash)
        .map(|e| e.files.keys().copied().collect())
        .unwrap_or_default();
    let selected = current_selection(handle);
    managed.extend(
        subtitle_indices(handle)
            .into_iter()
            .filter(|i| selected.contains(&(*i as usize))),
    );
    let empty: Vec<u32> = handle
        .with_metadata(|m| {
            (0..m.file_infos.len() as u32)
                .filter(|i| !managed.contains(i))
                .filter(|i| {
                    file_local_path(handle, *i as usize, &daemon.started.downloads.dir)
                        .and_then(|p| std::fs::metadata(p).ok())
                        .is_some_and(|m| m.len() == 0)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut out: Vec<u32> = managed.into_iter().collect();
    out.extend(empty);
    out.sort_unstable();
    out
}
