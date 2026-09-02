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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Files {
    Keep,
    /// magneto's own: the file map, its selected subtitles, and the empty files
    /// the engine created for the rest. Bytes from before the add stay.
    Managed,
    All,
}

/// Whether policy allows magneto to drop this torrent's bytes.
pub fn disposable(entry: &TorrentMetadata) -> bool {
    entry.finalized && !entry.files.is_empty() && !entry.files.values().any(|f| f.persisted)
}

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
    daemon.metadata.write().remove(info_hash);
    let _ = daemon.save_metadata();
    daemon.rechecked.remove(info_hash);
    daemon.checks.remove(info_hash);
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;

    use super::*;
    use magneto_core::protocol::SourceKind;

    use crate::metadata::FileMetadata;

    fn entry(finalized: bool, files: &[(u32, bool)]) -> TorrentMetadata {
        TorrentMetadata {
            source: "magnet:?xt=urn:btih:aa".into(),
            source_kind: SourceKind::Magnet,
            added_at: Utc::now(),
            files: files
                .iter()
                .map(|(idx, persisted)| {
                    (*idx, FileMetadata { persisted: *persisted, shared: false, paused: false })
                })
                .collect::<BTreeMap<_, _>>(),
            finalized,
        }
    }

    #[test]
    fn only_a_finished_record_with_nothing_kept_is_disposable() {
        assert!(disposable(&entry(true, &[(0, false), (1, false)])));
        // One file the user asked to keep protects the whole torrent.
        assert!(!disposable(&entry(true, &[(0, true), (1, false)])));
        // An add that never classified, and a record rebuilt after a bad read,
        // both look like "nothing kept" and must not.
        assert!(!disposable(&entry(false, &[])));
        assert!(!disposable(&entry(false, &[(0, false)])));
        assert!(!disposable(&entry(true, &[])));
    }
}
