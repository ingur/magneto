// Facts derived from a torrent summary. They used to travel on the wire as
// booleans a stats delta could not refresh; deriving them from `state` keeps
// them in step with every patch.

import type { TorrentSummary } from "./protocol";

type Status = Pick<TorrentSummary, "state" | "is_paused">;

export function isInitializing(t: Status): boolean {
  return t.state === "initializing";
}

export function isComplete(t: Status): boolean {
  return t.state === "complete";
}

export function isSeeding(t: Status): boolean {
  return t.state === "complete" && !t.is_paused;
}
