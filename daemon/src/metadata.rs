use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

use magneto_core::protocol::SourceKind;

pub type InfoHash = String;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetadataStore {
    pub torrents: BTreeMap<InfoHash, TorrentMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentMetadata {
    pub source: String,
    pub source_kind: SourceKind,
    pub added_at: DateTime<Utc>,
    pub files: BTreeMap<u32, FileMetadata>,
    // False from the add until the file check finishes, while `files` is still
    // empty. Cleanup must read that as unknown, not as "nothing persisted".
    #[serde(default = "default_finalized")]
    pub finalized: bool,
}

// Entries written before this field existed had already been finalized.
fn default_finalized() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FileMetadata {
    pub persisted: bool,
    pub shared: bool,
    // User paused this file (deselected, bytes kept on disk). Distinguishes a
    // "paused" file from one that was never wanted ("idle"). Defaulted so
    // metadata.json written before this field still parses on upgrade.
    #[serde(default)]
    pub paused: bool,
}

impl MetadataStore {
    /// Load the store, replacing an unreadable one with a fresh store and a
    /// `.bak` copy of what was there. Missing and unreadable are deliberately
    /// indistinguishable to callers: a store with no entry for a torrent says
    /// nothing about it, and nothing may be deleted on that basis.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let store = Self::default();
                store.save(path)?;
                return Ok(store);
            }
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        match serde_json::from_str::<Self>(&text) {
            Ok(store) => Ok(store),
            Err(e) => {
                let backup = path.with_extension("json.bak");
                warn!(error = %e, backup = %backup.display(), "metadata.json unparseable; backing up and starting fresh");
                let _ = std::fs::rename(path, &backup);
                let store = Self::default();
                store.save(path)?;
                Ok(store)
            }
        }
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self).context("serializing metadata")?;
        write_durable(path, text.as_bytes())
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn get(&self, info_hash: &str) -> Option<&TorrentMetadata> {
        self.torrents.get(info_hash)
    }

    pub fn get_mut(&mut self, info_hash: &str) -> Option<&mut TorrentMetadata> {
        self.torrents.get_mut(info_hash)
    }

    pub fn insert(&mut self, info_hash: InfoHash, entry: TorrentMetadata) {
        self.torrents.insert(info_hash, entry);
    }

    pub fn remove(&mut self, info_hash: &str) -> Option<TorrentMetadata> {
        self.torrents.remove(info_hash)
    }

    pub fn contains(&self, info_hash: &str) -> bool {
        self.torrents.contains_key(info_hash)
    }
}

pub fn torrents_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("torrents")
}

pub fn torrent_file_path(data_dir: &Path, info_hash: &str) -> PathBuf {
    torrents_dir(data_dir).join(format!("{info_hash}.torrent"))
}

pub fn save_torrent_bytes(data_dir: &Path, info_hash: &str, bytes: &[u8]) -> Result<PathBuf> {
    let path = torrent_file_path(data_dir, info_hash);
    write_durable(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

/// Publish bytes so a crash leaves either the old file or the new one. Both
/// metadata.json and the saved `.torrent` copies are recovery authority, so a
/// torn write of either costs the user data.
pub fn write_durable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent();
    if let Some(parent) = parent {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Some(parent) = parent
        && let Ok(dir) = std::fs::File::open(parent)
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

pub fn delete_torrent_bytes(data_dir: &Path, info_hash: &str) {
    let path = torrent_file_path(data_dir, info_hash);
    if path.exists()
        && let Err(e) = std::fs::remove_file(&path)
    {
        warn!(error = %e, path = %path.display(), "failed to delete saved .torrent file");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_preserves_integer_keyed_files_map() {
        let mut store = MetadataStore::default();
        let mut files = BTreeMap::new();
        files.insert(0u32, FileMetadata { persisted: true, shared: false, paused: false });
        files.insert(2u32, FileMetadata { persisted: false, shared: true, paused: true });
        store.insert(
            "abcdef0123456789abcdef0123456789abcdef01".into(),
            TorrentMetadata {
                source: "magnet:?xt=urn:btih:abcd".into(),
                source_kind: SourceKind::Magnet,
                added_at: chrono::Utc::now(),
                files,
                finalized: true,
            },
        );

        let json = serde_json::to_string(&store).unwrap();
        assert!(json.contains("\"0\""));
        assert!(json.contains("\"2\""));

        let back: MetadataStore = serde_json::from_str(&json).unwrap();
        let entry = back.torrents.values().next().unwrap();
        assert_eq!(entry.files.len(), 2);
        assert!(entry.files.get(&0).unwrap().persisted);
        assert!(!entry.files.get(&0).unwrap().shared);
        assert!(!entry.files.get(&2).unwrap().persisted);
        assert!(entry.files.get(&2).unwrap().shared);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("magneto-meta-{}-{tag}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> MetadataStore {
        let mut store = MetadataStore::default();
        let mut files = BTreeMap::new();
        files.insert(0u32, FileMetadata { persisted: true, shared: false, paused: false });
        store.insert(
            "abcdef0123456789abcdef0123456789abcdef01".into(),
            TorrentMetadata {
                source: "magnet:?xt=urn:btih:abcd".into(),
                source_kind: SourceKind::Magnet,
                added_at: chrono::Utc::now(),
                files,
                finalized: true,
            },
        );
        store
    }

    #[test]
    fn save_then_load_round_trips_durably() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("metadata.json");
        sample().save(&path).unwrap();
        let loaded = MetadataStore::load_or_create(&path).unwrap();
        assert!(loaded.torrents.values().next().unwrap().files.get(&0).unwrap().persisted);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_creates_empty_store() {
        let dir = temp_dir("missing");
        let path = dir.join("metadata.json");
        let loaded = MetadataStore::load_or_create(&path).unwrap();
        assert!(loaded.torrents.is_empty());
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_file_starts_fresh_and_backs_up() {
        let dir = temp_dir("corrupt");
        let path = dir.join("metadata.json");
        sample().save(&path).unwrap();
        std::fs::write(&path, b"{ not valid json").unwrap();
        let loaded = MetadataStore::load_or_create(&path).unwrap();
        assert!(loaded.torrents.is_empty());
        assert!(path.with_extension("json.bak").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn entry_without_finalized_field_reads_as_finalized() {
        let json = r#"{"torrents":{"aa":{"source":"magnet:?xt=urn:btih:aa","source_kind":"magnet","added_at":"2024-01-01T00:00:00Z","files":{}}}}"#;
        let store: MetadataStore = serde_json::from_str(json).unwrap();
        assert!(store.get("aa").unwrap().finalized);
    }
}
