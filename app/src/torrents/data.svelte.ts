// Per-torrent file-detail cache. The daemon client owns the torrent summary
// map (daemon.torrents); this owns the FileEntry[] for torrents the user has
// opened, loaded lazily via get_torrent. The daemon emits no events for
// persist/share/pause/resume, so callers refetch after those; per-file stats
// deltas and lifecycle events keep an open torrent's files live in between.

import { daemon } from "@/daemon/client.svelte";
import type {
  FileEntry,
  FileStatsDelta,
  InfoHash,
  ServerEvent,
  TorrentDetail,
} from "@/daemon/protocol";

// The per-file flags an optimistic action flips locally before the daemon confirms.
export type FilePatch = Partial<Pick<FileEntry, "persisted" | "shared" | "selected">>;

export class TorrentsData {
  #details = $state<Record<InfoHash, TorrentDetail>>({});

  constructor() {
    // App-lifetime singleton: subscribe once and never tear down. Fires
    // after the client's own state update (see DaemonClient.onEvent).
    daemon.onEvent((e) => this.#apply(e));
  }

  detail(infoHash: InfoHash): TorrentDetail | undefined {
    return this.#details[infoHash];
  }

  // A torrent is ready for file-level ops once its detail is loaded and it is
  // not (re)initializing. The daemon emits no event for persist/share/pause/
  // resume, so file/folder targets are gated on this (see actions.gateReady).
  ready(infoHash: InfoHash): boolean {
    const summary = daemon.torrents[infoHash];
    return (
      summary !== undefined && !summary.is_initializing && this.#details[infoHash] !== undefined
    );
  }

  // Fetch (or refetch) a torrent's detail and cache it. Called on
  // torrent-open and after persist/share/pause/resume.
  async load(infoHash: InfoHash): Promise<TorrentDetail> {
    const detail = await daemon.request<TorrentDetail>("get_torrent", { info_hash: infoHash });
    this.#details[infoHash] = detail;
    return detail;
  }

  // Optimistically patch fields on specific files of an open torrent, returning
  // a revert that restores the prior values. The revert mutates the captured
  // FileEntry objects and only the patched keys, so a concurrent stats delta
  // survives it, and if the detail was replaced (load / torrent_ready /
  // snapshot) the revert harmlessly touches orphaned objects instead of
  // overwriting the fresh authoritative data.
  patchFiles(infoHash: InfoHash, indices: Set<number>, patch: FilePatch): () => void {
    const files = this.#details[infoHash]?.files;
    if (!files) return () => {};
    const keys = Object.keys(patch) as (keyof FilePatch)[];
    const reverts: Array<() => void> = [];
    for (const f of files) {
      if (!indices.has(f.index)) continue;
      const before: FilePatch = {};
      for (const k of keys) before[k] = f[k];
      Object.assign(f, patch);
      reverts.push(() => Object.assign(f, before));
    }
    return () => reverts.forEach((r) => r());
  }

  #apply(event: ServerEvent): void {
    switch (event.type) {
      case "stats": {
        for (const delta of event.files) this.#applyFileDelta(delta);
        break;
      }
      case "torrent_ready": {
        // An open torrent (re)resolved its files: replace the cache so
        // rows pick up the new file list. Ignore torrents we haven't opened.
        if (this.#details[event.info_hash]) {
          const { type: _type, ...detail } = event;
          this.#details[event.info_hash] = detail;
        }
        break;
      }
      case "torrent_removed": {
        delete this.#details[event.info_hash];
        break;
      }
      case "snapshot": {
        // Reconnect/restart invalidates every cached file list; reopened on
        // demand against the fresh daemon.
        this.#details = {};
        break;
      }
    }
  }

  #applyFileDelta(delta: FileStatsDelta): void {
    const file = this.#details[delta.info_hash]?.files.find((f) => f.index === delta.file_index);
    if (file) {
      file.downloaded_bytes = delta.downloaded_bytes;
      file.state = delta.state;
    }
  }
}

export const data = new TorrentsData();
