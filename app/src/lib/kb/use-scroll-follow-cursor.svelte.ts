import { tick } from "svelte";
import { kb, type LayerHandle } from "./kb.svelte";

// Scroll-follow helper for layer-owning components. Subscribes to
// kb.cursorTick: programmatic landings (setCursorOn / next / prev) bump
// it; mouse clicks via kbItem do NOT, so click-to-cursor never pulls a
// row out from under the user.
//
// Targets the EXACT cursored item's node from the active layer (via
// kb.cursorNode), not a global querySelector; ids can collide across
// layers (theme names, dropdown values, button ids like 'cancel'/'confirm'
// could match torrent ids).
//
// Call from a component's <script> top-level. The handle prop is read
// through a getter because `let handle = $state<LayerHandle>()` is
// undefined at script-init and assigned later by KbLayer's `bind:handle`.
export function useScrollFollowCursor(getHandle: () => LayerHandle | undefined) {
  let last = -1;
  $effect(() => {
    const t = kb.cursorTick;
    if (t === last) return;
    last = t;
    const h = getHandle();
    if (!h || !kb.isActive(h)) return;
    const node = kb.cursorNode(h);
    if (!node) return;
    tick().then(() => {
      // At the extremes, scroll the viewport the whole way: the first item
      // reveals anything above it (a section header), the last reaches the true
      // bottom. scrollIntoView({block:'nearest'}) can't do either; it stops at
      // the item's own box, which for Settings is the inner control, not the
      // row. Middle items use it (scroll-padding gives the breathing room).
      const viewport = node.closest<HTMLElement>("[data-scroll-viewport]");
      const items = viewport?.querySelectorAll("[data-kb-id]");
      if (viewport && items && items.length > 0) {
        if (items[0] === node) return void viewport.scrollTo({ top: 0 });
        if (items[items.length - 1] === node)
          return void viewport.scrollTo({ top: viewport.scrollHeight });
      }
      node.scrollIntoView({ block: "nearest" });
    });
  });
}
