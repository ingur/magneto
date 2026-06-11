import { describe, expect, it } from "vitest";

import { fileId, folderId, idToInfoHash, idToTarget, torrentId } from "./ids";

describe("row ids", () => {
  it("round-trips a torrent id", () => {
    const id = torrentId("abc123");
    expect(idToTarget(id)).toEqual({ kind: "torrent", info_hash: "abc123" });
    expect(idToInfoHash(id)).toBe("abc123");
  });

  it("round-trips a file id", () => {
    const id = fileId("abc123", 7);
    expect(idToTarget(id)).toEqual({ kind: "file", info_hash: "abc123", file_index: 7 });
    expect(idToInfoHash(id)).toBe("abc123");
  });

  it("round-trips a folder id with a slashed path", () => {
    const id = folderId("abc123", "Season 1/Extras");
    expect(idToTarget(id)).toEqual({
      kind: "folder",
      info_hash: "abc123",
      path: "Season 1/Extras",
    });
    expect(idToInfoHash(id)).toBe("abc123");
  });

  it("disambiguates a numeric folder path from a file index", () => {
    const folder = folderId("abc", "5");
    const file = fileId("abc", 5);
    expect(folder).not.toBe(file);
    expect(idToTarget(folder)).toEqual({ kind: "folder", info_hash: "abc", path: "5" });
    expect(idToTarget(file)).toEqual({ kind: "file", info_hash: "abc", file_index: 5 });
  });

  it("returns null for an unparseable id", () => {
    expect(idToTarget("zzz")).toBeNull();
    expect(idToInfoHash("zzz")).toBeNull();
  });
});
