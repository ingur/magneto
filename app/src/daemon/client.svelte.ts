// Reconnecting WebSocket client for the magneto daemon control plane.
//
// One snapshot on connect, stats deltas merged into a torrent map keyed by
// info_hash, lifecycle events, request/response correlation, periodic ping, and
// exponential-backoff reconnect.

import type {
  Config,
  DaemonInfo,
  InfoHash,
  Outbound,
  ServerEvent,
  TorrentSummary,
} from "./protocol";

export type ConnStatus = "connecting" | "connected" | "reconnecting" | "disconnected";

const REQUEST_TIMEOUT = 15_000;
// add_torrent defers its reply until librqbit resolves magnet metadata, under
// a 120s daemon watchdog (ADD_TIMEOUT in commands.rs). Sized above it so the
// daemon's real success/failure always lands before the client gives up.
export const ADD_REQUEST_TIMEOUT = 125_000;
const PING_INTERVAL = 20_000;
const RECONNECT_BASE = 500;
const RECONNECT_MAX = 10_000;
// Force a stalled socket closed if the upgrade never completes, so a degraded
// daemon (accepts the TCP connection but never sends the 101) can't trap the
// client in CONNECTING with no close event.
const CONNECT_TIMEOUT = 10_000;

interface Pending {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

// What the resolver hands back on each dial: the control port and the token to
// authenticate against (null when reaching a daemon without token auth).
export interface ControlEndpoint {
  port: number;
  token: string | null;
}

// Build the control WebSocket URL, appending the auth token when present. The
// token is a hex string, but encode it anyway so a future format stays safe.
export function controlUrl(port: number, token: string | null): string {
  const base = `ws://127.0.0.1:${port}/ws`;
  return token ? `${base}?token=${encodeURIComponent(token)}` : base;
}

export class DaemonClient {
  status = $state<ConnStatus>("disconnected");
  daemon = $state<DaemonInfo | null>(null);
  config = $state<Config | null>(null);
  torrents = $state<Record<InfoHash, TorrentSummary>>({});
  lastError = $state<string | null>(null);

  #ws: WebSocket | null = null;
  #resolve: () => Promise<ControlEndpoint> = () =>
    Promise.reject(new Error("daemon client not started"));
  #opening = false;
  #seq = 0;
  #pending = new Map<string, Pending>();
  #attempts = 0;
  #reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  #pingTimer: ReturnType<typeof setInterval> | null = null;
  #intentional = false;
  #listeners = new Set<(event: ServerEvent) => void>();

  get torrentList(): TorrentSummary[] {
    return Object.values(this.torrents);
  }

  // Optimistically patch summary fields for one torrent, returning a revert that
  // restores the prior values. Like data.patchFiles: it captures the summary
  // object and only the patched keys, so a stats delta survives the revert and a
  // replaced summary (snapshot) makes the revert a harmless no-op on stale data.
  patchSummary(infoHash: InfoHash, patch: Partial<TorrentSummary>): () => void {
    const current = this.torrents[infoHash] as unknown as Record<string, unknown> | undefined;
    if (!current) return () => {};
    const before: Record<string, unknown> = {};
    for (const k of Object.keys(patch)) before[k] = current[k];
    Object.assign(current, patch);
    return () => Object.assign(current, before);
  }

  /**
   * Start, and keep, a connection. `resolve` is asked for the control endpoint
   * (port + token) on every dial, so a reconnect re-runs daemon discovery/spawn
   * rather than redialing a cached endpoint that may have changed or gone away.
   * A failing first attempt is not terminal: it falls into the same backoff loop
   * as a drop.
   */
  connect(resolve: () => Promise<ControlEndpoint>): void {
    this.#resolve = resolve;
    // Already live: keep the socket and its ping, refresh only the resolver
    // for future redials. Clearing timers here would kill the keepalive while
    // #open() declines to dial over an open socket.
    if (this.#ws?.readyState === WebSocket.OPEN) return;
    this.#intentional = false;
    this.#attempts = 0;
    // Cancel any reconnect left over from a prior lifecycle so this dial can't
    // race a stale timer into a second socket.
    this.#clearTimers();
    void this.#open();
  }

  disconnect(): void {
    this.#intentional = true;
    this.#clearTimers();
    this.#ws?.close();
    this.#ws = null;
    this.status = "disconnected";
  }

  /** Send a command and resolve with its `result` (or reject on error/timeout). */
  request<T = unknown>(
    type: string,
    payload: unknown = {},
    timeoutMs = REQUEST_TIMEOUT,
  ): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const ws = this.#ws;
      if (!ws || ws.readyState !== WebSocket.OPEN) {
        reject(new Error("daemon not connected"));
        return;
      }
      const id = String(++this.#seq);
      const timer = setTimeout(() => {
        this.#pending.delete(id);
        reject(new Error(`request "${type}" timed out`));
      }, timeoutMs);
      this.#pending.set(id, { resolve: resolve as Pending["resolve"], reject, timer });
      ws.send(JSON.stringify({ type, id, payload }));
    });
  }

  /**
   * Subscribe to server events (lifecycle toasts, etc.). Returns an
   * unsubscribe function. Listeners fire after the client's own state
   * update for each event; the shell maps the events it cares about to
   * toasts. Keeps the protocol-agnostic feedback layer out of the client.
   */
  onEvent(listener: (event: ServerEvent) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  async #open(): Promise<void> {
    // Guard the async window so only one resolve-then-dial runs at a time, and
    // never open a second socket over a live one (a stray connect()/reconnect
    // overlap); that would orphan the first with its handlers still attached.
    if (this.#opening) return;
    if (this.#ws && this.#ws.readyState !== WebSocket.CLOSED) return;
    this.#opening = true;
    this.status = this.#attempts > 0 ? "reconnecting" : "connecting";

    let endpoint: ControlEndpoint;
    try {
      endpoint = await this.#resolve();
    } catch (err) {
      // The host could not hand us a port (daemon down, spawn failed). Treat it
      // like a dropped socket: record it and retry with backoff.
      this.lastError = err instanceof Error ? err.message : String(err);
      this.#opening = false;
      if (!this.#intentional) {
        this.status = "reconnecting";
        this.#scheduleReconnect();
      }
      return;
    }

    // disconnect() may have run while the resolver was in flight.
    if (this.#intentional) {
      this.#opening = false;
      return;
    }

    const ws = new WebSocket(controlUrl(endpoint.port, endpoint.token));
    this.#ws = ws;
    this.#opening = false;

    const connectGuard = setTimeout(() => {
      if (ws.readyState !== WebSocket.OPEN) ws.close();
    }, CONNECT_TIMEOUT);

    ws.onopen = () => {
      clearTimeout(connectGuard);
      this.#attempts = 0;
      this.lastError = null;
      this.status = "connected";
      this.#startPing();
    };
    ws.onmessage = (ev) => this.#onMessage(ev.data);
    ws.onerror = () => {
      // The close handler drives reconnect; just record the symptom.
      this.lastError = "websocket error";
    };
    ws.onclose = () => {
      clearTimeout(connectGuard);
      this.#ws = null;
      this.#clearTimers();
      this.#rejectAllPending(new Error("connection closed"));
      if (this.#intentional) {
        this.status = "disconnected";
      } else {
        this.status = "reconnecting";
        this.#scheduleReconnect();
      }
    };
  }

  #onMessage(data: unknown): void {
    if (typeof data !== "string") return;
    let msg: Outbound;
    try {
      msg = JSON.parse(data) as Outbound;
    } catch {
      return;
    }
    if (msg.type === "response") {
      this.#settle(msg.id, () => this.#pending.get(msg.id)?.resolve(msg.result));
      return;
    }
    if (msg.type === "error") {
      this.#settle(msg.id, () => this.#pending.get(msg.id)?.reject(new Error(msg.error)));
      return;
    }
    this.#handleEvent(msg);
  }

  #settle(id: string, run: () => void): void {
    const pending = this.#pending.get(id);
    if (!pending) return;
    clearTimeout(pending.timer);
    run();
    this.#pending.delete(id);
  }

  #handleEvent(event: ServerEvent): void {
    switch (event.type) {
      case "snapshot": {
        this.daemon = event.daemon;
        this.config = event.config;
        const next: Record<InfoHash, TorrentSummary> = {};
        for (const t of event.torrents) next[t.info_hash] = t;
        this.torrents = next;
        break;
      }
      case "stats": {
        for (const delta of event.torrents) {
          const current = this.torrents[delta.info_hash];
          if (current) Object.assign(current, delta);
        }
        break;
      }
      case "torrent_added": {
        if (!this.torrents[event.info_hash]) {
          this.torrents[event.info_hash] = placeholderSummary(event.info_hash, event.state);
        }
        break;
      }
      case "torrent_ready": {
        const { type: _type, files: _files, ...summary } = event;
        this.torrents[event.info_hash] = summary;
        break;
      }
      case "torrent_complete": {
        const current = this.torrents[event.info_hash];
        const wasComplete = current?.state === "complete";
        if (current) current.state = "complete";
        // Skip relaying a re-emit: the daemon re-broadcasts complete for every
        // already-complete torrent on restart, and a reconnect snapshot already
        // marks them complete; only a real incomplete→complete edge should toast.
        if (wasComplete) return;
        break;
      }
      case "torrent_removed": {
        delete this.torrents[event.info_hash];
        break;
      }
      case "torrent_error": {
        const current = this.torrents[event.info_hash];
        if (current) current.state = "error";
        break;
      }
      case "config_changed": {
        this.config = event.config;
        break;
      }
      case "daemon_restarting":
      case "daemon_shutdown": {
        // The daemon is about to exit; the socket will close and reconnect.
        this.status = "reconnecting";
        break;
      }
    }

    // Relay every event to subscribers AFTER the internal state update, so
    // a listener (the shell mapping lifecycle events to toasts) sees the
    // torrents map already current. A throwing listener is logged, never
    // allowed to abort the relay or escape the message handler.
    for (const listener of this.#listeners) {
      try {
        listener(event);
      } catch (err) {
        console.error("daemon: event listener threw", err);
      }
    }
  }

  #startPing(): void {
    this.#pingTimer = setInterval(() => {
      this.request("ping").catch(() => this.#ws?.close());
    }, PING_INTERVAL);
  }

  #scheduleReconnect(): void {
    const ceiling = Math.min(RECONNECT_BASE * 1.5 ** this.#attempts, RECONNECT_MAX);
    // Jitter over the upper half of the window so many attempts (or many
    // clients) don't all redial on the same tick.
    const delay = ceiling * (0.5 + Math.random() * 0.5);
    this.#attempts += 1;
    this.#reconnectTimer = setTimeout(() => void this.#open(), delay);
  }

  #clearTimers(): void {
    if (this.#pingTimer) clearInterval(this.#pingTimer);
    if (this.#reconnectTimer) clearTimeout(this.#reconnectTimer);
    this.#pingTimer = null;
    this.#reconnectTimer = null;
  }

  #rejectAllPending(reason: Error): void {
    for (const pending of this.#pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(reason);
    }
    this.#pending.clear();
  }
}

function placeholderSummary(infoHash: InfoHash, state: TorrentSummary["state"]): TorrentSummary {
  return {
    info_hash: infoHash,
    name: null,
    source: null,
    source_kind: null,
    state,
    total_bytes_all: 0,
    total_bytes_selected: 0,
    downloaded_bytes: 0,
    download_speed: 0,
    upload_speed: 0,
    file_count: 0,
    complete_count: 0,
    selected_count: 0,
    persisted_count: 0,
    shared_count: 0,
    is_initializing: state === "initializing",
    is_complete: false,
    is_seeding: false,
    is_paused: false,
    // Real timestamp (not "") so the "added" sort places a just-added torrent
    // correctly instead of treating it as undated; the daemon's added_at
    // replaces it on the next snapshot/torrent_ready.
    added_at: new Date().toISOString(),
  };
}

export const daemon = new DaemonClient();
