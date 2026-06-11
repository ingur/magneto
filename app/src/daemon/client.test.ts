import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TorrentSummary } from "@/daemon/protocol";
import { controlUrl, DaemonClient } from "./client.svelte";

// jsdom has no WebSocket, and these tests exercise the reconnect/re-resolve
// logic rather than real sockets, so we stand one in and drive it by hand.
class MockSocket {
  static instances: MockSocket[] = [];
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;

  url: string;
  readyState = 0; // CONNECTING
  onopen: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];

  constructor(url: string) {
    this.url = url;
    MockSocket.instances.push(this);
  }
  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    if (this.readyState === 3) return;
    this.readyState = 3; // CLOSED
    this.onclose?.();
  }
  /** Test helper: complete the upgrade. */
  accept(): void {
    this.readyState = 1; // OPEN
    this.onopen?.();
  }
}

beforeEach(() => {
  vi.useFakeTimers();
  MockSocket.instances = [];
  vi.stubGlobal("WebSocket", MockSocket);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const RECONNECT_MAX = 10_000;
const CONNECT_TIMEOUT = 10_000;

describe("DaemonClient recovery", () => {
  it("keeps retrying when the port resolver rejects (daemon down at launch)", async () => {
    const resolve = vi.fn().mockRejectedValue(new Error("daemon not ready"));
    const client = new DaemonClient();

    client.connect(resolve);
    await vi.advanceTimersByTimeAsync(0); // let the first resolve settle

    expect(client.status).toBe("reconnecting");
    expect(client.lastError).toBe("daemon not ready");
    expect(MockSocket.instances).toHaveLength(0); // never dialed a dead port

    // The loop must keep going: a later attempt re-invokes the resolver.
    await vi.advanceTimersByTimeAsync(RECONNECT_MAX);
    expect(resolve.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("re-resolves the port on every dial, not just the first", async () => {
    const resolve = vi.fn().mockResolvedValue({ port: 61481, token: null });
    const client = new DaemonClient();

    client.connect(resolve);
    await vi.advanceTimersByTimeAsync(0);
    expect(MockSocket.instances).toHaveLength(1);
    MockSocket.instances[0].accept();
    expect(client.status).toBe("connected");

    // Drop the socket: the client must dial again AND ask the resolver again,
    // so a daemon that moved or was respawned is picked up.
    MockSocket.instances[0].close();
    expect(client.status).toBe("reconnecting");
    await vi.advanceTimersByTimeAsync(RECONNECT_MAX);

    expect(resolve.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(MockSocket.instances.length).toBeGreaterThanOrEqual(2);
  });

  it("force-closes a socket that never completes the upgrade", async () => {
    const resolve = vi.fn().mockResolvedValue({ port: 61481, token: null });
    const client = new DaemonClient();

    client.connect(resolve);
    await vi.advanceTimersByTimeAsync(0);
    const ws = MockSocket.instances[0];
    expect(ws.readyState).toBe(0); // still CONNECTING

    // It never accepts; the connect guard must close it after the timeout so
    // the reconnect path takes over instead of hanging forever.
    await vi.advanceTimersByTimeAsync(CONNECT_TIMEOUT);
    expect(ws.readyState).toBe(3); // CLOSED by the guard
    expect(client.status).toBe("reconnecting");
  });

  it("stops cleanly on disconnect: no reconnect, no further dials", async () => {
    const resolve = vi.fn().mockResolvedValue({ port: 61481, token: null });
    const client = new DaemonClient();

    client.connect(resolve);
    await vi.advanceTimersByTimeAsync(0);
    MockSocket.instances[0].accept();
    expect(client.status).toBe("connected");

    client.disconnect();
    expect(client.status).toBe("disconnected");

    const calls = resolve.mock.calls.length;
    await vi.advanceTimersByTimeAsync(RECONNECT_MAX);
    expect(resolve.mock.calls.length).toBe(calls); // intentional close: no retry
  });

  it("does not open a second socket when connect() runs while already live", async () => {
    const resolve = vi.fn().mockResolvedValue({ port: 61481, token: null });
    const client = new DaemonClient();

    client.connect(resolve);
    await vi.advanceTimersByTimeAsync(0);
    MockSocket.instances[0].accept();
    expect(client.status).toBe("connected");

    // A stray reconnect/connect overlap must not orphan the live socket with a
    // second one whose handlers fight the first.
    client.connect(resolve);
    await vi.advanceTimersByTimeAsync(0);
    expect(MockSocket.instances).toHaveLength(1);
  });

  it("dials the token onto the control URL when the resolver supplies one", async () => {
    const resolve = vi.fn().mockResolvedValue({ port: 61481, token: "deadbeef" });
    const client = new DaemonClient();

    client.connect(resolve);
    await vi.advanceTimersByTimeAsync(0);

    expect(MockSocket.instances).toHaveLength(1);
    expect(MockSocket.instances[0].url).toBe("ws://127.0.0.1:61481/ws?token=deadbeef");
  });
});

describe("controlUrl", () => {
  it("omits the query when there is no token", () => {
    expect(controlUrl(61481, null)).toBe("ws://127.0.0.1:61481/ws");
  });

  it("appends and encodes the token", () => {
    expect(controlUrl(61481, "ab+cd/ef")).toBe("ws://127.0.0.1:61481/ws?token=ab%2Bcd%2Fef");
  });
});

describe("DaemonClient.patchSummary", () => {
  function summary(over: Partial<TorrentSummary> = {}): TorrentSummary {
    return {
      info_hash: "h1",
      file_count: 3,
      persisted_count: 0,
      shared_count: 0,
      ...over,
    } as unknown as TorrentSummary;
  }

  it("patches a summary field and reverts to the prior value", () => {
    const client = new DaemonClient();
    client.torrents = { h1: summary() };
    const revert = client.patchSummary("h1", { persisted_count: 3 });
    expect(client.torrents.h1.persisted_count).toBe(3);
    revert();
    expect(client.torrents.h1.persisted_count).toBe(0);
  });

  it("revert is a no-op after the summary is replaced", () => {
    const client = new DaemonClient();
    client.torrents = { h1: summary({ persisted_count: 0 }) };
    const revert = client.patchSummary("h1", { persisted_count: 3 });
    client.torrents = { h1: summary({ persisted_count: 3 }) }; // authoritative refresh
    revert();
    expect(client.torrents.h1.persisted_count).toBe(3);
  });
});
