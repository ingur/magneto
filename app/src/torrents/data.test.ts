import { afterEach, describe, expect, it, vi } from "vitest";

import { daemon } from "@/daemon/client.svelte";
import type { FileEntry, TorrentDetail } from "@/daemon/protocol";
import { TorrentsData } from "./data.svelte";

function file(index: number, over: Partial<FileEntry> = {}): FileEntry {
  return {
    index,
    path: `f${index}.mkv`,
    size: 100,
    downloaded_bytes: 0,
    selected: true,
    state: "downloading",
    persisted: false,
    shared: false,
    ...over,
  };
}

// The cache only reads `.files`, so a minimal detail is enough here.
function detail(files: FileEntry[]): TorrentDetail {
  return { info_hash: "h1", files } as unknown as TorrentDetail;
}

afterEach(() => vi.restoreAllMocks());

describe("TorrentsData.patchFiles", () => {
  it("patches matching files and reverts to the prior values", async () => {
    vi.spyOn(daemon, "request").mockResolvedValue(detail([file(0), file(1), file(2)]));
    const data = new TorrentsData();
    await data.load("h1");

    const revert = data.patchFiles("h1", new Set([0, 2]), { persisted: true });
    const persisted = () => data.detail("h1")!.files.map((f) => f.persisted);
    expect(persisted()).toEqual([true, false, true]);

    revert();
    expect(persisted()).toEqual([false, false, false]);
  });

  it("revert restores only the patched keys, keeping a concurrent stats update", async () => {
    vi.spyOn(daemon, "request").mockResolvedValue(detail([file(0)]));
    const data = new TorrentsData();
    await data.load("h1");

    const revert = data.patchFiles("h1", new Set([0]), { persisted: true });
    // A stats delta lands between apply and revert.
    data.detail("h1")!.files[0].downloaded_bytes = 50;
    revert();

    const f = data.detail("h1")!.files[0];
    expect(f.persisted).toBe(false); // patched key restored
    expect(f.downloaded_bytes).toBe(50); // unrelated update kept
  });

  it("revert is a no-op after the detail is replaced mid-flight", async () => {
    const req = vi
      .spyOn(daemon, "request")
      .mockResolvedValue(detail([file(0, { persisted: false })]));
    const data = new TorrentsData();
    await data.load("h1");

    const revert = data.patchFiles("h1", new Set([0]), { persisted: true });
    // An authoritative refresh replaces the detail (now persisted = true).
    req.mockResolvedValue(detail([file(0, { persisted: true })]));
    await data.load("h1");

    revert(); // must not clobber the fresh detail back to false
    expect(data.detail("h1")!.files[0].persisted).toBe(true);
  });

  it("is a no-op for a torrent that isn't open", () => {
    const data = new TorrentsData();
    expect(() => data.patchFiles("missing", new Set([0]), { shared: true })()).not.toThrow();
  });
});
