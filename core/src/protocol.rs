use serde::{Deserialize, Serialize};

use crate::config::Config;

pub type InfoHash = String;

/// True for sources `add_torrent` accepts verbatim: magnet URIs and HTTP(S)
/// torrent URLs. Everything else is a local file path the caller must turn
/// into bytes itself (the CLI reads it; the app routes it through its read
/// fence).
pub fn is_direct_source(source: &str) -> bool {
    let lower = source.to_ascii_lowercase();
    lower.starts_with("magnet:") || lower.starts_with("http://") || lower.starts_with("https://")
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Outbound {
    #[serde(rename = "response")]
    Response { id: String, result: serde_json::Value },
    #[serde(rename = "error")]
    Error { id: String, error: String },
    #[serde(rename = "snapshot")]
    Snapshot(SnapshotEvent),
    #[serde(rename = "stats")]
    Stats(StatsEvent),
    #[serde(rename = "torrent_added")]
    TorrentAdded(TorrentAddedEvent),
    #[serde(rename = "torrent_ready")]
    TorrentReady(TorrentDetail),
    #[serde(rename = "torrent_complete")]
    TorrentComplete { info_hash: InfoHash },
    #[serde(rename = "torrent_removed")]
    TorrentRemoved(TorrentRemovedEvent),
    #[serde(rename = "torrent_error")]
    TorrentError { info_hash: InfoHash, error: String },
    #[serde(rename = "player_launch_failed")]
    PlayerLaunchFailed(PlayerLaunchFailedEvent),
    #[serde(rename = "config_changed")]
    ConfigChanged(ConfigChangedEvent),
    #[serde(rename = "daemon_restarting")]
    DaemonRestarting,
    #[serde(rename = "daemon_shutdown")]
    DaemonShutdown,
}

impl Outbound {
    pub fn response<T: Serialize>(id: String, result: T) -> Self {
        Self::Response { id, result: serde_json::to_value(result).unwrap_or(serde_json::Value::Null) }
    }
    pub fn error(id: String, error: impl Into<String>) -> Self {
        Self::Error { id, error: error.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    Torrent { info_hash: InfoHash },
    File { info_hash: InfoHash, file_index: u32 },
    Folder { info_hash: InfoHash, path: String },
}

impl Target {
    pub fn info_hash(&self) -> &str {
        match self {
            Self::Torrent { info_hash } | Self::File { info_hash, .. } | Self::Folder { info_hash, .. } => info_hash,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TorrentState {
    Initializing,
    Downloading,
    Paused,
    Idle,
    Complete,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    Complete,
    Downloading,
    Queued,
    Paused,
    Idle,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Magnet,
    Url,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentSummary {
    pub info_hash: InfoHash,
    pub name: Option<String>,
    // Original add source + kind, for copy-source / re-add. None until metadata is
    // recorded; for file kind, source is a local .torrent path.
    pub source: Option<String>,
    pub source_kind: Option<SourceKind>,
    pub state: TorrentState,
    // Byte fields are bytes; download_speed/upload_speed are BYTES PER SECOND.
    // total_bytes_all sums all managed (media) files; total_bytes_selected is the
    // selected-for-download subset and is the progress denominator.
    pub total_bytes_all: u64,
    pub total_bytes_selected: u64,
    pub downloaded_bytes: u64,
    pub download_speed: f64,
    pub upload_speed: f64,
    pub file_count: u32,
    pub complete_count: u32,
    pub selected_count: u32,
    pub persisted_count: u32,
    pub shared_count: u32,
    pub is_initializing: bool,
    pub is_complete: bool,
    pub is_seeding: bool,
    pub is_paused: bool,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub index: u32,
    pub path: String,
    pub size: u64,
    pub downloaded_bytes: u64,
    pub selected: bool,
    pub state: FileState,
    pub persisted: bool,
    pub shared: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentDetail {
    #[serde(flatten)]
    pub summary: TorrentSummary,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotEvent {
    pub daemon: DaemonInfo,
    pub config: Config,
    pub torrents: Vec<TorrentSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaemonInfo {
    pub version: &'static str,
    pub status: &'static str,
    // The control port the daemon bound. Loopback stream URLs are
    // http://127.0.0.1:{control_port}/stream/{info_hash}/{file_index}/{name},
    // where {name} is the url-encoded file basename.
    pub control_port: u16,
    pub lan_port: u16,
    pub upnp_active: bool,
    // Restart-required fields whose saved value differs from the running process.
    pub pending_restart: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StatsEvent {
    pub torrents: Vec<TorrentStatsDelta>,
    pub files: Vec<FileStatsDelta>,
}

impl StatsEvent {
    pub fn is_empty(&self) -> bool {
        self.torrents.is_empty() && self.files.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentStatsDelta {
    pub info_hash: InfoHash,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<TorrentState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_bytes_selected: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_speed: Option<f64>, // bytes per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upload_speed: Option<f64>, // bytes per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_paused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_seeding: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub complete_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persisted_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileStatsDelta {
    pub info_hash: InfoHash,
    pub file_index: u32,
    pub downloaded_bytes: u64,
    pub state: FileState,
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentAddedEvent {
    pub info_hash: InfoHash,
    pub source: String,
    pub state: TorrentState,
    pub already_existed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TorrentRemovedEvent {
    pub info_hash: InfoHash,
    pub reason: RemovalReason,
    pub fallback_launched: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemovalReason {
    User,
    NoMedia,
    Fallback,
    Cleanup,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerLaunchFailedEvent {
    pub info_hash: Option<InfoHash>,
    pub kind: PlayerLaunchKind,
    pub error: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerLaunchKind {
    Autoplay,
    Play,
    Fallback,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigChangedEvent {
    pub config: Config,
    pub restart_required: bool,
    pub pending_restart: Vec<String>,
}

// ---- Command payloads ----

#[derive(Debug, Clone, Deserialize)]
pub struct AddTorrentReq {
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddTorrentResp {
    pub info_hash: InfoHash,
    pub name: Option<String>,
    pub state: Option<TorrentState>,
    pub files: Option<Vec<FileEntry>>,
    pub media: Option<bool>,
    pub already_existed: bool,
    pub fallback_launched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<FallbackReason>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    NotConfigured,
    SpawnFailed,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListTorrentsResp {
    pub torrents: Vec<TorrentSummary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GetTorrentReq {
    pub info_hash: InfoHash,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoveTorrentReq {
    pub info_hash: InfoHash,
    pub delete_files: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FallbackReq {
    pub info_hash: InfoHash,
}

#[derive(Debug, Clone, Serialize)]
pub struct FallbackResp {
    pub launched: bool,
    pub removed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<FallbackReason>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TargetsReq {
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AffectedResp {
    pub affected: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetPersistReq {
    pub targets: Vec<Target>,
    pub persisted: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetSharedReq {
    pub targets: Vec<Target>,
    pub shared: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayResp {
    pub items: Vec<PlayItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayItem {
    pub name: String,
    pub uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResolveLocalPathReq {
    pub target: Target,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveLocalPathResp {
    pub path: String,
    pub kind: PathKind,
    pub exists: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PathKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetConfigResp {
    pub config: Config,
    pub restart_required: bool,
    pub pending_restart: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OkResp {
    pub ok: bool,
}

impl OkResp {
    pub const TRUE: Self = Self { ok: true };
}

#[derive(Debug, Clone, Serialize)]
pub struct PingResp {
    pub pong: bool,
}

impl PingResp {
    pub const TRUE: Self = Self { pong: true };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_sources_are_magnet_and_http() {
        assert!(is_direct_source("MAGNET:?xt=urn:btih:abc"));
        assert!(is_direct_source("https://example.com/file.torrent"));
        assert!(is_direct_source("http://example.com/x"));
        assert!(!is_direct_source("/tmp/file.torrent"));
        assert!(!is_direct_source("file:///tmp/file.torrent"));
    }

    fn json_obj(o: &Outbound) -> serde_json::Map<String, serde_json::Value> {
        let v = serde_json::to_value(o).unwrap();
        v.as_object().unwrap().clone()
    }

    #[test]
    fn response_envelope_has_type_id_result() {
        let out = Outbound::response("req-1".into(), serde_json::json!({"pong": true}));
        let obj = json_obj(&out);
        assert_eq!(obj.get("type").unwrap(), "response");
        assert_eq!(obj.get("id").unwrap(), "req-1");
        assert!(obj.contains_key("result"));
        assert!(!obj.contains_key("payload"));
    }

    #[test]
    fn error_envelope_has_type_id_error() {
        let out = Outbound::error("req-2".into(), "nope");
        let obj = json_obj(&out);
        assert_eq!(obj.get("type").unwrap(), "error");
        assert_eq!(obj.get("id").unwrap(), "req-2");
        assert_eq!(obj.get("error").unwrap(), "nope");
        assert!(!obj.contains_key("payload"));
    }

    #[test]
    fn torrent_ready_event_is_flat_not_payload_wrapped() {
        let detail = TorrentDetail {
            summary: TorrentSummary {
                info_hash: "abc".into(),
                name: Some("movie".into()),
                source: None,
                source_kind: None,
                state: TorrentState::Complete,
                total_bytes_all: 0,
                total_bytes_selected: 0,
                downloaded_bytes: 0,
                download_speed: 0.0,
                upload_speed: 0.0,
                file_count: 0,
                complete_count: 0,
                selected_count: 0,
                persisted_count: 0,
                shared_count: 0,
                is_initializing: false,
                is_complete: true,
                is_seeding: false,
                is_paused: false,
                added_at: "2026-06-05T00:00:00Z".into(),
            },
            files: vec![],
        };
        let obj = json_obj(&Outbound::TorrentReady(detail));
        assert_eq!(obj.get("type").unwrap(), "torrent_ready");
        assert_eq!(obj.get("info_hash").unwrap(), "abc");
        assert_eq!(obj.get("name").unwrap(), "movie");
        assert!(obj.contains_key("files"));
        assert!(!obj.contains_key("payload"));
    }

    #[test]
    fn unit_event_serializes_without_payload_wrapper() {
        let obj = json_obj(&Outbound::DaemonShutdown);
        assert_eq!(obj.get("type").unwrap(), "daemon_shutdown");
        assert!(!obj.contains_key("payload"));
    }
}
