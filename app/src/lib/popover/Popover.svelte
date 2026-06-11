<script lang="ts">
  import type { Snippet } from "svelte";
  import { portal } from "./portal";
  import { positionAnchored, positionAtPoint, positionDropdown } from "./position";

  // Reusable positioned, portaled surface. Owns: portal, position
  // computation, outside-click close, optional scroll/resize reposition,
  // trigger exclusion. Does NOT own: kb layer, item rendering, global
  // popover coordination; those are caller concerns. Popovers can
  // coexist; each one uses its own outside-click handler.
  //
  // Anchor shapes:
  //   { kind: 'rect', el, mode: 'dropdown' }   - drops below/above trigger,
  //                                              caller usually wants
  //                                              matchTriggerWidth=true.
  //   { kind: 'rect', el, mode: 'anchored' }   - flips to opposite corner
  //                                              of trigger when needed
  //                                              (file-manager menus).
  //   { kind: 'point', x, y }                  - cursor / right-click.
  //
  // bounds() lets the caller pick the clamp region (viewport, closest
  // [data-menu-bounds], closest [role="dialog"], or any custom rect) at
  // call time. Use lib/popover/bounds.ts to compose it.
  //
  // closeOnRightClickOutside=false (Menu): right-click outside doesn't
  // dismiss, so the global contextmenu handler can decide what to do
  // (close + don't reopen, today's two-step behavior).
  //
  // repositionOnScrollResize=true (Dropdown): captures scroll on every
  // ancestor (overlay's ScrollArea, etc.) so the menu tracks its trigger.

  type Anchor =
    | { kind: "rect"; el: HTMLElement; mode: "dropdown" | "anchored" }
    | { kind: "point"; x: number; y: number };

  interface Props {
    open: boolean;
    anchor: Anchor;
    est: { width: number; height: number };
    bounds: () => DOMRect;
    closeOnRightClickOutside: boolean;
    repositionOnScrollResize: boolean;
    trigger?: HTMLElement | null;
    gap?: number;
    minWidth?: number;
    matchTriggerWidth?: boolean;
    z?: number;
    // Snippet receives `close` so child UIs (escape binding, item activate)
    // can dismiss the popover, and `maxHeight` so dropdown-style children
    // can clamp scroll regions to the available space.
    children: Snippet<[{ close: () => void; maxHeight: number | null }]>;
  }

  let {
    open = $bindable(false),
    anchor,
    est,
    bounds,
    closeOnRightClickOutside,
    repositionOnScrollResize,
    trigger = null,
    gap = 4,
    minWidth,
    matchTriggerWidth = false,
    z = 50,
    children,
  }: Props = $props();

  let popoverEl: HTMLDivElement | undefined = $state();

  // Position state. Driven by computePosition(): initial computation
  // happens via the open-effect below; reposition listeners (when enabled)
  // re-call it on scroll/resize.
  let top = $state<number | null>(null);
  let bottom = $state<number | null>(null);
  let left = $state<number | null>(null);
  let right = $state<number | null>(null);
  let width = $state<number | null>(null);
  let maxHeight = $state<number | null>(null);

  function close() {
    open = false;
  }

  function computePosition() {
    const b = bounds();
    if (anchor.kind === "rect") {
      const rect = anchor.el.getBoundingClientRect();
      if (anchor.mode === "dropdown") {
        const e = positionDropdown(rect, b, { gap, preferredHeight: est.height });
        top = e.top;
        bottom = e.bottom;
        left = rect.left;
        right = null;
        width = matchTriggerWidth ? rect.width : null;
        maxHeight = e.maxHeight;
      } else {
        const e = positionAnchored(rect, b, est, { gap });
        top = e.top;
        bottom = e.bottom;
        left = e.left;
        right = e.right;
        width = null;
        maxHeight = null;
      }
    } else {
      const e = positionAtPoint(anchor.x, anchor.y, b, est, { gap });
      top = e.top;
      bottom = e.bottom;
      left = e.left;
      right = e.right;
      width = null;
      maxHeight = null;
    }
  }

  // Outside-click close. Skip right-click when configured (Menu's
  // contextmenu coordination). Trigger element (when provided) is
  // excluded so the trigger button can act as a real toggle without
  // close/re-open races.
  $effect(() => {
    if (!open) return;
    computePosition();

    function onDown(e: PointerEvent) {
      if (!closeOnRightClickOutside && e.button === 2) return;
      const target = e.target as Node;
      if (popoverEl?.contains(target)) return;
      if (trigger?.contains(target)) return;
      close();
    }
    document.addEventListener("pointerdown", onDown);

    let cleanup: (() => void) | undefined;
    if (repositionOnScrollResize) {
      const onRelayout = () => computePosition();
      // Capture-phase scroll listener catches scroll events from any
      // descendant (e.g. an Overlay's ScrollArea).
      window.addEventListener("scroll", onRelayout, { capture: true, passive: true });
      window.addEventListener("resize", onRelayout);
      cleanup = () => {
        window.removeEventListener("scroll", onRelayout, { capture: true });
        window.removeEventListener("resize", onRelayout);
      };
    }

    return () => {
      document.removeEventListener("pointerdown", onDown);
      cleanup?.();
    };
  });

  // Build style from set edges only (filter nulls). Z-index is inline,
  // never as a `z-${n}` Tailwind class; Tailwind purges what it doesn't
  // see.
  const style = $derived(
    [
      `z-index: ${z}`,
      top !== null ? `top: ${top}px` : null,
      bottom !== null ? `bottom: ${bottom}px` : null,
      left !== null ? `left: ${left}px` : null,
      right !== null ? `right: ${right}px` : null,
      width !== null ? `width: ${width}px` : null,
      minWidth !== undefined ? `min-width: ${minWidth}px` : null,
      maxHeight !== null ? `max-height: ${maxHeight}px` : null,
    ]
      .filter(Boolean)
      .join("; "),
  );
</script>

{#if open}
  <div bind:this={popoverEl} use:portal class="fixed" {style}>
    {@render children({ close, maxHeight })}
  </div>
{/if}
