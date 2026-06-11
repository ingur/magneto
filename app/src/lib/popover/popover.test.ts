import { beforeEach, describe, expect, it, vi } from "vitest";

import { getBounds } from "./bounds";
import { menus } from "./menus.svelte";
import { positionAnchored, positionAtPoint, positionDropdown } from "./position";

// Plain rect stand-in (the position helpers only read these fields).
function rect(left: number, top: number, width: number, height: number): DOMRect {
  return {
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
    toJSON() {},
  } as DOMRect;
}

const W = window.innerWidth;
const H = window.innerHeight;
const viewport = rect(0, 0, W, H);

describe("positionDropdown", () => {
  it("drops below when there is room", () => {
    const e = positionDropdown(rect(100, 100, 50, 20), viewport);
    expect(e.top).toBe(124); // bottom(120) + gap(4)
    expect(e.bottom).toBeNull();
    expect(e.maxHeight).toBe(240); // clamped to preferred
  });

  it("flips above when there is no room below", () => {
    const trigger = rect(100, H - 50, 50, 20);
    const e = positionDropdown(trigger, viewport);
    expect(e.top).toBeNull();
    expect(e.bottom).toBe(H - trigger.top + 4); // anchored by opposite edge
    expect(e.maxHeight).toBe(240);
  });

  it("clamps maxHeight to the available space and subtracts the edge margin", () => {
    // bounds.bottom 300; space below = 300 - 120 - gap(4) - edge(16) = 160
    const e = positionDropdown(rect(100, 100, 50, 20), rect(0, 0, W, 300));
    expect(e.top).toBe(124);
    expect(e.maxHeight).toBe(160);
  });
});

describe("positionAnchored", () => {
  const est = { width: 240, height: 160 };

  it("below + left-aligns when the trigger is narrow", () => {
    const e = positionAnchored(rect(100, 100, 50, 20), viewport, est);
    expect(e.top).toBe(124);
    expect(e.bottom).toBeNull();
    expect(e.left).toBe(100); // not enough room left of the right edge → left-align
    expect(e.right).toBeNull();
  });

  it("right-aligns when the menu fits left of the trigger's right edge", () => {
    const trigger = rect(500, 100, 300, 20); // right edge at 800
    const e = positionAnchored(trigger, viewport, est);
    expect(e.right).toBe(W - 800);
    expect(e.left).toBeNull();
  });

  it("flips above by the opposite edge when below has no room", () => {
    const trigger = rect(100, H - 30, 50, 20);
    const e = positionAnchored(trigger, viewport, est);
    expect(e.top).toBeNull();
    expect(e.bottom).toBe(H - trigger.top + 4);
  });
});

describe("positionAtPoint", () => {
  const est = { width: 240, height: 160 };

  it("anchors at the point when both axes have room", () => {
    const e = positionAtPoint(100, 100, viewport, est);
    expect(e.top).toBe(100);
    expect(e.left).toBe(100);
    expect(e.bottom).toBeNull();
    expect(e.right).toBeNull();
  });

  it("flips both axes by the opposite edges near the corner", () => {
    const e = positionAtPoint(W - 10, H - 10, viewport, est);
    expect(e.top).toBeNull();
    expect(e.left).toBeNull();
    expect(e.bottom).toBe(H - (H - 10)); // = 10
    expect(e.right).toBe(W - (W - 10)); // = 10
  });
});

describe("getBounds", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("returns a function source verbatim", () => {
    const r = rect(1, 2, 3, 4);
    expect(getBounds(null, () => r)).toBe(r);
  });

  it("falls back to the viewport with no source or no match", () => {
    const vp = getBounds(null);
    expect(vp.left).toBe(0);
    expect(vp.top).toBe(0);
    expect(vp.width).toBe(W);
    expect(vp.height).toBe(H);
  });

  it("wraps a bare attribute name in [brackets] and resolves closest", () => {
    const box = document.createElement("div");
    box.setAttribute("data-foo", "");
    box.getBoundingClientRect = () => rect(5, 6, 7, 8);
    const inner = document.createElement("span");
    box.appendChild(inner);
    document.body.appendChild(box);
    expect(getBounds(inner, "data-foo").left).toBe(5);
  });

  it("uses a punctuated source as a full selector", () => {
    const dlg = document.createElement("div");
    dlg.setAttribute("role", "dialog");
    dlg.getBoundingClientRect = () => rect(9, 9, 1, 1);
    const inner = document.createElement("span");
    dlg.appendChild(inner);
    document.body.appendChild(dlg);
    expect(getBounds(inner, '[role="dialog"]').left).toBe(9);
  });

  it("falls back to the viewport when the selector matches nothing", () => {
    const inner = document.createElement("span");
    document.body.appendChild(inner);
    expect(getBounds(inner, "data-nope").width).toBe(W);
  });
});

describe("menus coordinator", () => {
  beforeEach(() => {
    menus.closer = null;
    document.body.innerHTML = "";
  });

  it("register sets the closer and isAnyOpen; unregister clears it", () => {
    const c = vi.fn();
    const un = menus.register(c);
    expect(menus.isAnyOpen).toBe(true);
    un();
    expect(menus.isAnyOpen).toBe(false);
  });

  it("a stale unregister only clears if it still owns the slot", () => {
    const a = vi.fn();
    const unA = menus.register(a);
    const b = vi.fn();
    const unB = menus.register(b);
    unA(); // a no longer owns the slot → no-op
    expect(menus.isAnyOpen).toBe(true);
    menus.closeAny();
    expect(b).toHaveBeenCalledTimes(1);
    expect(a).not.toHaveBeenCalled();
    unB();
    expect(menus.isAnyOpen).toBe(false);
  });

  it("installContextHandler closes any open menu on a plain right-click", () => {
    const remove = menus.installContextHandler();
    const c = vi.fn();
    menus.register(c);
    const el = document.createElement("div");
    document.body.appendChild(el);
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    expect(c).toHaveBeenCalledTimes(1);
    remove();
  });

  it("leaves menus alone on a trigger-area or in-menu right-click", () => {
    const remove = menus.installContextHandler();
    for (const attr of ["data-menu-trigger-area", "data-menu-instance"]) {
      menus.closer = null;
      const c = vi.fn();
      menus.register(c);
      const box = document.createElement("div");
      box.setAttribute(attr, "");
      const child = document.createElement("button");
      box.appendChild(child);
      document.body.appendChild(box);
      child.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
      expect(c).not.toHaveBeenCalled();
    }
    remove();
  });

  it("the remover detaches the handler", () => {
    const remove = menus.installContextHandler();
    remove();
    const c = vi.fn();
    menus.register(c);
    const el = document.createElement("div");
    document.body.appendChild(el);
    el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    expect(c).not.toHaveBeenCalled();
  });
});
