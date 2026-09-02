import { beforeEach, describe, expect, it, vi } from "vitest";

import { cheatsheetFor } from "./cheatsheet";
import { displayKey, kb, kbItem, type Binding, type KbItemInit } from "./kb.svelte";

beforeEach(() => {
  kb.stack = [];
  kb.cursorTick = 0;
  kb.kbActive = false;
  document.body.innerHTML = "";
});

// A KeyboardEvent stand-in: full control over the fields dispatch reads,
// without jsdom's KeyboardEvent-init quirks (isComposing, keyCode).
function ev(init: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
  return {
    ctrlKey: false,
    altKey: false,
    metaKey: false,
    shiftKey: false,
    repeat: false,
    isComposing: false,
    keyCode: 0,
    target: null,
    preventDefault: () => {},
    stopPropagation: () => {},
    ...init,
  } as unknown as KeyboardEvent;
}

function mountLayer(bindings: Record<string, Binding> = {}, cursorId: string | null = null) {
  const handle = kb.push({ name: "test", bindings, cursorId });
  const el = document.createElement("div");
  el.setAttribute("data-kb-layer", String(handle.id));
  document.body.appendChild(el);
  return { handle, el };
}

function addItem(el: HTMLElement, init: KbItemInit) {
  const node = document.createElement("button");
  el.appendChild(node);
  const action = kbItem(node, init);
  return { node, action };
}

describe("dispatch / canonicalKey", () => {
  it("lowercases letters so CapsLock/Shift still match (J → j)", () => {
    const run = vi.fn();
    mountLayer({ j: { run } });
    expect(kb.dispatch(ev({ key: "J" }))).toBe(true);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it("dispatches a shifted uppercase letter distinctly when bound (g vs G)", () => {
    const g = vi.fn();
    const G = vi.fn();
    mountLayer({ g: { run: g }, G: { run: G } });
    kb.dispatch(ev({ key: "G", shiftKey: true }));
    kb.dispatch(ev({ key: "g" }));
    expect(G).toHaveBeenCalledTimes(1);
    expect(g).toHaveBeenCalledTimes(1);
  });

  it("falls back to the lowercase binding when the uppercase form is unbound", () => {
    const x = vi.fn();
    const save = vi.fn();
    mountLayer({ x: { run: x }, "ctrl+s": { run: save } });
    kb.dispatch(ev({ key: "X", shiftKey: true }));
    kb.dispatch(ev({ key: "S", ctrlKey: true, shiftKey: true }));
    expect(x).toHaveBeenCalledTimes(1);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("a bare modifier press neither dispatches nor enables keyboard mode", () => {
    mountLayer({ j: { run: vi.fn() } });
    expect(kb.dispatch(ev({ key: "Control", ctrlKey: true }))).toBe(false);
    expect(kb.dispatch(ev({ key: "Shift", shiftKey: true }))).toBe(false);
    expect(kb.dispatch(ev({ key: "Alt", altKey: true }))).toBe(false);
    expect(kb.dispatch(ev({ key: "Meta", metaKey: true }))).toBe(false);
    expect(kb.kbActive).toBe(false);
  });

  it("a bare modifier press doesn't drop keyboard mode either", () => {
    mountLayer({ j: { run: vi.fn() } });
    kb.dispatch(ev({ key: "j" }));
    expect(kb.kbActive).toBe(true);
    kb.dispatch(ev({ key: "Control", ctrlKey: true }));
    expect(kb.kbActive).toBe(true);
  });

  it("a real key enables keyboard mode; a modifier combo still dispatches", () => {
    const run = vi.fn();
    mountLayer({ "ctrl+a": { run } });
    expect(kb.kbActive).toBe(false);
    expect(kb.dispatch(ev({ key: "a", ctrlKey: true }))).toBe(true);
    expect(run).toHaveBeenCalledTimes(1);
    expect(kb.kbActive).toBe(true);
  });

  it("leaves punctuation untouched (/, ?)", () => {
    const slash = vi.fn();
    const q = vi.fn();
    mountLayer({ "/": { run: slash }, "?": { run: q } });
    kb.dispatch(ev({ key: "/" }));
    kb.dispatch(ev({ key: "?" }));
    expect(slash).toHaveBeenCalledTimes(1);
    expect(q).toHaveBeenCalledTimes(1);
  });

  it("aliases arrows and tab/shift-tab to hjkl", () => {
    const j = vi.fn();
    const k = vi.fn();
    const h = vi.fn();
    const l = vi.fn();
    mountLayer({ j: { run: j }, k: { run: k }, h: { run: h }, l: { run: l } });
    kb.dispatch(ev({ key: "ArrowDown" }));
    kb.dispatch(ev({ key: "ArrowUp" }));
    kb.dispatch(ev({ key: "ArrowLeft" }));
    kb.dispatch(ev({ key: "ArrowRight" }));
    kb.dispatch(ev({ key: "Tab" }));
    kb.dispatch(ev({ key: "Tab", shiftKey: true }));
    expect(j).toHaveBeenCalledTimes(2);
    expect(k).toHaveBeenCalledTimes(2);
    expect(h).toHaveBeenCalledTimes(1);
    expect(l).toHaveBeenCalledTimes(1);
  });

  it("passes plain keys through while typing, but ctrl-combos reach kb", () => {
    const j = vi.fn();
    const save = vi.fn();
    mountLayer({ j: { run: j }, "ctrl+s": { run: save } });
    const input = document.createElement("input");
    expect(kb.dispatch(ev({ key: "j", target: input }))).toBe(false);
    expect(j).not.toHaveBeenCalled();
    expect(kb.dispatch(ev({ key: "s", ctrlKey: true, target: input }))).toBe(true);
    expect(save).toHaveBeenCalledTimes(1);
  });

  it("always swallows Tab (preventDefault) even inside a text input", () => {
    mountLayer({ j: { run: () => {} } });
    const input = document.createElement("input");
    const preventDefault = vi.fn();
    expect(kb.dispatch(ev({ key: "Tab", target: input, preventDefault }))).toBe(false);
    expect(preventDefault).toHaveBeenCalled();
  });

  it("capture 'all' swallows unbound keys; 'matched' lets them pass", () => {
    const { handle } = mountLayer();
    kb.setCapture(handle, "all");
    const pd = vi.fn();
    expect(kb.dispatch(ev({ key: "z", preventDefault: pd }))).toBe(true);
    expect(pd).toHaveBeenCalled();
    kb.setCapture(handle, "matched");
    expect(kb.dispatch(ev({ key: "z" }))).toBe(false);
  });

  it("runs movement repeats but swallows non-movement repeats", () => {
    const j = vi.fn();
    const x = vi.fn();
    mountLayer({ j: { run: j }, x: { run: x } });
    const pd = vi.fn();
    expect(kb.dispatch(ev({ key: "x", repeat: true, preventDefault: pd }))).toBe(true);
    expect(x).not.toHaveBeenCalled();
    expect(pd).toHaveBeenCalled();
    kb.dispatch(ev({ key: "j", repeat: true }));
    expect(j).toHaveBeenCalledTimes(1);
  });

  it("ignores IME composition (isComposing / keyCode 229)", () => {
    const j = vi.fn();
    mountLayer({ j: { run: j } });
    expect(kb.dispatch(ev({ key: "j", isComposing: true }))).toBe(false);
    expect(kb.dispatch(ev({ key: "j", keyCode: 229 }))).toBe(false);
    expect(j).not.toHaveBeenCalled();
  });

  it("logs and survives a throwing binding", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const next = vi.fn();
    mountLayer({
      x: {
        run: () => {
          throw new Error("boom");
        },
      },
      j: { run: next },
    });
    expect(() => kb.dispatch(ev({ key: "x" }))).not.toThrow();
    expect(spy).toHaveBeenCalled();
    kb.dispatch(ev({ key: "j" }));
    expect(next).toHaveBeenCalledTimes(1);
    spy.mockRestore();
  });
});

describe("displayKey", () => {
  it("renames special keys and splits modifiers", () => {
    expect(displayKey("enter")).toBe("↵");
    expect(displayKey("escape")).toBe("esc");
    expect(displayKey("space")).toBe("␣");
    expect(displayKey("ctrl+space")).toBe("ctrl+␣");
    expect(displayKey("a")).toBe("a");
  });
});

describe("hints", () => {
  it("groups same-label keys in declaration order, priority = max", () => {
    mountLayer({
      j: { label: "navigate", run() {} },
      k: { label: "navigate", priority: 80, run() {} },
      x: { label: "delete", run() {} },
      z: { run() {} },
    });
    expect(kb.hints).toEqual([
      { key: "j/k", label: "navigate", priority: 80, run: undefined },
      { key: "x", label: "delete", priority: 50, run: undefined },
    ]);
  });

  it("exposes run only when the group's first binding is clickable", () => {
    const run = () => {};
    mountLayer({ "?": { label: "help", clickable: true, run } });
    expect(kb.hints[0].run).toBe(run);
  });
});

describe("cheatsheetFor", () => {
  it("groups by category, merges descriptions, expands arrow aliases, no sort", () => {
    const { handle } = mountLayer({
      j: { label: "navigate", category: "Move", run() {} },
      k: { label: "navigate", category: "Move", run() {} },
      l: { description: "enter folder", category: "Move", run() {} },
      x: { label: "delete", category: "Actions", run() {} },
    });
    expect(cheatsheetFor(kb.findById(handle.id))).toEqual([
      {
        category: "Move",
        entries: [
          { description: "navigate", keys: ["j", "↓", "k", "↑"] },
          { description: "enter folder", keys: ["l", "→"] },
        ],
      },
      { category: "Actions", entries: [{ description: "delete", keys: ["x"] }] },
    ]);
  });

  it("returns [] for a null layer", () => {
    expect(cheatsheetFor(null)).toEqual([]);
  });
});

describe("layer lifecycle", () => {
  it("push / findById / remove, and mutators no-op after pop", () => {
    const h = kb.push({ name: "a", bindings: {} });
    expect(kb.findById(h.id)).not.toBeNull();
    expect(kb.isActive(h)).toBe(true);
    kb.remove(h);
    expect(kb.findById(h.id)).toBeNull();
    expect(() => kb.setBindings(h, { j: { run() {} } })).not.toThrow();
    expect(() => kb.setCursorOn(h, "x")).not.toThrow();
  });
});

describe("cursor movement", () => {
  it("clamps at both ends", () => {
    const { handle, el } = mountLayer();
    addItem(el, { id: "a" });
    addItem(el, { id: "b" });
    addItem(el, { id: "c" });
    kb.setCursorOn(handle, "a");
    kb.prev();
    expect(kb.cursor()).toBe("a");
    kb.next();
    kb.next();
    expect(kb.cursor()).toBe("c");
    kb.next();
    expect(kb.cursor()).toBe("c");
  });

  it("falls back to the first item when the cursor is stale", () => {
    const { handle, el } = mountLayer();
    addItem(el, { id: "a" });
    addItem(el, { id: "b" });
    kb.setCursorOn(handle, "gone");
    expect(kb.cursor()).toBe("a");
  });

  it("first/last jump to the ends and bump cursorTick", () => {
    const { handle, el } = mountLayer();
    addItem(el, { id: "a" });
    addItem(el, { id: "b" });
    addItem(el, { id: "c" });
    kb.setCursorOn(handle, "b");
    const start = kb.cursorTick;
    kb.first();
    expect(kb.cursor()).toBe("a");
    kb.last();
    expect(kb.cursor()).toBe("c");
    expect(kb.cursorTick).toBe(start + 2);
  });

  it("first/last with a group land on that group's ends, skipping others", () => {
    const { handle, el } = mountLayer();
    addItem(el, { id: "r1", group: "rows" });
    addItem(el, { id: "r2", group: "rows" });
    addItem(el, { id: "save", group: "buttons" });
    kb.setCursorOn(handle, "save");
    kb.last("rows");
    expect(kb.cursor()).toBe("r2");
    kb.first("rows");
    expect(kb.cursor()).toBe("r1");
  });

  it("setCursorOn bumps cursorTick; mouse setCursor does not", () => {
    const { handle, el } = mountLayer();
    addItem(el, { id: "a" });
    addItem(el, { id: "b" });
    const start = kb.cursorTick;
    kb.setCursorOn(handle, "b");
    expect(kb.cursorTick).toBe(start + 1);
    const mid = kb.cursorTick;
    kb.setCursor("a");
    expect(kb.cursorTick).toBe(mid);
    expect(kb.cursor()).toBe("a");
  });

  it("navigates in DOM order even when items mount out of order", () => {
    const { handle, el } = mountLayer();
    const nb = document.createElement("button");
    el.appendChild(nb);
    kbItem(nb, { id: "b" });
    const na = document.createElement("button");
    el.insertBefore(na, nb);
    kbItem(na, { id: "a" });
    kb.setCursorOn(handle, "a");
    kb.next();
    expect(kb.cursor()).toBe("b");
  });

  it("clamps the cursor to the next item when the cursored item unmounts", () => {
    const { handle, el } = mountLayer();
    const a = addItem(el, { id: "a" });
    addItem(el, { id: "b" });
    kb.setCursorOn(handle, "a");
    a.action.destroy?.();
    a.node.remove();
    expect(kb.cursor()).toBe("b");
  });

  it("clamps in DOM order, not mount order, when the cursored item unmounts", () => {
    const { handle, el } = mountLayer();
    // Mount c, b, a but lay them out a, b, c.
    const nc = document.createElement("button");
    el.appendChild(nc);
    kbItem(nc, { id: "c" });
    const nb = document.createElement("button");
    el.insertBefore(nb, nc);
    const b = kbItem(nb, { id: "b" });
    const na = document.createElement("button");
    el.insertBefore(na, nb);
    kbItem(na, { id: "a" });
    kb.setCursorOn(handle, "b");
    nb.remove();
    b.destroy?.();
    expect(kb.cursor()).toBe("a");
  });
});

describe("cursor visual (data-kb-cursor)", () => {
  it("shows iff kbActive and is the layer's cursor; pointer hides, keypress reveals", () => {
    const { handle, el } = mountLayer({ j: { run() {} } });
    const { node } = addItem(el, { id: "a" });
    addItem(el, { id: "b" });
    kb.setCursorOn(handle, "a");
    expect(node.hasAttribute("data-kb-cursor")).toBe(false);
    kb.dispatch(ev({ key: "z" })); // unbound: flips to kb mode without moving
    expect(kb.kbActive).toBe(true);
    expect(node.hasAttribute("data-kb-cursor")).toBe(true);
    kb.setKbActive(false);
    expect(node.hasAttribute("data-kb-cursor")).toBe(false);
  });

  it("moves the visual to the new cursor node on a move", () => {
    const { handle, el } = mountLayer({ j: { run() {} } });
    const { node: na } = addItem(el, { id: "a" });
    const { node: nb } = addItem(el, { id: "b" });
    kb.setCursorOn(handle, "a");
    kb.setKbActive(true);
    expect(na.hasAttribute("data-kb-cursor")).toBe(true);
    kb.next();
    expect(na.hasAttribute("data-kb-cursor")).toBe(false);
    expect(nb.hasAttribute("data-kb-cursor")).toBe(true);
  });

  it("keeps the cursor visual on a background layer", () => {
    const a = mountLayer();
    const { node } = addItem(a.el, { id: "a1" });
    kb.setCursorOn(a.handle, "a1");
    kb.setKbActive(true);
    expect(node.hasAttribute("data-kb-cursor")).toBe(true);
    mountLayer(); // push a second layer on top
    expect(node.hasAttribute("data-kb-cursor")).toBe(true);
  });
});

describe("kbItem layer resolution (portal hazard)", () => {
  it("registers on the nearest data-kb-layer ancestor, not the active layer", () => {
    const outer = mountLayer();
    const inner = mountLayer(); // inner is active (top of stack)
    // A subtree tagged as inner's layer, physically nested inside outer's
    // element; mirrors a menu rendered through a portal.
    const portal = document.createElement("div");
    portal.setAttribute("data-kb-layer", String(inner.handle.id));
    outer.el.appendChild(portal);
    const node = document.createElement("button");
    portal.appendChild(node);
    kbItem(node, { id: "x" });
    expect(kb.findById(inner.handle.id)!.items.has("x")).toBe(true);
    expect(kb.findById(outer.handle.id)!.items.has("x")).toBe(false);
  });
});
