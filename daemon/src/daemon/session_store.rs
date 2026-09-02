//! Magneto owns librqbit's session directory. librqbit restores each persisted
//! torrent from `{hash}.torrent` beside `session.json`, and falls back to a bare
//! magnet when those bytes are missing or empty, which it then resolves inside
//! session construction. Repairing the directory first, from magneto's own copy,
//! keeps a torn write from stalling the boot.

use std::path::{Path, PathBuf};

use serde_json::Value;
use tracing::{info, warn};

use crate::metadata::{torrent_file_path, write_durable};

pub fn session_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("session")
}

/// Flush a freshly written fastresume bitfield. The engine writes it in the
/// background, so a crash can leave it torn or all zeros, and the next boot
/// then discards data that is on disk.
pub fn sync_bitfield(data_dir: &Path, info_hash: &str) {
    let path = session_dir(data_dir).join(format!("{info_hash}.bitv"));
    match std::fs::OpenOptions::new().read(true).write(true).open(&path) {
        Ok(f) => {
            if let Err(e) = f.sync_all() {
                warn!(error = %e, path = %path.display(), "failed to flush fastresume bitfield");
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(error = %e, path = %path.display(), "failed to open fastresume bitfield"),
    }
}

/// Drop the engine's own files for a torrent that has just been deleted. The
/// engine removes them itself but swallows the failure, and a row left behind
/// with a usable torrent file would come back on the next boot.
pub fn forget(data_dir: &Path, info_hash: &str) {
    let dir = session_dir(data_dir);
    for ext in ["torrent", "bitv"] {
        let path = dir.join(format!("{info_hash}.{ext}"));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => warn!(error = %e, path = %path.display(), "failed to remove session file"),
        }
    }
}

/// The parts of a session row the engine requires. Mirrors librqbit's
/// `SerializedTorrent`: a row missing any of these, or holding the wrong type,
/// makes its whole typed database fail to load.
#[derive(serde::Deserialize)]
struct Row {
    info_hash: String,
    #[allow(dead_code)]
    trackers: std::collections::HashSet<String>,
    output_folder: PathBuf,
    #[allow(dead_code)]
    only_files: Option<Vec<usize>>,
    #[allow(dead_code)]
    is_paused: bool,
}

impl Row {
    fn parse(row: &Value) -> Option<Self> {
        let row: Self = serde_json::from_value(row.clone()).ok()?;
        // The hash names the sidecar files, so anything but a hex id20 is junk.
        let hex = row.info_hash.len() == 40
            && row.info_hash.bytes().all(|b| b.is_ascii_hexdigit());
        hex.then_some(row)
    }
}

/// Bring the session directory and magneto's own `.torrent` copies back in step.
/// Nothing here fails the boot: an unreadable database is set aside, and a row
/// with no usable metainfo on either side is dropped so reconcile can re-add the
/// torrent from magneto's record. A row whose session copy is fine is always
/// kept, whether or not magneto knows about it.
pub fn repair(data_dir: &Path) {
    let dir = session_dir(data_dir);
    let db = dir.join("session.json");
    let text = match std::fs::read_to_string(&db) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            warn!(error = %e, path = %db.display(), "session database unreadable");
            return quarantine(&db);
        }
    };
    let mut root: Value = match serde_json::from_str(&text) {
        Ok(root) => root,
        Err(e) => {
            warn!(error = %e, "session database unparseable");
            return quarantine(&db);
        }
    };
    let Some(rows) = root.get_mut("torrents").and_then(Value::as_object_mut) else {
        warn!("session database has no torrents map");
        return quarantine(&db);
    };

    // One row the engine cannot deserialize fails the whole database, so a row
    // that does not match its shape is dropped like one with no torrent file.
    let mut doomed: Vec<String> = Vec::new();
    for (id, row) in rows.iter() {
        // The engine keys this map by torrent id, so a non-numeric key fails it
        // as surely as a malformed row.
        let Some(row) = id.parse::<usize>().ok().and(Row::parse(row)) else {
            doomed.push(id.clone());
            continue;
        };
        if !sync_metainfo(data_dir, &dir, &row.info_hash) {
            doomed.push(id.clone());
            continue;
        }
        drop_stale_bitfield(&dir, &row.info_hash, &row.output_folder);
    }
    if doomed.is_empty() {
        return;
    }
    warn!(count = doomed.len(), "dropping unusable session rows");
    rows.retain(|id, _| !doomed.contains(id));
    match serde_json::to_vec(&root) {
        Ok(bytes) => {
            if let Err(e) = write_durable(&db, &bytes) {
                warn!(error = %e, "failed to rewrite session database");
            }
        }
        Err(e) => warn!(error = %e, "failed to serialize session database"),
    }
}

/// Make both copies of one torrent's metainfo usable. Returns false only when
/// neither is: the session copy is what librqbit restores from, magneto's copy
/// is what reconcile re-adds from.
fn sync_metainfo(data_dir: &Path, dir: &Path, info_hash: &str) -> bool {
    let session_copy = dir.join(format!("{info_hash}.torrent"));
    let own_copy = torrent_file_path(data_dir, info_hash);
    match (usable(&session_copy, info_hash), usable(&own_copy, info_hash)) {
        (true, true) => true,
        (true, false) => {
            restore(&session_copy, &own_copy);
            true
        }
        (false, true) => restore(&own_copy, &session_copy),
        (false, false) => false,
    }
}

/// Drop a fastresume bitfield that claims nothing while the files hold data.
/// The engine trusts a right-length bitfield without re-hashing, so one that a
/// crash left zeroed makes it re-download bytes that are already there. Removing
/// it costs one full check and recovers the data.
fn drop_stale_bitfield(dir: &Path, info_hash: &str, output_folder: &Path) {
    let path = dir.join(format!("{info_hash}.bitv"));
    let Ok(bytes) = std::fs::read(&path) else { return };
    if bytes.is_empty() || bytes.iter().any(|b| *b != 0) {
        return;
    }
    if !holds_data(output_folder, &dir.join(format!("{info_hash}.torrent"))) {
        return;
    }
    match std::fs::remove_file(&path) {
        Ok(()) => warn!(
            hash = %crate::daemon::short(info_hash),
            "fastresume bitfield claimed nothing while files hold data; re-checking"
        ),
        Err(e) => warn!(error = %e, path = %path.display(), "failed to drop stale bitfield"),
    }
}

/// Whether any of the torrent's files has blocks allocated on disk. Length alone
/// says nothing: the engine preallocates selected files as sparse, so an empty
/// file of full length reads as no data.
#[cfg(unix)]
fn holds_data(output_folder: &Path, metainfo_path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(bytes) = std::fs::read(metainfo_path) else { return false };
    let Ok(meta) = librqbit::torrent_from_bytes(&bytes) else { return false };
    let Ok(info) = meta.info.data.validate() else { return false };
    info.iter_file_details().any(|f| {
        std::fs::metadata(output_folder.join(f.filename.to_pathbuf()))
            .is_ok_and(|m| m.blocks() > 0)
    })
}

// Sparse preallocation is not detectable the same way off unix, and guessing
// from length would force a full check of every torrent on every boot.
#[cfg(not(unix))]
fn holds_data(_output_folder: &Path, _metainfo_path: &Path) -> bool {
    false
}

/// Usable means the file parses as metainfo for the hash it is filed under. A
/// truncated write still leaves bytes behind, and a file holding some other
/// torrent would restore the wrong one.
fn usable(path: &Path, info_hash: &str) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    librqbit::torrent_from_bytes(&bytes)
        .is_ok_and(|t| t.info_hash.as_string().eq_ignore_ascii_case(info_hash))
}

fn restore(from: &Path, to: &Path) -> bool {
    match std::fs::read(from).and_then(|bytes| write_durable(to, &bytes)) {
        Ok(()) => {
            info!(path = %to.display(), "restored torrent file");
            true
        }
        Err(e) => {
            warn!(error = %e, path = %to.display(), "failed to restore torrent file");
            false
        }
    }
}

fn quarantine(db: &Path) {
    let backup = db.with_extension("json.bak");
    match std::fs::rename(db, &backup) {
        Ok(()) => warn!(backup = %backup.display(), "session database set aside"),
        Err(e) => warn!(error = %e, "failed to set aside session database"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> Value {
        serde_json::json!({
            "info_hash": "aabbccddeeff00112233445566778899aabbccdd",
            "trackers": ["http://tracker.example/announce"],
            "output_folder": "/downloads/Show.S01",
            "only_files": [0, 1],
            "is_paused": false,
        })
    }

    #[test]
    fn engine_row_shape_is_accepted() {
        let row = Row::parse(&healthy()).expect("healthy row parses");
        assert_eq!(row.info_hash.len(), 40);
        assert_eq!(row.output_folder, PathBuf::from("/downloads/Show.S01"));

        let mut null_selection = healthy();
        null_selection["only_files"] = Value::Null;
        assert!(Row::parse(&null_selection).is_some(), "a whole-torrent selection is valid");
    }

    #[test]
    fn junk_rows_are_rejected() {
        for (field, value) in [
            ("info_hash", Value::from(42)),
            ("info_hash", Value::from("nothex")),
            ("output_folder", Value::from(7)),
            ("trackers", Value::from("not-a-list")),
            ("only_files", Value::from("all")),
            ("is_paused", Value::from("yes")),
        ] {
            let mut row = healthy();
            row[field] = value;
            assert!(Row::parse(&row).is_none(), "{field} with the wrong type must be rejected");
        }
        // `only_files` is the one the engine tolerates missing: serde reads an
        // absent Option as None, which means "the whole torrent".
        for field in ["info_hash", "trackers", "output_folder", "is_paused"] {
            let mut row = healthy();
            row.as_object_mut().unwrap().remove(field);
            assert!(Row::parse(&row).is_none(), "a row missing {field} must be rejected");
        }
    }
}
