// OS-handler intake: magnet links and .torrent paths the Tauri host queued
// (cold-start argv, a second launch forwarded by single-instance, macOS open
// events). The host queue is the single buffer: sources stay there until the
// daemon socket is connected, so nothing is lost while the daemon is still
// spawning or if the webview reloads. Drained on every sources-ready ping
// and on every snapshot (each (re)connect emits one, which also covers any
// ping that fires before the listener is registered), then routed through
// the normal add flow.

import { daemon } from "@/daemon/client.svelte";
import { onSourcesReady, takePendingSources } from "@/daemon/tauri";
import { isAddSource, runAddPaths, runAddSource } from "./add";

/** Start draining host-queued sources. Returns the unsubscriber. */
export function initIntake(): () => void {
  const offReady = onSourcesReady(() => void drain());
  const offEvents = daemon.onEvent((e) => {
    if (e.type === "snapshot") void drain();
  });
  return () => {
    offReady();
    offEvents();
  };
}

async function drain(): Promise<void> {
  if (daemon.status !== "connected") return;
  const sources = await takePendingSources();
  const paths: string[] = [];
  for (const source of sources) {
    if (isAddSource(source)) await runAddSource(source);
    else paths.push(source);
  }
  if (paths.length > 0) await runAddPaths(paths);
}
