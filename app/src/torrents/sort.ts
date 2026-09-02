// Row sorting over structured fields (never display strings). Returns the
// input untouched for "added" so keyed-each keeps stable DOM; the other
// modes sort a copy.

import type { Row, RowState } from "./projection";
import type { SortMode } from "./types";

// Status sort groups by activity: actively moving first, settled last. Stalled
// is downloading that receives nothing and ranks with it; queued sits just
// behind (wanted and pending), ahead of paused/idle.
const STATE_RANK: Record<RowState, number> = {
  downloading: 0,
  stalled: 0,
  queued: 1,
  paused: 2,
  idle: 3,
  initializing: 4,
  error: 5,
  complete: 6,
};

export function sortRows(rows: Row[], mode: SortMode): Row[] {
  switch (mode) {
    case "added":
      // Root torrents carry added_at → newest first; rows without one (a folder
      // page, or a not-yet-resolved torrent) sort last rather than disabling the
      // sort for the whole list. No addedAt anywhere → keep natural file order
      // (returns the same array so keyed-each DOM stays stable).
      if (!rows.some((r) => r.addedAt)) return rows;
      return [...rows].sort((a, b) => (b.addedAt ?? "").localeCompare(a.addedAt ?? ""));
    case "name-asc":
      return [...rows].sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }));
    case "name-desc":
      return [...rows].sort((a, b) => b.name.localeCompare(a.name, undefined, { numeric: true }));
    case "size-desc":
      return [...rows].sort((a, b) => b.size - a.size);
    case "size-asc":
      return [...rows].sort((a, b) => a.size - b.size);
    case "status":
      return [...rows].sort((a, b) => STATE_RANK[a.state] - STATE_RANK[b.state]);
  }
}
