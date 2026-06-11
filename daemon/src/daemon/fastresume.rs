//! Cleanup of librqbit's per-torrent fastresume files. librqbit names them by
//! info_hash under the session dir and removes them only best-effort (swallowing
//! errors), so magneto owns the removal to keep a re-add of the same torrent clean.

use std::path::{Path, PathBuf};

use tracing::warn;

/// The directory librqbit writes its fastresume files into.
pub fn session_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("session")
}

/// Remove the `.bitv` and `.torrent` fastresume files for one torrent.
/// Best-effort: a missing file is fine, an error is logged. `info_hash` is the
/// lowercase 40-hex string librqbit uses for the filenames.
pub fn delete_fastresume(data_dir: &Path, info_hash: &str) {
    let dir = session_dir(data_dir);
    for ext in ["bitv", "torrent"] {
        let path = dir.join(format!("{info_hash}.{ext}"));
        if path.exists()
            && let Err(e) = std::fs::remove_file(&path)
        {
            warn!(path = %path.display(), error = %e, "failed to delete fastresume file");
        }
    }
}
