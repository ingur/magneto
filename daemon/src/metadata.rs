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
    /// Load the store, returning `(store, recovered)`. `recovered` is true when
    /// the file existed but could not be parsed and a fresh store was started in
    /// its place. The caller must then skip destructive cleanup (which keys off
    /// per-file `persisted` flags that the fresh store no longer has).
    pub fn load_or_create(path: &Path) -> Result<(Self, bool)> {
        if !path.exists() {
            let store = Self::default();
            store.save(path)?;
            return Ok((store, false));
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        match serde_json::from_str::<Self>(&text) {
            Ok(store) => Ok((store, false)),
            Err(e) => {
                let backup = path.with_extension("json.bak");
                warn!(error = %e, backup = %backup.display(), "metadata.json unparseable; backing up and starting fresh");
                let _ = std::fs::rename(path, &backup);
                let store = Self::default();
                store.save(path)?;
                Ok((store, true))
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent();
        if let Some(parent) = parent {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self).context("serializing metadata")?;
        // Write + fsync the temp file so its bytes are durable before the rename
        // publishes it: a torn write must never replace good metadata, since a
        // fresh-parsed store would drive cleanup to delete persisted downloads.
        {
            let mut f = std::fs::File::create(&tmp)
                .with_context(|| format!("creating {}", tmp.display()))?;
            f.write_all(text.as_bytes())
                .with_context(|| format!("writing {}", tmp.display()))?;
            f.sync_all().with_context(|| format!("syncing {}", tmp.display()))?;
        }
        std::fs::rename(&tmp, path)
            .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
        // fsync the directory so the rename itself survives a crash.
        if let Some(parent) = parent
            && let Ok(dir) = std::fs::File::open(parent)
        {
            let _ = dir.sync_all();
        }
        Ok(())
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
    let dir = torrents_dir(data_dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{info_hash}.torrent"));
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
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
            },
        );
        store
    }

    #[test]
    fn save_then_load_round_trips_durably_without_recovery() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("metadata.json");
        sample().save(&path).unwrap();
        let (loaded, recovered) = MetadataStore::load_or_create(&path).unwrap();
        assert!(!recovered);
        assert!(loaded.torrents.values().next().unwrap().files.get(&0).unwrap().persisted);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_creates_without_recovery() {
        let dir = temp_dir("missing");
        let path = dir.join("metadata.json");
        let (loaded, recovered) = MetadataStore::load_or_create(&path).unwrap();
        assert!(!recovered);
        assert!(loaded.torrents.is_empty());
        assert!(path.exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_file_recovers_fresh_and_backs_up() {
        let dir = temp_dir("corrupt");
        let path = dir.join("metadata.json");
        sample().save(&path).unwrap();
        std::fs::write(&path, b"{ not valid json").unwrap();
        let (loaded, recovered) = MetadataStore::load_or_create(&path).unwrap();
        // `recovered` is the flag that makes startup skip destructive cleanup.
        assert!(recovered);
        assert!(loaded.torrents.is_empty());
        assert!(path.with_extension("json.bak").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
