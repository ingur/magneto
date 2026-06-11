// Row identity. The browser keys every row by a kind-tagged string id; the
// daemon addresses commands by a structured Target. These helpers are the
// single, lossless bridge between the two.
//
// The kind tag (t/f/d) disambiguates a numeric folder path from a file
// index: folder "5" (`d:hash:5`) never collides with file index 5
// (`f:hash:5`). info_hash is hex (no colons), so the first colon after the
// tag always separates it from the suffix (file index or folder path).

import type { Target } from "@/daemon/protocol";

export type RowKind = "torrent" | "folder" | "file";

export function torrentId(infoHash: string): string {
  return `t:${infoHash}`;
}

export function fileId(infoHash: string, index: number): string {
  return `f:${infoHash}:${index}`;
}

export function folderId(infoHash: string, path: string): string {
  return `d:${infoHash}:${path}`;
}

// Decode a row id to the Target a command addresses. Returns null for an
// unparseable id so callers can filter defensively.
export function idToTarget(id: string): Target | null {
  const tag = id[0];
  const rest = id.slice(2); // drop "t:" / "f:" / "d:"
  if (tag === "t") return { kind: "torrent", info_hash: rest };
  const sep = rest.indexOf(":");
  if (sep < 0) return null;
  const infoHash = rest.slice(0, sep);
  const suffix = rest.slice(sep + 1);
  if (tag === "f") {
    const fileIndex = Number(suffix);
    if (!Number.isInteger(fileIndex)) return null;
    return { kind: "file", info_hash: infoHash, file_index: fileIndex };
  }
  if (tag === "d") return { kind: "folder", info_hash: infoHash, path: suffix };
  return null;
}

// The info_hash a row belongs to, for get_torrent / remove_torrent, which
// address a whole torrent rather than a Target.
export function idToInfoHash(id: string): string | null {
  return idToTarget(id)?.info_hash ?? null;
}
