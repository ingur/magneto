// The add-torrent flow: drop `.torrent` files, paste a magnet/link, or pick
// files from the list's context menu. All funnel to runAddSource, which sends
// one add_torrent. A magnet add resolves asynchronously, so the success /
// no-media / fallback toast comes from the event bridge (App.svelte); the add
// only toasts the synchronous outcomes (already-existed, request error).

import { daemon } from "@/daemon/client.svelte";
import type { AddTorrentResp } from "@/daemon/protocol";
import { pickTorrentFiles, readClipboardText, readTorrentFile } from "@/daemon/tauri";
import { toast } from "@/lib/feedback/toasts/toasts.svelte";

const SOURCE_RE = /^(magnet:|https?:\/\/)/i;

/** A pasteable add source: a magnet URI or an HTTP(S) torrent URL. */
export function isAddSource(text: string): boolean {
  return SOURCE_RE.test(text.trim());
}

/** Same extension semantics as the host's fence (sources.rs / Path::extension):
 *  a file named exactly ".torrent" has no extension and is rejected, so a
 *  dropped path never bounces off the host with a generic read error. */
export function isTorrentFile(path: string): boolean {
  const name = basename(path).toLowerCase();
  return name.endsWith(".torrent") && name !== ".torrent";
}

export async function runAddSource(source: string): Promise<void> {
  try {
    const resp = await daemon.request<AddTorrentResp>("add_torrent", { source });
    if (resp.already_existed) toast.info("Already added");
  } catch (e) {
    toast.error(addError(e));
  }
}

/** Paste path: validate the text, then add. */
export function runAddText(text: string): void {
  const trimmed = text.trim();
  if (isAddSource(trimmed)) void runAddSource(trimmed);
  else toast.error("No magnet or torrent link in the clipboard");
}

/** Context-menu paste: read the clipboard, then run the paste path. */
export async function runPasteClipboard(): Promise<void> {
  let text = "";
  try {
    text = await readClipboardText();
  } catch {
    // unreadable clipboard → empty text → runAddText's "nothing pasteable" toast
  }
  runAddText(text);
}

/** Picker path: choose `.torrent` files, then add them like a drop. */
export async function runAddPicked(): Promise<void> {
  const paths = await pickTorrentFiles();
  if (paths.length > 0) await runAddPaths(paths);
}

/** Drop path: add every `.torrent`, toast the rest. */
export async function runAddPaths(paths: string[]): Promise<void> {
  const torrents = paths.filter(isTorrentFile);
  for (const path of torrents) {
    try {
      await runAddSource(await readTorrentFile(path));
    } catch {
      toast.error(`Couldn't read ${basename(path)}`);
    }
  }
  const rejected = paths.length - torrents.length;
  if (rejected > 0) {
    toast.error(
      torrents.length === 0
        ? "Only .torrent files can be dropped"
        : `${rejected} non-.torrent file${rejected === 1 ? "" : "s"} skipped`,
    );
  }
}

function addError(e: unknown): string {
  const msg = e instanceof Error ? e.message : String(e);
  return msg === "daemon not connected" ? "Not connected to the daemon" : msg;
}

function basename(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}
