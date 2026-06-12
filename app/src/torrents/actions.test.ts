import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { daemon } from "@/daemon/client.svelte";
import type { TorrentSummary } from "@/daemon/protocol";
import { toast } from "@/lib/feedback/toasts/toasts.svelte";

import * as actions from "./actions";
import { fileId, torrentId } from "./ids";
import { nav } from "./nav.svelte";
import type { Row } from "./projection";

function row(id: string, over: Partial<Row> = {}): Row {
  return {
    id,
    kind: "torrent",
    name: "x",
    state: "complete",
    progress: 1,
    size: 0,
    enterable: true,
    persisted: "none",
    shared: "none",
    mixed: false,
    ...over,
  };
}

beforeEach(() => {
  nav.pathIds = [];
  nav.clearSelection();
  vi.spyOn(daemon, "request").mockResolvedValue({} as never);
  vi.spyOn(toast, "info").mockReturnValue(0);
  vi.spyOn(toast, "success").mockReturnValue(0);
  vi.spyOn(toast, "warn").mockReturnValue(0);
  vi.spyOn(toast, "error").mockReturnValue(0);
});

afterEach(() => vi.restoreAllMocks());

describe("targeting + bulk flag", () => {
  it("acting on an unmarked row keeps an unrelated selection (regression)", () => {
    nav.markAll(["A", "B"]);
    const t = actions.rowTargets(row("C"), false);
    expect(t.ids).toEqual(["C"]);
    expect(t.bulk).toBe(false);
  });

  it("acting on a marked row targets the whole selection", () => {
    nav.markAll(["A", "B"]);
    const t = actions.rowTargets(row("A"), true);
    expect(new Set(t.ids)).toEqual(new Set(["A", "B"]));
    expect(t.bulk).toBe(true);
    expect(t.subject).toBe("selection");
  });

  it("no selection targets just the row, subject = kind", () => {
    const t = actions.rowTargets(row("C", { kind: "file" }), false);
    expect(t.ids).toEqual(["C"]);
    expect(t.bulk).toBe(false);
    expect(t.subject).toBe("file");
  });

  it("resolveTargets marks a selection as bulk", () => {
    nav.markAll(["A", "B"]);
    const t = actions.resolveTargets();
    expect(new Set(t.ids)).toEqual(new Set(["A", "B"]));
    expect(t.bulk).toBe(true);
  });

  it("resolveTargets with no selection and no cursor is empty, not bulk", () => {
    const t = actions.resolveTargets();
    expect(t.ids).toEqual([]);
    expect(t.bulk).toBe(false);
  });
});

describe("run* orchestration", () => {
  it("keeps the selection after non-destructive bulk actions (chaining)", async () => {
    const [a, b, c] = [torrentId("a"), torrentId("b"), torrentId("c")];

    nav.markAll([a, b]);
    await actions.runTogglePersist({
      ids: [a, b],
      rows: [],
      leader: row(a),
      subject: "selection",
      bulk: true,
    });
    expect(nav.selection.size).toBe(2); // still marked, persist can chain into share/download

    await actions.runToggleDownload({
      ids: [a, b],
      rows: [row(a, { state: "paused" }), row(b, { state: "paused" })],
      leader: row(a, { state: "paused" }),
      subject: "selection",
      bulk: true,
    });
    expect(nav.selection.size).toBe(2);

    await actions.runTogglePersist({
      ids: [c],
      rows: [],
      leader: row(c),
      subject: "torrent",
      bulk: false,
    });
    expect(nav.selection.size).toBe(2); // untouched, acted on a non-selected row
  });

  it("toggle-download flips only targets already in the matching direction", async () => {
    const [a, b] = [torrentId("a"), torrentId("b")];
    const rows = [row(a, { state: "downloading" }), row(b, { state: "paused" })];
    await actions.runToggleDownload({
      ids: [a, b],
      rows,
      leader: rows[0],
      subject: "selection",
      bulk: true,
    });

    const pauseCall = vi.mocked(daemon.request).mock.calls.find((c) => c[0] === "pause");
    expect(pauseCall).toBeDefined();
    expect(pauseCall![1]).toEqual({ targets: [{ kind: "torrent", info_hash: "a" }] });
  });

  it("pauses a queued file (already wanted) rather than treating it as a resume", async () => {
    const [a, b] = [torrentId("a"), torrentId("b")];
    const rows = [row(a, { state: "queued" }), row(b, { state: "idle" })];
    await actions.runToggleDownload({
      ids: [a, b],
      rows,
      leader: rows[0],
      subject: "selection",
      bulk: true,
    });

    const calls = vi.mocked(daemon.request).mock.calls;
    const pauseCall = calls.find((c) => c[0] === "pause");
    expect(pauseCall).toBeDefined();
    // The queued row is paused; the idle row is the opposite direction, excluded.
    expect(pauseCall![1]).toEqual({ targets: [{ kind: "torrent", info_hash: "a" }] });
    expect(calls.find((c) => c[0] === "resume")).toBeUndefined();
  });

  it("stop wins: a downloading row in the selection pauses regardless of the cursor row", async () => {
    const [a, b] = [torrentId("a"), torrentId("b")];
    const rows = [row(a, { state: "paused" }), row(b, { state: "downloading" })];
    await actions.runToggleDownload({
      ids: [a, b],
      rows,
      leader: rows[0],
      subject: "selection",
      bulk: true,
    });

    const calls = vi.mocked(daemon.request).mock.calls;
    const pauseCall = calls.find((c) => c[0] === "pause");
    expect(pauseCall).toBeDefined();
    expect(pauseCall![1]).toEqual({ targets: [{ kind: "torrent", info_hash: "b" }] });
    expect(calls.find((c) => c[0] === "resume")).toBeUndefined();
  });

  it("complete torrent with unselected media resumes as download-remaining", async () => {
    const [a, b] = [torrentId("a"), torrentId("b")];
    const rows = [
      row(a, { state: "complete", completeCount: 1, fileCount: 10 }),
      row(b, { state: "complete", completeCount: 10, fileCount: 10 }),
    ];
    await actions.runToggleDownload({
      ids: [a, b],
      rows,
      leader: rows[0],
      subject: "selection",
      bulk: true,
    });

    const resumeCall = vi.mocked(daemon.request).mock.calls.find((c) => c[0] === "resume");
    expect(resumeCall).toBeDefined();
    // Only the partially-downloaded torrent qualifies; the fully-complete one
    // has nothing left to start.
    expect(resumeCall![1]).toEqual({ targets: [{ kind: "torrent", info_hash: "a" }] });
  });

  it("fully-complete leader stays a no-op", async () => {
    const a = torrentId("a");
    const rows = [row(a, { state: "complete", completeCount: 5, fileCount: 5 })];
    await actions.runToggleDownload({
      ids: [a],
      rows,
      leader: rows[0],
      subject: "torrent",
      bulk: false,
    });
    expect(vi.mocked(daemon.request).mock.calls.find((c) => c[0] === "resume")).toBeUndefined();
    expect(vi.mocked(daemon.request).mock.calls.find((c) => c[0] === "pause")).toBeUndefined();
  });

  it("resumes an errored torrent (retry) alongside paused/idle targets", async () => {
    const [a, b] = [torrentId("a"), torrentId("b")];
    const rows = [row(a, { state: "error" }), row(b, { state: "paused" })];
    await actions.runToggleDownload({
      ids: [a, b],
      rows,
      leader: rows[0],
      subject: "selection",
      bulk: true,
    });

    const resumeCall = vi.mocked(daemon.request).mock.calls.find((c) => c[0] === "resume");
    expect(resumeCall).toBeDefined();
    expect(resumeCall![1]).toEqual({
      targets: [
        { kind: "torrent", info_hash: "a" },
        { kind: "torrent", info_hash: "b" },
      ],
    });
  });
});

describe("ready gate", () => {
  it("gates file targets of a not-ready torrent and warns instead of a silent no-op", async () => {
    const f = fileId("x", 0);
    await actions.runPlay({
      ids: [f],
      rows: [],
      leader: row(f, { kind: "file" }),
      subject: "file",
      bulk: false,
    });
    expect(daemon.request).not.toHaveBeenCalled();
    expect(toast.warn).toHaveBeenCalled();
  });
});

describe("buildMagnet", () => {
  const summary = (over: Partial<TorrentSummary>): TorrentSummary => ({
    info_hash: "abc",
    name: "Movie",
    source: null,
    source_kind: null,
    state: "complete",
    total_bytes_all: 0,
    total_bytes_selected: 0,
    downloaded_bytes: 0,
    download_speed: 0,
    upload_speed: 0,
    file_count: 1,
    complete_count: 1,
    selected_count: 1,
    persisted_count: 0,
    shared_count: 0,
    is_initializing: false,
    is_complete: true,
    is_seeding: false,
    is_paused: false,
    added_at: "",
    ...over,
  });

  it("reuses the original magnet (trackers and all)", () => {
    const s = summary({ source_kind: "magnet", source: "magnet:?xt=urn:btih:xyz&tr=udp://t" });
    expect(actions.buildMagnet(s)).toBe("magnet:?xt=urn:btih:xyz&tr=udp://t");
  });

  it("builds a magnet from the info hash + name for non-magnet adds", () => {
    const s = summary({ source_kind: "file", info_hash: "abc", name: "My Movie" });
    expect(actions.buildMagnet(s)).toBe("magnet:?xt=urn:btih:abc&dn=My%20Movie");
  });

  it("omits dn when there's no name", () => {
    const s = summary({ source_kind: "url", info_hash: "abc", name: null });
    expect(actions.buildMagnet(s)).toBe("magnet:?xt=urn:btih:abc");
  });
});
