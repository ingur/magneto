import { beforeEach, describe, expect, it } from "vitest";
import { flushSync } from "svelte";

import { daemon } from "@/daemon/client.svelte";
import type { TorrentSummary } from "@/daemon/protocol";
import { nav } from "./nav.svelte";
import { torrentId } from "./ids";

function torrentSummary(info_hash: string, name: string): TorrentSummary {
  return {
    info_hash,
    name,
    source: null,
    source_kind: null,
    state: "downloading",
    total_bytes_all: 100,
    total_bytes_selected: 100,
    downloaded_bytes: 50,
    download_speed: 0,
    upload_speed: 0,
    file_count: 1,
    complete_count: 0,
    selected_count: 1,
    persisted_count: 0,
    shared_count: 0,
    is_initializing: false,
    is_complete: false,
    is_seeding: false,
    is_paused: false,
    added_at: "2026-06-01T00:00:00Z",
  };
}

beforeEach(() => {
  // Reset the singleton between tests.
  nav.pathIds = [];
  nav.forwardId = null;
  nav.navTick = 0;
  nav.initialCursorId = null;
  nav.sortMode = "added";
  nav.clearSelection();
  nav.filter = { active: false, typing: false, query: "" };
  daemon.torrents = {};
  localStorage.clear();
});

describe("nav history", () => {
  it("starts at root with no back/forward", () => {
    expect(nav.canBack).toBe(false);
    expect(nav.canForward).toBe(false);
  });

  it("enter pushes a path and enables back", () => {
    nav.enter("t1");
    expect(nav.pathIds).toEqual(["t1"]);
    expect(nav.canBack).toBe(true);
    expect(nav.canForward).toBe(false);
  });

  it("back pops and enables forward; forward round-trips", () => {
    nav.enter("t1");
    nav.enter("a");
    nav.back();
    expect(nav.pathIds).toEqual(["t1"]);
    expect(nav.canForward).toBe(true);
    nav.forward();
    expect(nav.pathIds).toEqual(["t1", "a"]);
    expect(nav.canForward).toBe(false);
  });

  it("entering clears the forward target", () => {
    nav.enter("t1");
    nav.back();
    expect(nav.canForward).toBe(true);
    nav.enter("t2");
    expect(nav.canForward).toBe(false);
    expect(nav.pathIds).toEqual(["t2"]);
  });

  it("home clears the path and forward", () => {
    nav.enter("t1");
    nav.enter("a");
    nav.back();
    nav.home();
    expect(nav.pathIds).toEqual([]);
    expect(nav.canBack).toBe(false);
    expect(nav.canForward).toBe(false);
  });

  it("back/forward are no-ops at their bounds (no navTick bump)", () => {
    nav.back();
    expect(nav.pathIds).toEqual([]);
    expect(nav.navTick).toBe(0);
    nav.forward();
    expect(nav.navTick).toBe(0);
  });

  it("each real navigation bumps navTick", () => {
    nav.enter("t1");
    nav.back();
    nav.forward();
    nav.home();
    expect(nav.navTick).toBe(4);
  });

  it("persists sortMode to localStorage", () => {
    nav.sortMode = "name-asc";
    flushSync();
    expect(localStorage.getItem("magneto:sort")).toBe("name-asc");
  });
});

describe("selection", () => {
  it("toggles a mark on and off", () => {
    nav.toggleMark("a");
    expect(nav.isMarked("a")).toBe(true);
    nav.toggleMark("a");
    expect(nav.isMarked("a")).toBe(false);
  });

  it("marks all and clears", () => {
    nav.markAll(["a", "b", "c"]);
    expect(nav.selection.size).toBe(3);
    nav.unmarkAll(["b"]);
    expect(nav.selection.size).toBe(2);
    nav.clearSelection();
    expect(nav.selection.size).toBe(0);
  });

  it("resets selection on every navigation", () => {
    nav.markAll(["a", "b"]);
    nav.enter("t1");
    expect(nav.selection.size).toBe(0);

    nav.markAll(["c"]);
    nav.back();
    expect(nav.selection.size).toBe(0);

    nav.markAll(["d"]);
    nav.home();
    expect(nav.selection.size).toBe(0);
  });
});

describe("filter", () => {
  it("startFilter activates typing, empties the query, resets selection", () => {
    nav.markAll(["a", "b"]);
    nav.startFilter();
    expect(nav.filter).toEqual({ active: true, typing: true, query: "" });
    expect(nav.selection.size).toBe(0);
  });

  it("setQuery updates the query and bumps navTick", () => {
    nav.startFilter();
    const before = nav.navTick;
    nav.setQuery("foo");
    expect(nav.filter.query).toBe("foo");
    expect(nav.navTick).toBe(before + 1);
  });

  it("commit/edit toggle typing without touching the query", () => {
    nav.startFilter();
    nav.setQuery("foo");
    nav.commitFilter();
    expect(nav.filter).toEqual({ active: true, typing: false, query: "foo" });
    nav.editFilter();
    expect(nav.filter.typing).toBe(true);
  });

  it("clearFilter resets the filter and the selection", () => {
    nav.startFilter();
    nav.setQuery("foo");
    nav.markAll(["a"]);
    nav.clearFilter();
    expect(nav.filter).toEqual({ active: false, typing: false, query: "" });
    expect(nav.selection.size).toBe(0);
  });

  it("any navigation clears the filter", () => {
    nav.startFilter();
    nav.setQuery("foo");
    nav.enter("t1");
    expect(nav.filter.active).toBe(false);

    nav.startFilter();
    nav.home();
    expect(nav.filter.active).toBe(false);
  });
});

describe("filter selection scope", () => {
  it("a bare '/' (empty query) leaves the full tree in place", () => {
    daemon.torrents = { a: torrentSummary("a", "Alpha"), b: torrentSummary("b", "Beta") };
    nav.startFilter();
    expect(nav.currentRows.map((r) => r.id).sort()).toEqual(
      [torrentId("a"), torrentId("b")].sort(),
    );
  });

  it("prunes committed-filter marks against the full scope, not the filtered view", () => {
    daemon.torrents = { a: torrentSummary("a", "Alpha"), b: torrentSummary("b", "Beta") };
    nav.startFilter();
    nav.setQuery("alpha"); // Beta would scroll out of the fuzzy results
    nav.markAll([torrentId("a"), torrentId("b")]);
    nav.pruneStaleMarks();
    expect(nav.selection.size).toBe(2); // Beta kept: still in scope, just filtered from view
    daemon.torrents = { a: torrentSummary("a", "Alpha") }; // Beta genuinely removed
    nav.pruneStaleMarks();
    expect(nav.isMarked(torrentId("b"))).toBe(false);
    expect(nav.isMarked(torrentId("a"))).toBe(true);
  });
});
