// Mouse rubber-band selection for the browser list. A left press anywhere in
// the list arms the gesture; crossing DRAG_THRESHOLD activates the band, so
// clicks, double-clicks and row buttons keep their meaning. While active, the
// banded rows are recomputed from live row geometry every frame (the list can
// re-sort mid-drag) and handed to nav.replaceSelection; nav stays the sole
// selection owner. The anchor lives in content space (scroll-compensated) so
// edge auto-scroll keeps the band pinned to rows; the rendered `rect` is
// viewport-local and the host element clips it.

import { kb } from "@/lib/kb/kb.svelte";
import { nav } from "@/torrents/nav.svelte";
import { bandRect, bandSelection, edgeVelocity, intersects, type BandRect } from "./drag-select";

// GTK's default drag threshold. Activation suppresses the release click, so
// a hair-trigger would eat real clicks.
const DRAG_THRESHOLD = 8;

export class DragSelect {
  // Band rect for rendering, viewport-local. null while idle or merely armed.
  rect = $state<BandRect | null>(null);

  #viewport: HTMLElement | null = null;
  #captureEl: Element | null = null;
  #anchorX = 0; // content coords
  #anchorY = 0;
  #pointerX = 0; // client coords
  #pointerY = 0;
  #pointerId = -1;
  #armed = false;
  #additive = false;
  #base: ReadonlySet<string> = new Set();
  #raf = 0;

  get active(): boolean {
    return this.rect !== null;
  }

  onpointerdown = (e: PointerEvent) => {
    if (e.button !== 0 || e.pointerType !== "mouse" || this.active) return;
    const target = e.target as Element;
    // The scrollbar thumb owns its drag; buttons keep their click semantics,
    // except the mark square: bands may start there and inherit its ADDITIVE
    // semantics (a replace band from a jittery checkbox click would clobber
    // existing marks).
    if (target.closest("[data-scroll-thumb]")) return;
    const button = target.closest("button");
    const onMarkSquare = button?.hasAttribute("data-mark-button") ?? false;
    if (button && !onMarkSquare) return;
    const viewport = (e.currentTarget as Element).querySelector<HTMLElement>(
      "[data-scroll-viewport]",
    );
    if (!viewport) return;
    const vp = viewport.getBoundingClientRect();
    this.#viewport = viewport;
    this.#anchorX = e.clientX - vp.left;
    this.#anchorY = e.clientY - vp.top + viewport.scrollTop;
    this.#pointerX = e.clientX;
    this.#pointerY = e.clientY;
    this.#pointerId = e.pointerId;
    this.#additive = onMarkSquare || e.ctrlKey || e.metaKey;
    this.#armed = true;
  };

  onpointermove = (e: PointerEvent) => {
    if ((!this.#armed && !this.active) || e.pointerId !== this.#pointerId) return;
    this.#pointerX = e.clientX;
    this.#pointerY = e.clientY;
    if (!this.#armed) return;
    // Release happened where we couldn't see it (outside the list): disarm.
    if (!(e.buttons & 1)) {
      this.#armed = false;
      return;
    }
    const viewport = this.#viewport!;
    const vp = viewport.getBoundingClientRect();
    const dx = e.clientX - vp.left - this.#anchorX;
    const dy = e.clientY - vp.top + viewport.scrollTop - this.#anchorY;
    if (Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    // Activate. Capture starts here, not at arming: capturing every press
    // would retarget the synthesized click/dblclick away from rows and break
    // double-click-to-play.
    this.#armed = false;
    this.#base = new Set(nav.selection);
    this.#captureEl = e.currentTarget as Element;
    this.#captureEl.setPointerCapture(e.pointerId);
    this.#raf = requestAnimationFrame(this.#frame);
  };

  onpointerup = (e: PointerEvent) => {
    if (e.pointerId !== this.#pointerId) return;
    this.#armed = false;
    if (!this.active) return;
    this.#end();
    // Swallow the click/dblclick synthesized from this release so a band
    // ending over a row can't double-click-activate it.
    suppressClicksOnce();
  };

  // System-level interruption (window loses the pointer, capture revoked):
  // commit whatever is selected and stop cleanly.
  onpointercancel = () => {
    this.#armed = false;
    if (this.active) this.#end();
  };
  onlostpointercapture = () => {
    if (this.active) this.#end();
  };

  /** Escape: drop the band and restore the selection from drag start.
   *  Returns whether a band was active (escape falls through otherwise). */
  cancel(): boolean {
    this.#armed = false;
    if (!this.active) return false;
    nav.replaceSelection(this.#base);
    this.#end();
    return true;
  }

  // Per-frame while active: edge auto-scroll, then recompute band + selection
  // against current row geometry. Unconditional recompute keeps this simple;
  // the list is never virtualized and row counts stay modest.
  #frame = () => {
    const viewport = this.#viewport;
    if (!viewport) return;
    const vp = viewport.getBoundingClientRect();
    const v = edgeVelocity(this.#pointerY, vp.top, vp.bottom);
    if (v !== 0) viewport.scrollTop += v; // assignment self-clamps
    this.#recompute(viewport, vp);
    this.#raf = requestAnimationFrame(this.#frame);
  };

  #recompute(viewport: HTMLElement, vp: DOMRect) {
    const scrollTop = viewport.scrollTop;
    // Pointer clamped to the viewport box, then into content space.
    const px = Math.min(Math.max(this.#pointerX, vp.left), vp.right) - vp.left;
    const py = Math.min(Math.max(this.#pointerY, vp.top), vp.bottom) - vp.top + scrollTop;
    const band = bandRect(this.#anchorX, this.#anchorY, px, py);

    const hits: string[] = [];
    for (const node of viewport.querySelectorAll<HTMLElement>("[data-kb-id]")) {
      const r = node.getBoundingClientRect();
      const row = {
        left: r.left - vp.left,
        top: r.top - vp.top + scrollTop,
        width: r.width,
        height: r.height,
      };
      if (intersects(band, row)) hits.push(node.dataset.kbId!);
    }

    nav.replaceSelection(bandSelection(this.#base, hits, this.#additive));
    // Leading edge: the last banded row when dragging down, the first when up.
    if (hits.length > 0) kb.setCursor(py >= this.#anchorY ? hits[hits.length - 1] : hits[0]);

    this.rect = {
      left: band.left,
      top: band.top - scrollTop,
      width: band.width,
      height: band.height,
    };
  }

  // Idempotent: the explicit pointerup path releases capture, which fires
  // lostpointercapture right back into a no-op.
  #end() {
    cancelAnimationFrame(this.#raf);
    try {
      this.#captureEl?.releasePointerCapture(this.#pointerId);
    } catch {
      // capture already gone
    }
    this.#captureEl = null;
    this.#viewport = null;
    this.#base = new Set();
    this.rect = null;
  }
}

// One-shot capture-phase swallow of the click/dblclick a band release
// synthesizes. Removed on the next macrotask; both events dispatch within
// the same turn as the pointerup.
function suppressClicksOnce() {
  const swallow = (e: Event) => {
    e.stopPropagation();
    e.preventDefault();
  };
  window.addEventListener("click", swallow, true);
  window.addEventListener("dblclick", swallow, true);
  setTimeout(() => {
    window.removeEventListener("click", swallow, true);
    window.removeEventListener("dblclick", swallow, true);
  }, 0);
}
