import { beforeAll, describe, expect, it } from "vitest";

import type { FileEntry, FileState, TorrentDetail, TorrentSummary } from "@/daemon/protocol";
import { ensureFuseLoaded, filterMatch } from "./filter.svelte";
import { folderSource, rootSource } from "./projection";

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

function file(index: number, path: string, state: FileState = "idle"): FileEntry {
  return {
    index,
    path,
    size: 100,
    downloaded_bytes: 0,
    selected: true,
    state,
    persisted: false,
    shared: false,
  };
}

function detail(files: FileEntry[]): TorrentDetail {
  return { ...summary(), files };
}

const shows = () =>
  rootSource([
    summary({ name: "Breaking Bad" }),
    summary({ info_hash: "h2", name: "Better Call Saul" }),
  ]);

describe("filter source", () => {
  it("rootSource maps torrents to name-keyed entries", () => {
    const entries = shows();
    expect(entries.map((e) => e.text)).toEqual(["Breaking Bad", "Better Call Saul"]);
    expect(entries[0].row.kind).toBe("torrent");
  });

  it("folderSource flattens files under the path with a relative-path hint", () => {
    const entries = folderSource(
      detail([file(0, "S1/e1.mkv"), file(1, "S1/Extras/bts.mkv"), file(2, "S2/e1.mkv")]),
      "S1",
    );
    expect(entries.map((e) => e.text)).toEqual(["e1.mkv", "Extras/bts.mkv"]);
    const bts = entries.find((e) => e.text === "Extras/bts.mkv")!;
    expect(bts.row.name).toBe("bts.mkv");
    expect(bts.row.pathHint).toBe("Extras/");
  });

  it("folderSource respects the segment boundary", () => {
    const entries = folderSource(detail([file(0, "S1/e1.mkv"), file(1, "S10/e1.mkv")]), "S1");
    expect(entries.map((e) => e.text)).toEqual(["e1.mkv"]);
  });

  it("folderSource at the torrent root flattens every file; top-level files have no hint", () => {
    const entries = folderSource(detail([file(0, "intro.mkv"), file(1, "S1/e1.mkv")]), "");
    expect(entries.map((e) => e.text)).toEqual(["intro.mkv", "S1/e1.mkv"]);
    expect(entries[0].row.pathHint).toBeUndefined();
    expect(entries[1].row.pathHint).toBe("S1/");
  });
});

describe("filterMatch", () => {
  it("returns every row before fuse has loaded", () => {
    expect(filterMatch(shows(), "zzz").length).toBe(2);
  });

  describe("with fuse loaded", () => {
    beforeAll(() => ensureFuseLoaded());

    it("ranks the best fuzzy match first", () => {
      const names = filterMatch(shows(), "breaking bad").map((r) => r.name);
      expect(names[0]).toBe("Breaking Bad");
    });

    it("returns nothing for a non-match", () => {
      expect(filterMatch(shows(), "zzzzz")).toEqual([]);
    });
  });
});
