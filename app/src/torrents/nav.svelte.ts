// UI navigation + the page-local data the browser renders: the projected rows
// for the current path, page-local selection, cursor restore, and reconciliation
// against the live torrent list. The chrome (TopBar/SortButton) binds to the
// history + sort subset.
//
// Stores IDs, not row references: pathIds/selection stay resolvable across
// stats updates as long as the IDs survive.

import { untrack } from "svelte";
import { SvelteSet } from "svelte/reactivity";

import { daemon } from "@/daemon/client.svelte";
import { data } from "./data.svelte";
import { idToInfoHash, idToTarget } from "./ids";
import {
  folderSource,
  projectFolder,
  projectRoot,
  rootSource,
  type FilterEntry,
  type Row,
} from "./projection";
import { filterMatch } from "./filter.svelte";
import { sortRows } from "./sort";
import { sortLabels, type SortMode } from "./types";

const SORT_STORAGE_KEY = "magneto:sort";
const SORT_DEFAULT: SortMode = "added";

function readInitialSort(): SortMode {
  if (typeof localStorage === "undefined") return SORT_DEFAULT;
  try {
    const saved = localStorage.getItem(SORT_STORAGE_KEY) as SortMode | null;
    // Object.hasOwn, not `in`: a stored value like "toString" would pass the
    // prototype-chain check, then fall through sortRows' switch to undefined and
    // throw on every render, an unrecoverable blank app.
    if (saved && Object.hasOwn(sortLabels, saved)) return saved;
  } catch {
    // localStorage blocked (private mode); fall back to the default.
  }
  return SORT_DEFAULT;
}

class TorrentsNav {
  // Path as an ID chain: [] at root, [torrentId] at a torrent's root,
  // [torrentId, folderId, …] deeper. Stable across data updates while IDs survive.
  pathIds = $state<string[]>([]);
  // Single-step forward target: the id just backed out of, if any.
  forwardId = $state<string | null>(null);
  // Bumped on every navigation; the list view lands its cursor on change.
  navTick = $state(0);
  // The cursor the next list mount should land on (null → first row).
  initialCursorId = $state<string | null>(null);
  // Active sort mode, persisted to localStorage (local UI preference).
  sortMode = $state<SortMode>(readInitialSort());

  // Page-local selection of row IDs; resets on every navigation (goals:
  // selection is local to the current page). SvelteSet so add/delete/has
  // are reactive directly.
  selection = new SvelteSet<string>();

  // Fuzzy filter over the current scope. Active suspends folder nav; `typing`
  // means the input is live. Selection resets on enter/exit.
  filter = $state({ active: false, typing: false, query: "" });

  // Cursor to restore when backing into a path again, keyed by path. Saved
  // when descending, consumed by back(). Distinct from selection (which
  // resets): goals restores the cursor on Back but resets selection.
  #cursorByPath: Record<string, string> = {};

  // Rows for the current path: torrent summaries at root, projected
  // folder/file rows inside an open torrent. Re-derives from the live
  // daemon state + detail cache + sort mode.
  currentRows = $derived.by<Row[]>(() => {
    // Only the typed query engages the filter; a bare `/` leaves the sorted
    // tree in place.
    if (this.filter.active && this.filter.query !== "") {
      return filterMatch(this.#filterSource(), this.filter.query);
    }
    if (this.pathIds.length === 0) {
      return sortRows(projectRoot(daemon.torrentList), this.sortMode);
    }
    const infoHash = idToInfoHash(this.pathIds[0]);
    if (!infoHash) return [];
    const detail = data.detail(infoHash);
    if (!detail) return []; // open in flight, get_torrent pending
    return sortRows(projectFolder(detail, this.#folderPath()), this.sortMode);
  });

  get canBack(): boolean {
    return this.pathIds.length > 0;
  }
  get canForward(): boolean {
    return this.forwardId !== null;
  }

  // --- selection ---
  isMarked(id: string): boolean {
    return this.selection.has(id);
  }
  toggleMark(id: string) {
    if (this.selection.has(id)) this.selection.delete(id);
    else this.selection.add(id);
  }
  markAll(ids: string[]) {
    for (const id of ids) this.selection.add(id);
  }
  unmarkAll(ids: string[]) {
    for (const id of ids) this.selection.delete(id);
  }
  // Make the selection exactly `ids`. Diff-applied (not clear + re-add) so the
  // rubber-band drag's per-frame updates don't churn unchanged marks.
  replaceSelection(ids: ReadonlySet<string>) {
    for (const id of [...this.selection]) if (!ids.has(id)) this.selection.delete(id);
    for (const id of ids) this.selection.add(id);
  }
  clearSelection() {
    this.selection.clear();
  }

  // Drop marks whose row has genuinely left the current scope (removed /
  // reconnect). IGNORES the active filter: a committed filter's marks must
  // survive a query that scrolls them out of the fuzzy results; only a
  // truly-gone row is pruned (selection resets on filter exit anyway).
  pruneStaleMarks() {
    if (this.selection.size === 0) return;
    const scope = this.filter.active && this.filter.query !== "" ? this.#filterSource() : null;
    const live = new Set(scope ? scope.map((e) => e.row.id) : this.currentRows.map((r) => r.id));
    for (const id of [...this.selection]) if (!live.has(id)) this.selection.delete(id);
  }

  // --- navigation (pure history primitive; the caller validates that a row
  // is enterable before calling enter) ---
  enter(id: string, fromCursorId?: string) {
    if (fromCursorId !== undefined) this.#cursorByPath[pathKey(this.pathIds)] = fromCursorId;
    this.forwardId = null;
    this.initialCursorId = null; // land on the first row of the entered page
    this.pathIds = [...this.pathIds, id];
    this.selection.clear();
    this.#resetFilter();
    this.navTick++;
  }

  back() {
    if (this.pathIds.length === 0) return;
    this.forwardId = this.pathIds.at(-1) ?? null;
    const parent = this.pathIds.slice(0, -1);
    this.initialCursorId = this.#cursorByPath[pathKey(parent)] ?? null;
    this.pathIds = parent;
    this.selection.clear();
    this.#resetFilter();
    this.navTick++;
  }

  forward() {
    if (!this.forwardId) return;
    this.initialCursorId = null;
    this.pathIds = [...this.pathIds, this.forwardId];
    this.forwardId = null;
    this.selection.clear();
    this.#resetFilter();
    this.navTick++;
  }

  home() {
    this.forwardId = null;
    this.initialCursorId = null;
    this.pathIds = [];
    this.selection.clear();
    this.#resetFilter();
    this.navTick++;
  }

  // --- filter ---
  startFilter() {
    this.filter = { active: true, typing: true, query: "" };
    this.selection.clear();
    this.initialCursorId = null;
    this.navTick++;
  }
  setQuery(query: string) {
    this.filter.query = query;
    this.initialCursorId = null;
    this.navTick++;
  }
  commitFilter() {
    this.filter.typing = false;
  }
  editFilter() {
    this.filter.typing = true;
  }
  clearFilter() {
    this.#resetFilter();
    this.selection.clear();
    this.initialCursorId = null;
    this.navTick++;
  }
  #resetFilter() {
    if (this.filter.active) this.filter = { active: false, typing: false, query: "" };
  }

  // Flat candidate set for the active filter, scoped to the current position.
  #filterSource(): FilterEntry[] {
    if (this.pathIds.length === 0) return rootSource(daemon.torrentList);
    const infoHash = idToInfoHash(this.pathIds[0]);
    if (!infoHash) return [];
    const detail = data.detail(infoHash);
    if (!detail) return [];
    return folderSource(detail, this.#folderPath());
  }

  // Torrent-internal folder path of the current page ("" at the torrent
  // root). Folder IDs encode the full path, so the deepest one is it.
  #folderPath(): string {
    if (this.pathIds.length <= 1) return "";
    const target = idToTarget(this.pathIds.at(-1)!);
    return target?.kind === "folder" ? target.path : "";
  }
}

function pathKey(ids: string[]): string {
  return JSON.stringify(ids);
}

export const nav = new TorrentsNav();

if (typeof window !== "undefined") {
  $effect.root(() => {
    // Persist sort mode (local UI preference).
    $effect(() => {
      try {
        localStorage.setItem(SORT_STORAGE_KEY, nav.sortMode);
      } catch {
        // localStorage blocked; sort stays in-memory for the session.
      }
    });

    // Collapse to root when the open torrent disappears (removed/reconnect).
    // Tracks the path + that torrent's presence; the write runs untracked so
    // it can't loop on its own effect. Straight to root, not an intermediate
    // ancestor: the wire carries no per-folder liveness, so a vanished torrent
    // invalidates the whole path.
    $effect(() => {
      const ids = nav.pathIds;
      const openHash = ids.length > 0 ? idToInfoHash(ids[0]) || null : null;
      const present = openHash !== null && daemon.torrents[openHash] !== undefined;
      if (openHash !== null && !present) untrack(() => nav.home());
    });

    // Reconnect wipes the detail cache (data.svelte snapshot handler) but the
    // torrent is still in the fresh snapshot, so the page would render an empty
    // folder forever. Re-fetch its detail to self-heal; home() if it can't load.
    $effect(() => {
      const ids = nav.pathIds;
      const openHash = ids.length > 0 ? idToInfoHash(ids[0]) : null;
      if (!openHash) return;
      const present = daemon.torrents[openHash] !== undefined;
      const missing = data.detail(openHash) === undefined;
      if (present && missing) {
        untrack(() => void data.load(openHash).catch(() => nav.home()));
      }
    });

    // Drop a stale forward target whose torrent is gone (e.g. back out of a
    // folder, then delete the torrent) so canForward (and the TopBar forward
    // button) goes inactive immediately instead of recovering only on a click.
    $effect(() => {
      const fid = nav.forwardId;
      if (fid === null) return;
      const hash = idToInfoHash(fid);
      const present = hash !== null && daemon.torrents[hash] !== undefined;
      if (!present) untrack(() => (nav.forwardId = null));
    });

    // Cursor ready-landing (visible rows) + selection prune. The one-shot
    // landing covers the snapshot arriving after the browser mounts (navTick=0
    // lands on an empty list); pruneStaleMarks drops marks for genuinely-gone
    // rows (against the full scope, not the filtered view). Writes run
    // untracked, no self-loop.
    let landed = false;
    $effect(() => {
      const rows = nav.currentRows;
      untrack(() => {
        if (!landed && rows.length > 0) {
          landed = true;
          nav.navTick++;
        }
        nav.pruneStaleMarks();
      });
    });
  });
}
