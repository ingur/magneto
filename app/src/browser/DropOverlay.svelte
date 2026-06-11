<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { onMount } from "svelte";

  import { isTauri } from "@/daemon/tauri";
  import { runAddPaths } from "@/torrents/add";

  // A tint over the main region while files are dragged onto the window. Native
  // drops are intercepted by the webview (dragDropEnabled defaults true), so the
  // drag state and dropped paths come from onDragDropEvent, not DOM events.
  let dragging = $state(false);

  onMount(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    // onDragDropEvent resolves its unlisten asynchronously; track it so a fast
    // unmount can't leak the listener.
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter" || p.type === "over") dragging = true;
        else if (p.type === "leave") dragging = false;
        else if (p.type === "drop") {
          dragging = false;
          void runAddPaths(p.paths);
        }
      })
      .then((un) => (disposed ? un() : (unlisten = un)));
    return () => {
      disposed = true;
      unlisten?.();
    };
  });
</script>

{#if dragging}
  <div
    class="bg-backdrop absolute inset-0 flex items-center justify-center backdrop-blur-[2px]"
    style="z-index: 20"
  >
    <div class="text-fg text-sm">Drop <span class="text-accent">.torrent</span> to add</div>
  </div>
{/if}
