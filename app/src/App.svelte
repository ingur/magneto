<script lang="ts">
  import { onMount } from "svelte";

  import { daemon } from "@/daemon/client.svelte";
  import { ensureDaemon, initNotifications, quitApp } from "@/daemon/tauri";
  import { notifyBackground } from "@/notifications.svelte";
  import type { ServerEvent } from "@/daemon/protocol";
  import { menus } from "@/lib/popover/menus.svelte";
  import { overlays } from "@/lib/surface/overlays.svelte";
  import { prompt } from "@/lib/feedback/prompts/prompts.svelte";
  import { toast } from "@/lib/feedback/toasts/toasts.svelte";
  import { runAddText } from "@/torrents/add";
  import { initIntake } from "@/torrents/intake";
  import { resolveTargets, runCopyMagnet } from "@/torrents/actions";
  import { zoom } from "@/zoom";
  import TopBar from "@/chrome/TopBar.svelte";
  import StatusBar from "@/chrome/StatusBar.svelte";
  import Browser from "@/browser/Browser.svelte";
  import DropOverlay from "@/browser/DropOverlay.svelte";
  import OverlayHost from "@/overlays/OverlayHost.svelte";
  import PromptStack from "@/lib/feedback/prompts/PromptStack.svelte";
  import ToastStack from "@/lib/feedback/toasts/ToastStack.svelte";

  // Map daemon lifecycle events to toasts. Runs after the client's own
  // state update (see DaemonClient.onEvent), so the torrents map is current
  // for events that keep the torrent. torrent_removed is the exception:
  // the torrent is already deleted and the event carries no name, so its
  // copy is reason-based only.
  function handleEvent(e: ServerEvent) {
    switch (e.type) {
      case "torrent_ready":
        toast.success(`Added ${e.name ?? "torrent"}`);
        break;
      case "torrent_complete": {
        const name = daemon.torrents[e.info_hash]?.name ?? "Download";
        toast.success(`${name} complete`);
        notifyBackground("Download complete", name);
        break;
      }
      case "torrent_removed":
        if (e.reason === "no_media")
          toast.warn(
            e.fallback_launched
              ? "No playable media. Sent to the fallback app"
              : "No playable media. Torrent removed",
          );
        else if (e.reason === "fallback") toast.info("Sent to the fallback app");
        // "user" removals are toasted by the delete action itself (one toast for
        // the whole gesture, not one per torrent); "cleanup" is shutdown
        // housekeeping. Both stay silent here.
        break;
      case "torrent_error":
        toast.error(`${daemon.torrents[e.info_hash]?.name ?? e.info_hash.slice(0, 8)}: ${e.error}`);
        break;
      case "player_launch_failed":
        toast.error(`Couldn't launch the player: ${e.error}`);
        break;
      // config_changed drives no toast: Settings owns save/restart messaging,
      // and a restart-required save restarts automatically (see settings/store).
    }
  }

  onMount(() => {
    const offContext = menus.installContextHandler();
    const offEvents = daemon.onEvent(handleEvent);
    // The client re-resolves the port on every dial and retries on failure, so
    // a daemon that is down or still starting at launch is just a connecting/
    // reconnecting state, not a dead end. The status chrome shows the progress.
    // Window close only hides to tray (host-side); the daemon is stopped by
    // the host when the app exits, so this lifecycle owns no shutdown.
    daemon.connect(ensureDaemon);
    void initNotifications();

    // OS-handed sources (magnet clicks, .torrent opens) flow from the host
    // queue into the add flow once the socket is up.
    const offIntake = initIntake();

    // Paste a magnet/link to add, outside a text input and any open overlay.
    const onPaste = (e: ClipboardEvent) => {
      const el = e.target as HTMLElement | null;
      const inField =
        el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
      if (inField || overlays.any) return;
      const text = e.clipboardData?.getData("text") ?? "";
      if (!text) return;
      e.preventDefault();
      runAddText(text);
    };
    window.addEventListener("paste", onPaste);

    // UI zoom works in every layer (overlays included): a window listener catches
    // ctrl/cmd +/-/0 when the active kb layer doesn't bind it. The browser layer
    // does, and the kb engine stops propagation on a match, so this never
    // double-fires there. Accepts ctrl OR meta so it works on Linux and macOS.
    const onZoom = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      if (e.key === "=" || e.key === "+") zoom.in();
      else if (e.key === "-") zoom.out();
      else if (e.key === "0") zoom.reset();
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onZoom);

    // ctrl/cmd+q quits for real: window close only hides to tray, so the
    // chord is the keyboard path to an actual exit (the tray menu is the
    // mouse path, and the only path on hosts without a tray). Confirmed via
    // prompt so a stray chord can't stop active downloads; the in-flight
    // flag keeps a held/repeated chord from queueing prompts.
    let quitPrompting = false;
    const onQuit = (e: KeyboardEvent) => {
      if (e.key !== "q" || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
      e.preventDefault();
      if (quitPrompting) return;
      quitPrompting = true;
      void (async () => {
        try {
          const ok = await prompt({
            type: "confirm",
            title: "Quit Magneto?",
            description: "Active downloads and sharing stop.\nClosing the window keeps them running.",
            confirmLabel: "Quit",
            tint: "danger",
          });
          if (ok) await quitApp();
        } finally {
          quitPrompting = false;
        }
      })();
    };
    window.addEventListener("keydown", onQuit);

    // ctrl/cmd+c yanks the cursor/selection's magnet, the familiar copy chord,
    // aliasing `y`. A window listener (not a kb binding) so it defers to native
    // copy inside a text field and only acts in the browser context.
    const onYank = (e: KeyboardEvent) => {
      if (e.key !== "c" || !(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
      const el = e.target as HTMLElement | null;
      const inField =
        el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
      if (inField || overlays.any) return;
      e.preventDefault();
      runCopyMagnet(resolveTargets());
    };
    window.addEventListener("keydown", onYank);

    return () => {
      offEvents();
      offContext();
      offIntake();
      window.removeEventListener("paste", onPaste);
      window.removeEventListener("keydown", onZoom);
      window.removeEventListener("keydown", onQuit);
      window.removeEventListener("keydown", onYank);
      daemon.disconnect();
    };
  });
</script>

<!-- Full-bleed shell: the app fills the borderless window (decorations:false).
     TopBar and StatusBar are fixed chrome; the middle region is the only
     positioning context for the list, overlays, prompts, and toasts, so
     they never cover the StatusBar. data-menu-bounds tags it as the clamp
     region popover menus must stay inside. -->
<div class="flex h-full flex-col overflow-hidden bg-bg font-sans text-fg antialiased select-none">
  <TopBar />
  <div class="relative flex min-h-0 flex-1 flex-col" data-menu-bounds>
    <Browser />
    <OverlayHost />
    <PromptStack />
    <ToastStack />
    <DropOverlay />
  </div>
  <StatusBar />
</div>
