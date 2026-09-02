// Projection: the single owner of the row model. Turns the daemon's two
// flat shapes (TorrentSummary[] at root, TorrentDetail.files inside a
// torrent) into the unified Row[] the browser renders: it synthesizes
// folder rows from file paths and aggregates folder stats from descendants.
// Pure functions over protocol data; the reactive layer (nav) calls these
// inside a $derived. Home of the Row type.

import type {
  FileEntry,
  Target,
  TorrentDetail,
  TorrentState,
  TorrentSummary,
} from "@/daemon/protocol";
import { isComplete, isInitializing, isSeeding } from "@/daemon/summary";
import { fileId, folderId, torrentId, type RowKind } from "./ids";

// Aggregate persist/share state for rows that stand for many files.
export type Agg = "all" | "none" | "mixed";

// One unified state across kinds. Torrents use the full set; files and folders
// are never "initializing" but can be "queued" (selected, waiting their turn),
// which a whole torrent never is.
export type RowState = TorrentState | "queued";

export interface Row {
  id: string;
  kind: RowKind;
  name: string;
  state: RowState;
  progress: number; // 0..1
  size: number; // bytes, selected scope
  enterable: boolean; // can be navigated into (torrent, or folder)
  persisted: Agg; // file rows collapse to "all" / "none"
  shared: Agg;
  mixed: boolean; // incomplete descendants in differing activity states
  // torrent-only
  downloadSpeed?: number; // bytes/sec
  uploadSpeed?: number; // bytes/sec
  isSeeding?: boolean;
  isPaused?: boolean; // engine-level pause (file rows carry it as state)
  resolving?: boolean; // metadata not in yet: no name, no files
  checkProgress?: number; // 0..1 while the engine checks files
  error?: string; // why the torrent is in the error state
  addedAt?: string;
  remaining?: number; // bytes left of the selected set (drives ETA)
  // aggregate rows (torrent + folder)
  fileCount?: number;
  completeCount?: number;
  downloadingCount?: number; // folders: descendants currently downloading
  // file-only
  fileIndex?: number;
  selected?: boolean;
  pathHint?: string; // filter results: muted relative-path prefix
}

export function projectRoot(torrents: TorrentSummary[]): Row[] {
  return torrents.map(torrentRow);
}

// Project a torrent's media files into the folder + file rows visible at
// `path` ("" = torrent root, else "Season 1" / "Season 1/Extras"). Immediate
// files become file rows; anything under an immediate subfolder collapses
// into one folder row whose stats aggregate its whole subtree. Folders sort
// before files in source order; the active sort mode reorders on top.
export function projectFolder(detail: TorrentDetail, path: string): Row[] {
  const base = path === "" ? "" : `${path}/`;
  const subfolders = new Map<string, FileEntry[]>();
  const files: Row[] = [];
  for (const f of detail.files) {
    const fp = normalizePath(f.path);
    if (base !== "" && !fp.startsWith(base)) continue;
    const rel = fp.slice(base.length);
    const slash = rel.indexOf("/");
    if (slash === -1) {
      files.push(fileRow(detail.info_hash, f));
    } else {
      const name = rel.slice(0, slash);
      const group = subfolders.get(name);
      if (group) group.push(f);
      else subfolders.set(name, [f]);
    }
  }
  const folders: Row[] = [];
  for (const [name, descendants] of subfolders) {
    const folderPath = path === "" ? name : `${path}/${name}`;
    folders.push(folderRow(detail.info_hash, folderPath, name, descendants));
  }
  return [...folders, ...files];
}

export interface FilterEntry {
  row: Row;
  text: string;
}

// Flat candidate set for the fuzzy filter at the current scope: torrent names
// at root, every file under the current path (flattened) inside a torrent.
export function rootSource(torrents: TorrentSummary[]): FilterEntry[] {
  return projectRoot(torrents).map((row) => ({ row, text: row.name }));
}

export function folderSource(detail: TorrentDetail, path: string): FilterEntry[] {
  const base = path === "" ? "" : `${path}/`;
  const out: FilterEntry[] = [];
  for (const f of detail.files) {
    const fp = normalizePath(f.path);
    if (base !== "" && !fp.startsWith(base)) continue;
    const rel = fp.slice(base.length);
    const slash = rel.lastIndexOf("/");
    out.push({
      row: {
        ...fileRow(detail.info_hash, f),
        pathHint: slash === -1 ? undefined : rel.slice(0, slash + 1),
      },
      text: rel,
    });
  }
  return out;
}

// The file indices a target covers within a loaded detail: a file is itself, a
// folder is its subtree, a torrent is all its files. Mirrors the daemon's target
// expansion over the same media set, so an optimistic patch matches the result.
export function targetFiles(detail: TorrentDetail, target: Target): number[] {
  if (target.kind === "torrent") return detail.files.map((f) => f.index);
  if (target.kind === "file") return [target.file_index];
  const base = `${target.path}/`;
  return detail.files.filter((f) => normalizePath(f.path).startsWith(base)).map((f) => f.index);
}

function torrentRow(t: TorrentSummary): Row {
  return {
    id: torrentId(t.info_hash),
    kind: "torrent",
    name: t.name ?? shortHash(t.info_hash),
    state: t.state,
    // While the engine checks files the bar tracks the check; download
    // progress would sit at a meaningless 0 until it finishes.
    progress: isInitializing(t)
      ? (t.check_progress ?? 0)
      : t.total_bytes_selected === 0
        ? isComplete(t)
          ? 1
          : 0
        : clamp01(t.downloaded_bytes / t.total_bytes_selected),
    // Displayed size is the torrent's full media size; progress and ETA run
    // against the selected subset (which is 0 in the brief window between add
    // and auto-download selecting the files; size must not collapse to 0 then).
    size: t.total_bytes_all,
    // A single-media-file torrent is a directly-playable leaf, not a folder to
    // enter (file_count is media-only). Multi-file torrents open into their tree.
    enterable: t.file_count > 1,
    persisted: aggFromCount(t.persisted_count, t.file_count),
    shared: aggFromCount(t.shared_count, t.file_count),
    mixed: false, // per-state file counts aren't on the summary
    downloadSpeed: t.download_speed,
    uploadSpeed: t.upload_speed,
    isSeeding: isSeeding(t),
    isPaused: t.is_paused,
    resolving: isInitializing(t) && t.name === null,
    checkProgress: t.check_progress ?? undefined,
    error: t.error ?? undefined,
    addedAt: t.added_at,
    remaining: Math.max(0, t.total_bytes_selected - t.downloaded_bytes),
    fileCount: t.file_count,
    completeCount: t.complete_count,
  };
}

function fileRow(infoHash: string, f: FileEntry): Row {
  return {
    id: fileId(infoHash, f.index),
    kind: "file",
    name: basename(f.path),
    state: f.state,
    progress:
      f.size === 0 ? (f.state === "complete" ? 1 : 0) : clamp01(f.downloaded_bytes / f.size),
    size: f.size,
    enterable: false,
    persisted: f.persisted ? "all" : "none",
    shared: f.shared ? "all" : "none",
    mixed: false,
    fileIndex: f.index,
    selected: f.selected,
  };
}

function folderRow(infoHash: string, path: string, name: string, descendants: FileEntry[]): Row {
  let size = 0;
  let downloaded = 0;
  let hasError = false;
  let hasDownloading = false;
  let hasQueued = false;
  let hasPaused = false;
  let hasIdle = false;
  let allComplete = true;
  let completeCount = 0;
  let downloadingCount = 0;
  let anyPersisted = false;
  let anyUnpersisted = false;
  let anyShared = false;
  let anyUnshared = false;
  for (const f of descendants) {
    size += f.size;
    downloaded += Math.min(f.downloaded_bytes, f.size);
    if (f.state === "error") hasError = true;
    else if (f.state === "downloading") hasDownloading = true;
    else if (f.state === "queued") hasQueued = true;
    else if (f.state === "paused") hasPaused = true;
    else if (f.state === "idle") hasIdle = true;
    if (f.state === "complete") completeCount++;
    else allComplete = false;
    if (f.state === "downloading") downloadingCount++;
    if (f.persisted) anyPersisted = true;
    else anyUnpersisted = true;
    if (f.shared) anyShared = true;
    else anyUnshared = true;
  }
  // "mixed" = the folder's incomplete descendants span ≥2 distinct activity
  // states (downloading/queued/paused/idle), the single signal to surface
  // partial counts. NOT a wire state; the folder's primary `state` stays singular.
  const activityKinds =
    (hasDownloading ? 1 : 0) + (hasQueued ? 1 : 0) + (hasPaused ? 1 : 0) + (hasIdle ? 1 : 0);
  return {
    id: folderId(infoHash, path),
    kind: "folder",
    name,
    // Precedence: a single errored descendant dominates, then any active
    // download, then a queued (waiting) one, then paused, then idle, else
    // everything is complete.
    state: hasError
      ? "error"
      : hasDownloading
        ? "downloading"
        : hasQueued
          ? "queued"
          : hasPaused
            ? "paused"
            : hasIdle
              ? "idle"
              : "complete",
    progress: size === 0 ? (allComplete ? 1 : 0) : clamp01(downloaded / size),
    size,
    enterable: true,
    persisted: agg(anyPersisted, anyUnpersisted),
    shared: agg(anyShared, anyUnshared),
    mixed: !allComplete && activityKinds > 1,
    fileCount: descendants.length,
    completeCount,
    downloadingCount,
  };
}

function agg(any: boolean, anyNot: boolean): Agg {
  if (any && anyNot) return "mixed";
  return any ? "all" : "none";
}

function aggFromCount(n: number, total: number): Agg {
  if (total === 0 || n === 0) return "none";
  return n >= total ? "all" : "mixed";
}

function clamp01(x: number): number {
  return Math.max(0, Math.min(1, x));
}

function normalizePath(p: string): string {
  return p.replace(/\\/g, "/");
}

function basename(p: string): string {
  const n = normalizePath(p);
  const i = n.lastIndexOf("/");
  return i === -1 ? n : n.slice(i + 1);
}

function shortHash(h: string): string {
  return h.slice(0, 8);
}
