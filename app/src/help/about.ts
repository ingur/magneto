// The About prompt and repository link, shared by the Help overlay's buttons
// and the topbar wordmark's context menu.

import { appVersion, openExternal } from "@/daemon/tauri";
import { prompt } from "@/lib/feedback/prompts/prompts.svelte";
import { toast } from "@/lib/feedback/toasts/toasts.svelte";

export const REPO_URL = "https://github.com/ingur/magneto";

export async function showAbout(): Promise<void> {
  const version = await appVersion();
  void prompt({
    type: "info",
    title: `magneto v${version}`,
    description: "A media-focused torrent client.\nmade by ingur ♥️",
  });
}

export async function openRepository(): Promise<void> {
  try {
    await openExternal(REPO_URL);
  } catch (e) {
    toast.error(`Couldn't open the link: ${e instanceof Error ? e.message : String(e)}`);
  }
}
