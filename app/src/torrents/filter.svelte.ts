// Fuzzy matcher for the browser filter. fuse.js is loaded lazily on the first
// `/` so it never weighs on first paint; the matcher reads the loaded
// constructor through $state, so currentRows recomputes the moment it arrives
// (until then it returns the full set, so the first `/` is never a race).

import type FuseType from "fuse.js";

import type { FilterEntry, Row } from "./projection";

let ctor = $state<typeof FuseType | null>(null);

export async function ensureFuseLoaded(): Promise<void> {
  if (ctor) return;
  ctor = (await import("fuse.js")).default;
}

export function filterMatch(entries: FilterEntry[], query: string): Row[] {
  const Fuse = ctor;
  if (!Fuse) return entries.map((e) => e.row);
  const fuse = new Fuse(entries, { keys: ["text"], threshold: 0.3, ignoreLocation: true });
  return fuse.search(query).map((r) => r.item.row);
}
