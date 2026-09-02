import { afterEach, describe, expect, it, vi } from "vitest";

import { daemon } from "@/daemon/client.svelte";

import {
  addFailureHandled,
  adding,
  dedupeSources,
  isAddSource,
  isTorrentFile,
  runAddSource,
} from "./add";

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

describe("in-flight adds", () => {
  const magnet = (tr: string) => `magnet:?xt=urn:btih:ABCDEF0123&dn=x&tr=${tr}`;

  afterEach(() => {
    adding.clear();
    vi.restoreAllMocks();
  });

  it("sends one request for two adds of the same torrent", async () => {
    let release: (v: unknown) => void = () => {};
    const request = vi
      .spyOn(daemon, "request")
      .mockImplementation(() => new Promise((r) => (release = r)));

    const first = runAddSource(magnet("udp://a"));
    expect(adding.size).toBe(1);
    // Same torrent, different tracker list: still the same add.
    await expect(runAddSource(magnet("udp://b"))).resolves.toBe(true);
    expect(request).toHaveBeenCalledTimes(1);

    release({ already_existed: false });
    await first;
    expect(adding.size).toBe(0);
  });

  it("stops tracking an add that failed", async () => {
    vi.spyOn(daemon, "request").mockRejectedValue(new Error("daemon not connected"));
    await expect(runAddSource(magnet("udp://a"))).resolves.toBe(false);
    expect(adding.size).toBe(0);
  });
});

describe("dedupeSources", () => {
  it("keeps the first of each torrent in a batch", () => {
    const a = "magnet:?xt=urn:btih:AAAA&tr=udp://one";
    const b = " magnet:?xt=urn:btih:aaaa&dn=x&tr=udp://two ";
    const c = "magnet:?xt=urn:btih:BBBB";
    expect(dedupeSources([a, b, c, c])).toEqual([a, c]);
  });

  it("treats sources without an info hash by their trimmed text", () => {
    expect(dedupeSources(["/tmp/a.torrent", " /tmp/a.torrent", "/tmp/b.torrent"])).toEqual([
      "/tmp/a.torrent",
      "/tmp/b.torrent",
    ]);
  });
});
