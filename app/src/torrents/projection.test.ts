import { describe, expect, it } from "vitest";

import type { FileEntry, FileState, TorrentDetail, TorrentSummary } from "@/daemon/protocol";
import { projectFolder, projectRoot } from "./projection";

function summary(over: Partial<TorrentSummary> = {}): TorrentSummary {
  return {
    info_hash: "h1",
    name: "Movie",
    source: "",
    source_kind: "magnet",
    state: "downloading",
    error: null,
    check_progress: null,
    total_bytes_all: 100,
    total_bytes_selected: 100,
    downloaded_bytes: 50,
    download_speed: 0,
    upload_speed: 0,
    file_count: 2,
    complete_count: 1,
    selected_count: 2,
    persisted_count: 0,
    shared_count: 0,
    is_paused: false,
    added_at: "2026-06-01T00:00:00Z",
    ...over,
  };
}

function file(
  index: number,
  path: string,
  state: FileState,
  over: Partial<FileEntry> = {},
): FileEntry {
  const complete = state === "complete";
  return {
    index,
    path,
    size: 100,
    downloaded_bytes: complete ? 100 : state === "downloading" ? 50 : 0,
    selected: state !== "idle",
    state,
    persisted: false,
    shared: false,
    ...over,
  };
}

function detail(files: FileEntry[], over: Partial<TorrentSummary> = {}): TorrentDetail {
  return { ...summary(over), files };
}

describe("projectRoot", () => {
  it("maps a summary to a torrent row", () => {
    const [row] = projectRoot([summary()]);
    expect(row.id).toBe("t:h1");
    expect(row.kind).toBe("torrent");
    expect(row.name).toBe("Movie");
    expect(row.progress).toBeCloseTo(0.5);
    expect(row.size).toBe(100);
  });

  it("falls back to a short info_hash when the name is null", () => {
    const [row] = projectRoot([summary({ name: null, info_hash: "abcdef0123456789" })]);
    expect(row.name).toBe("abcdef01");
  });

  it("derives persist aggregate from the count", () => {
    expect(projectRoot([summary({ persisted_count: 0, file_count: 2 })])[0].persisted).toBe("none");
    expect(projectRoot([summary({ persisted_count: 2, file_count: 2 })])[0].persisted).toBe("all");
    expect(projectRoot([summary({ persisted_count: 1, file_count: 2 })])[0].persisted).toBe(
      "mixed",
    );
  });

  it("renders a single-media-file torrent as a playable leaf (not enterable)", () => {
    expect(projectRoot([summary({ file_count: 1 })])[0].enterable).toBe(false);
    expect(projectRoot([summary({ file_count: 2 })])[0].enterable).toBe(true);
  });

  it("shows the torrent's full media size, not the selected subset", () => {
    // Right after add the files aren't selected yet; size must still be the
    // real total (else a torrent flashes 0 B), and remaining tracks selected.
    const [row] = projectRoot([
      summary({ total_bytes_all: 1500, total_bytes_selected: 0, downloaded_bytes: 0 }),
    ]);
    expect(row.size).toBe(1500);
    expect(row.remaining).toBe(0);
  });

  it("computes remaining bytes of the selected set for ETA", () => {
    const [row] = projectRoot([
      summary({ total_bytes_all: 1000, total_bytes_selected: 800, downloaded_bytes: 300 }),
    ]);
    expect(row.size).toBe(1000);
    expect(row.remaining).toBe(500);
  });

  it("derives seeding and completion from state, never from the wire", () => {
    const complete = summary({ state: "complete", total_bytes_selected: 0 });
    expect(projectRoot([complete])[0].isSeeding).toBe(true);
    expect(projectRoot([complete])[0].progress).toBe(1);
    expect(projectRoot([summary({ state: "complete", is_paused: true })])[0].isSeeding).toBe(false);
    expect(projectRoot([summary({ state: "downloading" })])[0].isSeeding).toBe(false);
    expect(projectRoot([summary({ state: "idle", total_bytes_selected: 0 })])[0].progress).toBe(0);
  });

  it("tracks a file check on an initializing row and flags unresolved metadata", () => {
    const checking = summary({ state: "initializing", check_progress: 0.4, downloaded_bytes: 0 });
    const [row] = projectRoot([checking]);
    expect(row.progress).toBeCloseTo(0.4);
    expect(row.checkProgress).toBeCloseTo(0.4);
    expect(row.resolving).toBe(false);

    const [queued] = projectRoot([summary({ state: "initializing", downloaded_bytes: 0 })]);
    expect(queued.progress).toBe(0);
    expect(queued.checkProgress).toBeUndefined();

    const [resolving] = projectRoot([summary({ state: "initializing", name: null })]);
    expect(resolving.resolving).toBe(true);
    // A metadata fetch that failed is an errored row, not one still resolving.
    const [failed] = projectRoot([summary({ state: "error", name: null, error: "no peers" })]);
    expect(failed.resolving).toBe(false);
  });

  it("carries the engine's error text", () => {
    expect(projectRoot([summary({ state: "error", error: "disk full" })])[0].error).toBe(
      "disk full",
    );
    expect(projectRoot([summary()])[0].error).toBeUndefined();
  });
});

describe("projectFolder", () => {
  const files = [
    file(0, "intro.mkv", "complete"),
    file(1, "Season 1/e1.mkv", "downloading"),
    file(2, "Season 1/e2.mkv", "idle"),
  ];

  it("splits the root into folder rows + immediate file rows (folders first)", () => {
    const rows = projectFolder(detail(files), "");
    expect(rows.map((r) => [r.kind, r.name])).toEqual([
      ["folder", "Season 1"],
      ["file", "intro.mkv"],
    ]);
    expect(rows[0].id).toBe("d:h1:Season 1");
    expect(rows[1].id).toBe("f:h1:0");
  });

  it("aggregates folder size/downloaded/progress from descendants", () => {
    const folder = projectFolder(detail(files), "")[0];
    expect(folder.size).toBe(200);
    expect(folder.progress).toBeCloseTo(0.25); // (50 + 0) / 200
  });

  it("derives folder state by precedence and flags mixed activity", () => {
    const folder = projectFolder(detail(files), "")[0];
    expect(folder.state).toBe("downloading"); // a downloading descendant wins
    expect(folder.mixed).toBe(true); // downloading + idle coexist
  });

  it("evaluates error first in folder state precedence", () => {
    const withError = [file(0, "S/a.mkv", "downloading"), file(1, "S/b.mkv", "error")];
    expect(projectFolder(detail(withError), "")[0].state).toBe("error");
  });

  it("ranks a downloading descendant above a queued one", () => {
    const mix = [file(0, "S/a.mkv", "queued"), file(1, "S/b.mkv", "downloading")];
    const folder = projectFolder(detail(mix), "")[0];
    expect(folder.state).toBe("downloading"); // something is actually moving
    expect(folder.mixed).toBe(true); // downloading + queued coexist
  });

  it("reports a queued folder when descendants are selected but none is moving", () => {
    const waiting = [file(0, "S/a.mkv", "queued"), file(1, "S/b.mkv", "queued")];
    const folder = projectFolder(detail(waiting), "")[0];
    expect(folder.state).toBe("queued");
    expect(folder.mixed).toBe(false); // one activity kind, not mixed
  });

  it("reports a complete folder when all descendants are complete", () => {
    const done = [file(0, "S/a.mkv", "complete"), file(1, "S/b.mkv", "complete")];
    const folder = projectFolder(detail(done), "")[0];
    expect(folder.state).toBe("complete");
    expect(folder.progress).toBe(1);
    expect(folder.mixed).toBe(false);
  });

  it("aggregates persist state across descendants", () => {
    const mixed = [
      file(0, "S/a.mkv", "idle", { persisted: true }),
      file(1, "S/b.mkv", "idle", { persisted: false }),
    ];
    expect(projectFolder(detail(mixed), "")[0].persisted).toBe("mixed");
    const all = [
      file(0, "S/a.mkv", "idle", { persisted: true }),
      file(1, "S/b.mkv", "idle", { persisted: true }),
    ];
    expect(projectFolder(detail(all), "")[0].persisted).toBe("all");
  });

  it("projects a subfolder path to its files (relative basenames)", () => {
    const rows = projectFolder(detail(files), "Season 1");
    expect(rows.map((r) => [r.kind, r.name])).toEqual([
      ["file", "e1.mkv"],
      ["file", "e2.mkv"],
    ]);
  });

  it("normalizes backslash paths", () => {
    const win = [file(0, "Season 1\\e1.mkv", "idle")];
    const rows = projectFolder(detail(win), "");
    expect(rows[0].kind).toBe("folder");
    expect(rows[0].name).toBe("Season 1");
  });

  it("does not leak a sibling folder that shares a name prefix", () => {
    const sib = [file(0, "Season 1/e1.mkv", "idle"), file(1, "Season 10/e1.mkv", "idle")];
    const rows = projectFolder(detail(sib), "Season 1");
    // "Season 10/…" must not match the "Season 1" prefix (segment boundary).
    expect(rows.map((r) => r.name)).toEqual(["e1.mkv"]);
  });

  it("handles a numeric folder name without colliding with a file index", () => {
    const numeric = [file(0, "5/a.mkv", "idle")];
    const rows = projectFolder(detail(numeric), "");
    expect(rows[0].kind).toBe("folder");
    expect(rows[0].id).toBe("d:h1:5");
  });

  it("reports progress 1 for a zero-byte complete folder", () => {
    const empty = [file(0, "S/a.mkv", "complete", { size: 0, downloaded_bytes: 0 })];
    const folder = projectFolder(detail(empty), "")[0];
    expect(folder.size).toBe(0);
    expect(folder.progress).toBe(1);
  });

  it("exposes descendant completion + active-download counts on folder rows", () => {
    // Season 1 folder = [e1 downloading, e2 idle].
    const folder = projectFolder(detail(files), "")[0];
    expect(folder.fileCount).toBe(2);
    expect(folder.completeCount).toBe(0);
    expect(folder.downloadingCount).toBe(1);
  });
});
