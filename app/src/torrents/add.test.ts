import { describe, expect, it } from "vitest";

import { addFailureHandled, isAddSource, isTorrentFile } from "./add";

describe("add source validation", () => {
  it("accepts magnets and http(s) urls (trimmed, case-insensitive)", () => {
    expect(isAddSource("magnet:?xt=urn:btih:abc")).toBe(true);
    expect(isAddSource("  https://example.com/x.torrent ")).toBe(true);
    expect(isAddSource("http://example.com/x.torrent")).toBe(true);
    expect(isAddSource("MAGNET:?xt=urn:btih:abc")).toBe(true);
  });

  it("rejects anything else", () => {
    expect(isAddSource("hello world")).toBe(false);
    expect(isAddSource("ftp://x")).toBe(false);
    expect(isAddSource("")).toBe(false);
  });

  it("recognizes .torrent paths case-insensitively", () => {
    expect(isTorrentFile("/a/b/c.torrent")).toBe(true);
    expect(isTorrentFile("C:\\x\\Y.TORRENT")).toBe(true);
    expect(isTorrentFile("/a/b.mkv")).toBe(false);
  });

  it("rejects a file named exactly .torrent, like the host fence", () => {
    expect(isTorrentFile("/tmp/.torrent")).toBe(false);
    expect(isTorrentFile(".torrent")).toBe(false);
    expect(isTorrentFile("/tmp/.foo.torrent")).toBe(true);
  });
});

describe("failed add disposition", () => {
  it("hands a lost request back to the queue", () => {
    expect(addFailureHandled("daemon not connected")).toBe(false);
    expect(addFailureHandled("connection closed")).toBe(false);
  });

  it("keeps a daemon verdict out of the queue", () => {
    expect(addFailureHandled("add_torrent failed: metadata not resolved within 120s")).toBe(true);
    expect(addFailureHandled("invalid base64 torrent bytes: bad")).toBe(true);
    expect(addFailureHandled("could not record torrent: disk full")).toBe(true);
  });
});
