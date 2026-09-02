// Wire types for the magneto daemon WebSocket protocol.
//
// Hand-mirrored from the daemon's `protocol.rs` / `config.rs` (the source of
// truth); kept in sync by hand. A ts-rs/specta codegen step could replace this
// later as an additive change.

export type InfoHash = string;

export type TorrentState =
  | "initializing"
  | "downloading"
  | "stalled"
  | "paused"
  | "idle"
  | "complete"
  | "error";

export type FileState = "complete" | "downloading" | "queued" | "paused" | "idle" | "error";

export type SourceKind = "magnet" | "url" | "file";

export type RemovalReason = "user" | "no_media" | "fallback" | "cleanup";

export type FallbackReason = "not_configured" | "spawn_failed";

export type PlayerLaunchKind = "autoplay" | "play" | "fallback";

export type PathKind = "file" | "folder";

export type Target =
  | { kind: "torrent"; info_hash: InfoHash }
  | { kind: "file"; info_hash: InfoHash; file_index: number }
  | { kind: "folder"; info_hash: InfoHash; path: string };

// ---- Config (config.rs) ----

export interface NetworkConfig {
  control_port: number;
  lan_port: number;
  upnp_enabled: boolean;
  server_name: string;
}

export interface DownloadsConfig {
  dir: string;
  fallback_app: string;
  fallback_args: string[];
  auto_download: boolean;
  persist_by_default: boolean;
  share_by_default: boolean;
  autoplay: boolean;
}

export interface MediaConfig {
  extensions: string[];
}

export interface PlayerConfig {
  command: string;
  args: string[];
}

export interface Config {
  network: NetworkConfig;
  downloads: DownloadsConfig;
  media: MediaConfig;
  player: PlayerConfig;
}

// ---- Core models ----

export interface TorrentSummary {
  info_hash: InfoHash;
  name: string | null;
  source: string;
  source_kind: SourceKind;
  state: TorrentState;
  // Engine error text, present only in the error state.
  error: string | null;
  // 0..1 while the engine checks files, null otherwise.
  check_progress: number | null;
  // Byte fields are bytes; download_speed/upload_speed are bytes per second.
  // total_bytes_all sums all managed (media) files; total_bytes_selected is the
  // selected-for-download subset and the progress denominator.
  total_bytes_all: number;
  total_bytes_selected: number;
  downloaded_bytes: number;
  download_speed: number;
  upload_speed: number;
  file_count: number;
  complete_count: number;
  selected_count: number;
  persisted_count: number;
  shared_count: number;
  is_paused: boolean;
  added_at: string;
}

export interface FileEntry {
  index: number;
  path: string;
  size: number;
  downloaded_bytes: number;
  selected: boolean;
  state: FileState;
  persisted: boolean;
  shared: boolean;
}

export interface TorrentDetail extends TorrentSummary {
  files: FileEntry[];
}

export interface DaemonInfo {
  version: string;
  status: string;
  started_at: string;
  control_port: number;
  lan_port: number;
  upnp_active: boolean;
  pending_restart: string[];
}

// ---- Stats deltas (only changed fields are present) ----

// A complete patch of the summary: every field but the key is optional and
// omitted when unchanged. The nullable fields arrive as an explicit null when
// cleared, so the patch must be merged as-is, never filtered for nulls.
export type TorrentStatsDelta = Pick<TorrentSummary, "info_hash"> &
  Partial<Omit<TorrentSummary, "info_hash">>;

export interface FileStatsDelta {
  info_hash: InfoHash;
  file_index: number;
  downloaded_bytes: number;
  state: FileState;
}

// ---- Request envelope (client -> daemon) ----

export interface Request {
  type: string;
  id: string;
  payload: unknown;
}

// ---- Outbound messages (daemon -> client), discriminated by `type` ----

export interface ResponseMsg {
  type: "response";
  id: string;
  result: unknown;
}

export interface ErrorMsg {
  type: "error";
  id: string;
  error: string;
}

export interface SnapshotEvent {
  type: "snapshot";
  daemon: DaemonInfo;
  config: Config;
  torrents: TorrentSummary[];
}

export interface StatsEvent {
  type: "stats";
  torrents: TorrentStatsDelta[];
  files: FileStatsDelta[];
}

export type TorrentAddedEvent = {
  type: "torrent_added";
  already_existed: boolean;
} & TorrentSummary;

export type TorrentReadyEvent = { type: "torrent_ready" } & TorrentDetail;

export interface TorrentCompleteEvent {
  type: "torrent_complete";
  info_hash: InfoHash;
}

export interface TorrentRemovedEvent {
  type: "torrent_removed";
  info_hash: InfoHash;
  reason: RemovalReason;
  fallback_launched: boolean;
}

export interface TorrentErrorEvent {
  type: "torrent_error";
  info_hash: InfoHash;
  error: string;
}

export interface PlayerLaunchFailedEvent {
  type: "player_launch_failed";
  info_hash: InfoHash | null;
  kind: PlayerLaunchKind;
  error: string;
}

export interface ConfigChangedEvent {
  type: "config_changed";
  config: Config;
  restart_required: boolean;
  pending_restart: string[];
}

export interface DaemonRestartingEvent {
  type: "daemon_restarting";
}

export interface DaemonShutdownEvent {
  type: "daemon_shutdown";
}

export type ServerEvent =
  | SnapshotEvent
  | StatsEvent
  | TorrentAddedEvent
  | TorrentReadyEvent
  | TorrentCompleteEvent
  | TorrentRemovedEvent
  | TorrentErrorEvent
  | PlayerLaunchFailedEvent
  | ConfigChangedEvent
  | DaemonRestartingEvent
  | DaemonShutdownEvent;

export type Outbound = ResponseMsg | ErrorMsg | ServerEvent;

// ---- Command responses (client <- daemon, inside the response envelope) ----

export interface AddTorrentResp {
  info_hash: InfoHash;
  name: string | null;
  state: TorrentState | null;
  files: FileEntry[] | null;
  media: boolean | null;
  already_existed: boolean;
  fallback_launched: boolean;
  fallback_reason?: FallbackReason;
}

export interface ResolveLocalPathResp {
  path: string;
  kind: PathKind;
  exists: boolean;
}

export interface AffectedResp {
  affected: number;
}

export interface SetConfigResp {
  config: Config;
  restart_required: boolean;
  pending_restart: string[];
}
