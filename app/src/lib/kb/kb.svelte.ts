// Keyboard layer system.
//
// Concepts:
//   * Layer        : a named scope on a stack. Owns a keymap (`bindings`)
//                    and an items map with a `cursorId` pointing at the
//                    kb cursor. Top of stack receives all key events.
//                    Cursor is preserved while the layer is paused (deeper
//                    in stack) so reopening it restores where the user was.
//   * Binding      : `{ label?, run }`. Bindings with `label` show in
//                    StatusBar via `kb.hints`; bindings without are
//                    invisible (think: internal helpers, discoverable in
//                    the help overlay).
//   * Item         : `{ id, activate?, group? }`. Items self-register via
//                    the `kbItem` Svelte action, in DOM order. Activation
//                    is colocated with items, not bindings: `kb.activate()`
//                    runs the cursored item's activate; `kb.activate(id)`
//                    runs a specific item's activate (mouse paths). Layers
//                    bind enter/space (etc.) to that single primitive.
//                    `group` is optional metadata: layers ask semantic
//                    questions like `kb.cursorGroup() === 'buttons'`
//                    instead of doing string equality on cursor ids.
//   * Handle       : opaque `{ id }` returned by `kb.push`. Per-layer
//                    mutators (`setBindings`, `setCapture`, `setCursorOn`,
//                    `remove`, `isActive`) take a handle, never a Layer
//                    ref. Internally each call resolves to the $state
//                    proxy so writes notify Svelte signals. Handing out a
//                    Layer ref would let callers mutate the unwrapped
//                    object and silently desync render reads.
//
// One global keydown listener at module load. Matched bindings always
// preventDefault + stopPropagation, so binding `'/'` blocks browser quick-
// find, `'space'` blocks scroll, etc. Components never call
// preventDefault themselves, that conflation is what we're avoiding.
// Layers can opt into `capture: 'all'` to swallow unbound keys too
// (modal lockdown). Default is `matched`: unbound keys pass through.
//
// Text-input awareness: when the active element is a text input/textarea/
// contenteditable, kb stays out of the way entirely: every plain key
// (including Escape) reaches the input. Components decide what Esc means
// inside their input (typically: blur). After blurring, Esc reaches kb
// again and the layer's escape binding fires. Modifier-bearing combos
// (ctrl/alt/meta) always reach kb regardless of focus, so app shortcuts
// like Ctrl+S still work while typing.
//
// The kb cursor visual (the `data-kb-cursor` attribute) is applied
// imperatively by the engine at the moment the cursor or input mode
// changes (touching only the old and new nodes) rather than via a
// per-item reactive effect. With N navigable items a per-item effect would
// re-run all N on every cursor move; the imperative path is O(1) per move.

export type Binding = {
  // StatusBar hint label. Bindings with a label show in the StatusBar
  // grouped by label; bindings without are invisible there. Bindings
  // also fall back to `label` for the help cheatsheet description when
  // `description` is unset.
  label?: string;
  // StatusBar drop order when space is tight. Lower drops first; default
  // 50. Set lower (~30) for well-known navigation; higher (~80) for
  // discovery/exit hints; 90+ for "the user is actively engaged with
  // this" (search input, etc.).
  priority?: number;
  // Help cheatsheet section. Bindings without `category` fall under a
  // 'General' bucket. StatusBar doesn't read this.
  category?: string;
  // Help cheatsheet row text. Falls back to `label` when unset. Lets
  // bindings appear in the cheatsheet without cluttering the StatusBar
  // (e.g. h/l/arrows/backspace, directional aliases that don't need a
  // hint badge but are worth listing for newcomers). Bindings with
  // neither `label` nor `description` are invisible everywhere, truly
  // internal helpers.
  description?: string;
  // Opt-in: render the StatusBar hint as a click target (clicking the
  // hint runs `run`, same as pressing the key). Reserve for global chrome
  // actions where there's no cursor-targeting question: help, search,
  // close, cancel. Cursor-targeted actions (p/d/s/x/m, j/k navigate)
  // intentionally don't get this. Clicking small statusbar text to
  // act on a row halfway across the screen is a disconnected affordance,
  // and `x delete` clicking would be a destructive surface.
  clickable?: boolean;
  run: () => void;
};

export type Hint = {
  key: string;
  label: string;
  priority: number;
  // Present only when the underlying binding declared `clickable: true`.
  // StatusBar widgets render hints with `run` as click targets; hints
  // without stay as plain text. See Binding.clickable for the rationale.
  run?: () => void;
};

export type Capture = "matched" | "all";

export type Item = {
  id: string;
  activate?: () => void;
  group?: string;
  // The DOM node this item lives on. Set by the kbItem action; lets kb
  // sort items by document position so j/k always means "next in the
  // user's visual list", not "next in mount order" (which drifts when
  // a parent reorders its children, e.g. on a sort-mode change).
  node?: HTMLElement;
};

export type Layer = {
  id: number;
  name: string;
  bindings: Record<string, Binding>;
  capture: Capture;
  // Items keyed by id (insertion order = mount order). A Map keeps
  // add/remove O(1); a filter-swap of a large list (the `/` filter flow)
  // would be O(N²) with array findIndex+splice. Visual order is always
  // re-derived from the DOM via orderedItems().
  items: Map<string, Item>;
  cursorId: string | null;
};

export type LayerInit = {
  name: string;
  bindings: Record<string, Binding>;
  capture?: Capture;
  cursorId?: string | null;
};

// Opaque handle returned by kb.push. All mutators take a handle (not a
// Layer ref) so kb internally resolves to the $state proxy each call.
// Direct property writes on a captured Layer ref bypass Svelte 5 signal
// notifications, so we never hand a ref out to begin with.
export type LayerHandle = { id: number };

const MOVEMENT_KEYS = new Set(["j", "k", "h", "l"]);

// Modifier keys by their KeyboardEvent.key name. Pressed alone they never
// match a binding and must not flip kbActive: a held modifier is usually
// the start of a mouse gesture (ctrl+drag, ctrl+scroll zoom) and the cursor
// ring would flash mid-gesture. Real combos (ctrl+a) dispatch normally.
const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "AltGraph"]);

class KB {
  stack = $state<Layer[]>([]);
  nextLayerId = 1;

  // Counter that increments on kb-driven cursor moves (next/prev) AND
  // programmatic landings (setCursorOn). Mouse setCursor doesn't bump
  // it: mouse clicks point at where the user is already looking, so pulling
  // the row out from under them is jarring. Scroll-follow effects
  // subscribe to this to know "the cursor moved by something other than
  // a mouse click; bring it into view."
  cursorTick = $state(0);

  // Tracks the user's current input mode: true while keyboard is driving,
  // false the moment a pointer event lands. The data-kb-cursor attribute
  // is gated on this: mouse-only sessions never see the cursor visual;
  // using kb at any point reveals it; reaching for the mouse hides it
  // again. Cursor state is still tracked internally regardless (clicks
  // move it via kbItem's pointerdown), so the moment kbActive flips back
  // to true on the next keypress, the cursor picks up wherever the user
  // last interacted. Write through setKbActive so the visual stays synced.
  kbActive = $state(false);

  get active(): Layer | null {
    return this.stack.at(-1) ?? null;
  }

  isActive(handle: LayerHandle): boolean {
    return this.active?.id === handle.id;
  }

  // Hints group bindings by label, joining their keys with "/", so two
  // bindings declared as `j: { label: 'navigate' }` and `k: { label:
  // 'navigate' }` collapse to a single "j/k navigate" hint. Bindings
  // without a label are hidden by design (use ? to discover them).
  // Group order = first-seen declaration order; key order within a group
  // matches declaration order too. Priority of a group is the highest
  // priority among its constituent bindings, the conservative choice
  // (a group survives as long as any of its keys would).
  get hints(): Hint[] {
    const layer = this.active;
    if (!layer) return [];
    const groups: {
      label: string;
      keys: string[];
      priority: number;
      run: (() => void) | undefined;
    }[] = [];
    const indexByLabel = new Map<string, number>();
    for (const [key, b] of Object.entries(layer.bindings)) {
      if (!b.label) continue;
      const p = b.priority ?? 50;
      let i = indexByLabel.get(b.label);
      if (i === undefined) {
        i = groups.length;
        indexByLabel.set(b.label, i);
        // First binding decides clickability for the whole group (see
        // Binding.clickable for the rationale). If it opted in, run is
        // exposed; otherwise the hint stays informational.
        groups.push({
          label: b.label,
          keys: [],
          priority: p,
          run: b.clickable ? b.run : undefined,
        });
      }
      groups[i].keys.push(displayKey(key));
      if (p > groups[i].priority) groups[i].priority = p;
    }
    return groups.map((g) => ({
      key: g.keys.join("/"),
      label: g.label,
      priority: g.priority,
      run: g.run,
    }));
  }

  push(init: LayerInit): LayerHandle {
    const id = this.nextLayerId++;
    this.stack.push({
      id,
      name: init.name,
      bindings: init.bindings,
      capture: init.capture ?? "matched",
      items: new Map(),
      cursorId: init.cursorId ?? null,
    });
    if (import.meta.env.DEV) warnBindings(init.bindings);
    return { id };
  }

  remove(handle: LayerHandle) {
    for (let i = this.stack.length - 1; i >= 0; i--) {
      if (this.stack[i].id === handle.id) {
        this.stack.splice(i, 1);
        return;
      }
    }
  }

  layer(name: string): Layer | null {
    for (let i = this.stack.length - 1; i >= 0; i--) {
      if (this.stack[i].name === name) return this.stack[i];
    }
    return null;
  }

  // Resolve a handle to the $state proxy in the stack. All per-layer
  // mutators go through this so writes hit the proxy (signal-tracked),
  // not a captured original ref. Returns null if the handle has been
  // popped (no-op for callers).
  findById(id: number): Layer | null {
    for (let i = this.stack.length - 1; i >= 0; i--) {
      if (this.stack[i].id === id) return this.stack[i];
    }
    return null;
  }

  private resolve(handle: LayerHandle): Layer | null {
    return this.findById(handle.id);
  }

  setBindings(handle: LayerHandle, bindings: Record<string, Binding>) {
    const layer = this.resolve(handle);
    if (layer) {
      layer.bindings = bindings;
      if (import.meta.env.DEV) warnBindings(bindings);
    }
  }

  setCapture(handle: LayerHandle, capture: Capture) {
    const layer = this.resolve(handle);
    if (layer) layer.capture = capture;
  }

  // Single cursor-write path. Updates cursorId AND imperatively diffs the
  // data-kb-cursor attribute on the old/new nodes (O(1)). Does NOT bump
  // cursorTick: callers that should (next/prev/setCursorOn) bump it
  // themselves; mouse setCursor must not. Applies the new node's attribute
  // for any layer (incl. background ones) while kbActive, matching the
  // visual rule "this is the cursor of its layer, and the user is on kb."
  private writeCursor(layer: Layer, id: string | null) {
    const prev = layer.cursorId;
    if (prev === id) return;
    layer.cursorId = id;
    if (prev !== null) layer.items.get(prev)?.node?.removeAttribute("data-kb-cursor");
    if (this.kbActive && id !== null) {
      layer.items.get(id)?.node?.setAttribute("data-kb-cursor", "");
    }
  }

  // Flip input mode and sync every layer's cursor visual in one pass.
  // Public so the global pointerdown listener can switch to mouse mode.
  setKbActive(active: boolean) {
    if (this.kbActive === active) return;
    this.kbActive = active;
    // Mirror the mode onto the document root so CSS can gate mouse-only
    // affordances (the `mouse:` variant): a stationary pointer shouldn't light a
    // row's hover while the kb cursor is the sole indicator.
    if (typeof document !== "undefined") {
      document.documentElement.toggleAttribute("data-kb-active", active);
    }
    for (const layer of this.stack) {
      if (layer.cursorId === null) continue;
      const node = layer.items.get(layer.cursorId)?.node;
      if (!node) continue;
      if (active) node.setAttribute("data-kb-cursor", "");
      else node.removeAttribute("data-kb-cursor");
    }
  }

  // Set the cursor on a specific layer (not necessarily active). Used by
  // Browser to land cursor on nav. Tolerates an in-flight items swap by
  // clamping to the first valid item rather than silently no-oping. Bumps
  // cursorTick so scroll-follow can react. Distinct from setCursor(id),
  // which targets the active layer only and does NOT bump cursorTick
  // (mouse clicks move the cursor too, and we don't want those to scroll).
  setCursorOn(handle: LayerHandle, id: string | null) {
    const layer = this.resolve(handle);
    if (!layer) return;
    if (id !== null && layer.items.has(id)) {
      this.writeCursor(layer, id);
    } else {
      this.writeCursor(layer, this.orderedItems(layer)[0]?.id ?? null);
    }
    this.cursorTick++;
  }

  // Items sorted by document position. The single source of truth for
  // "what comes before / after this in the user's visual list."
  // Insertion order (the raw items map) drifts as soon as a parent
  // reorders its keyed-each children (sort changes, drag-and-drop, etc.)
  // because the kbItem action only runs on first mount. This getter
  // re-derives order from the live DOM each time it's called.
  private orderedItems(layer: Layer): Item[] {
    return [...layer.items.values()].sort((a, b) => {
      if (!a.node || !b.node) return a.node ? -1 : b.node ? 1 : 0;
      const p = a.node.compareDocumentPosition(b.node);
      if (p & Node.DOCUMENT_POSITION_FOLLOWING) return -1;
      if (p & Node.DOCUMENT_POSITION_PRECEDING) return 1;
      return 0;
    });
  }

  cursor(): string | null {
    return this.active?.cursorId ?? null;
  }

  setCursor(id: string) {
    const l = this.active;
    if (l && l.items.has(id)) this.writeCursor(l, id);
  }

  next() {
    this.move(1);
    this.cursorTick++;
  }

  prev() {
    this.move(-1);
    this.cursorTick++;
  }

  // Jump to the first / last item, the pager motions (g/G, home/end).
  // With `group`, jumps within that group only (Settings/Help land on
  // their rows, never the footer buttons). Bumps cursorTick like
  // next/prev so scroll-follow brings the landing into view.
  first(group?: string) {
    this.jump(group, "first");
  }

  last(group?: string) {
    this.jump(group, "last");
  }

  private jump(group: string | undefined, which: "first" | "last") {
    const l = this.active;
    if (!l) return;
    let items = this.orderedItems(l);
    if (group) items = items.filter((i) => i.group === group);
    const target = which === "first" ? items[0] : items.at(-1);
    if (target) this.writeCursor(l, target.id);
    this.cursorTick++;
  }

  private move(delta: number) {
    const l = this.active;
    if (!l) return;
    const ordered = this.orderedItems(l);
    if (ordered.length === 0) return;
    const idx = l.cursorId ? ordered.findIndex((i) => i.id === l.cursorId) : -1;
    if (idx === -1) {
      // Cursor stale (id not in current items): fall back to first valid
      // item rather than leaving the cursor floating. Without this,
      // findIndex returns -1 and j/k feel broken right after a transition.
      this.writeCursor(l, ordered[0].id);
      return;
    }
    const next = Math.max(0, Math.min(ordered.length - 1, idx + delta));
    this.writeCursor(l, ordered[next].id);
  }

  isCursor(id: string): boolean {
    return this.active?.cursorId === id;
  }

  // The DOM node of the cursored item on a given (or active) layer.
  // Used by scroll-follow helpers. Scoping to the layer's items avoids
  // querySelector('[data-kb-id=...]') collisions across layers (theme
  // names, dropdown values, button ids like 'cancel'/'confirm' could
  // match torrent ids globally). Returns null when no cursor or the
  // item hasn't bound a node yet.
  cursorNode(handle?: LayerHandle): HTMLElement | null {
    const layer = handle ? this.resolve(handle) : this.active;
    if (!layer || layer.cursorId === null) return null;
    return layer.items.get(layer.cursorId)?.node ?? null;
  }

  // The cursor id of a given layer, active or not (setCursorOn's read side).
  cursorOn(handle: LayerHandle): string | null {
    return this.resolve(handle)?.cursorId ?? null;
  }

  // Run an item's activate. With no id, runs the cursored item's activate.
  // With an id, runs that specific item's activate, used by mouse paths
  // (dblclick on a row, click on a menu item) so kb.activate is the
  // single activation primitive for both kb and mouse. The activate may
  // call into the async daemon client; an uncaught throw is logged, not
  // rethrown, so it can't escape the global listener after preventDefault.
  activate(id?: string) {
    const l = this.active;
    if (!l) return;
    const targetId = id ?? l.cursorId;
    if (targetId === null) return;
    const item = l.items.get(targetId);
    if (item?.activate) runSafely(() => item.activate!(), `activate "${targetId}"`);
  }

  // Group of the cursored item (or null). Layers use this to ask
  // semantic questions like `if (kb.cursorGroup() === 'buttons')`
  // instead of maintaining their own set of group ids.
  cursorGroup(): string | null {
    const l = this.active;
    if (!l || l.cursorId === null) return null;
    return l.items.get(l.cursorId)?.group ?? null;
  }

  firstOf(group: string): string | null {
    const l = this.active;
    if (!l) return null;
    return this.orderedItems(l).find((i) => i.group === group)?.id ?? null;
  }

  lastOf(group: string): string | null {
    const l = this.active;
    if (!l) return null;
    const items = this.orderedItems(l);
    for (let i = items.length - 1; i >= 0; i--) {
      if (items[i].group === group) return items[i].id;
    }
    return null;
  }

  // Engine-internal (kbItem action). A freshly-mounted item claims the
  // layer's cursor if it has none and kb is active; otherwise, if the
  // cursor already points at this id (set before the node existed), apply
  // its attribute now.
  registerCursor(layer: Layer, item: Item) {
    if (this.kbActive && layer.cursorId === null) {
      this.writeCursor(layer, item.id);
    } else if (this.kbActive && layer.cursorId === item.id) {
      item.node?.setAttribute("data-kb-cursor", "");
    }
  }

  // Engine-internal (kbItem action). The item has already been removed
  // from the layer's map and the DOM; if it held the cursor, clamp to the
  // first remaining item in visual order so the cursor never lingers on a
  // gone node. Its former neighbours are unknowable here, so a layer that
  // wants the cursor handed to one moves it before the item unmounts.
  clampCursorAfterRemoval(layer: Layer, removedId: string) {
    if (layer.cursorId === removedId) {
      this.writeCursor(layer, this.orderedItems(layer)[0]?.id ?? null);
    }
  }

  dispatch(e: KeyboardEvent): boolean {
    // IME composition: let the input method / text field own the keys.
    if (e.isComposing || e.keyCode === 229) return false;

    // Bare modifier presses are not keyboard driving (see MODIFIER_KEYS).
    if (MODIFIER_KEYS.has(e.key)) return false;

    const layer = this.active;
    if (!layer) return false;

    const key = canonicalKey(e);

    // Tab is neutralized app-wide: its only behavior is the j/k alias
    // resolved below. preventDefault here covers the rare path where
    // dispatch returns false (e.g. tab pressed inside a text input):
    // native tab cycling never fires regardless of tabindex coverage.
    if (key === "tab") e.preventDefault();

    const target = e.target as Element | null;
    const inText =
      target instanceof HTMLElement &&
      (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable);
    // While typing in a text input, kb stays out of the way for plain
    // keys, including Escape (the input's own handler blurs, and a
    // second Escape then reaches kb to close the layer). Modifier-bearing
    // combos always reach kb so app shortcuts (Ctrl+S etc.) keep working.
    // Tab is the exception (handled just above): always swallowed so it
    // can't move focus out of the input.
    if (inText && !e.ctrlKey && !e.altKey && !e.metaKey) {
      return false;
    }

    // Reaching here means the user is actively driving with the keyboard.
    // First press of the session reveals the cursor across every layer
    // (h/l for folder nav also count, they're dispatched key events).
    this.setKbActive(true);

    // Arrow keys alias to vim hjkl across every layer that binds them.
    // Resolution happens here so each layer declares the binding once
    // (under j/k/h/l) and the StatusBar hints still read as the vim keys.
    // Aliases only apply when no modifiers (ctrl/alt/meta) are held;
    // ctrl+arrow stays a separate combo for whatever else binds it.
    //
    // Tab/Shift+Tab also alias to j/k. With tabindex={-1} on every
    // interactive element app-wide, native tab cycling has nothing to
    // focus, so binding tab here is the only way it ever does anything.
    // Tab itself is always swallowed (preventDefault above) regardless
    // of binding so it can never reach native focus management.
    let lookupKey: string;
    if (e.ctrlKey || e.altKey || e.metaKey) {
      lookupKey = key;
    } else if (key === "tab") {
      lookupKey = e.shiftKey ? "k" : "j";
    } else {
      lookupKey = ARROW_ALIASES[key] ?? key;
    }

    let binding = layer.bindings[lookupKey];
    if (!binding) {
      // Shifted letters canonicalize uppercase (G, ctrl+A). When the layer
      // binds only the lowercase form, fall back to it; shift on a letter
      // never matters unless a layer explicitly binds the uppercase key.
      const lower = lookupKey.toLowerCase();
      if (lower !== lookupKey && layer.bindings[lower]) {
        lookupKey = lower;
        binding = layer.bindings[lower];
      }
    }
    if (binding) {
      e.preventDefault();
      e.stopPropagation();
      // Auto-repeat (held key) drives only movement: re-running a
      // destructive or one-shot action (x → remove, enter → activate) on
      // every repeat tick would be a surprise. Repeats of everything else
      // are swallowed (already prevented above) without re-running.
      if (!(e.repeat && !MOVEMENT_KEYS.has(lookupKey))) {
        runSafely(
          () => binding.run(),
          `binding "${lookupKey}"${binding.label ? ` (${binding.label})` : ""}`,
        );
      }
      return true;
    }
    if (layer.capture === "all") {
      e.preventDefault();
      return true;
    }
    return false;
  }
}

export const kb = new KB();

// Run an action, logging (not rethrowing) any throw with context. Bindings
// and item activations call into the async daemon/Tauri client, which
// rejects on disconnect/timeout; an uncaught throw would escape the global
// capture-phase listener silently after preventDefault already fired.
// Bindings stay responsible for user-facing toasts; the engine just must
// not eat the throw in silence.
function runSafely(fn: () => void, context: string) {
  try {
    fn();
  } catch (err) {
    console.error(`kb: ${context} threw`, err);
  }
}

// Dev-only: warn on binding keys that can never fire: a non-canonical key
// (an arrow alias, which dispatch resolves away before lookup; uppercase is
// canonical only as a single shifted letter, e.g. "G" or "ctrl+G") or two
// keys that resolve to the same canonical form (one shadows the other).
// Silent in production.
function warnBindings(bindings: Record<string, Binding>) {
  const seen = new Map<string, string>();
  for (const key of Object.keys(bindings)) {
    const canon =
      key === "tab"
        ? "tab"
        : (ARROW_ALIASES[key] ?? (/(^|\+)[A-Z]$/.test(key) ? key : key.toLowerCase()));
    if (canon !== key) {
      console.warn(
        `kb: binding "${key}" is non-canonical (resolves to "${canon}") and will never match; bind under "${canon}".`,
      );
    }
    const prev = seen.get(canon);
    if (prev !== undefined) {
      console.warn(
        `kb: bindings "${prev}" and "${key}" both resolve to "${canon}"; one shadows the other.`,
      );
    }
    seen.set(canon, key);
  }
}

// Svelte action that registers a DOM element as a kb item on its nearest
// KbLayer owner. Single source of truth for "this thing is navigable":
//   * sets data-kb-id on the node so scroll-follow can target it
//   * applies data-kb-cursor when this item is the layer's cursor (the
//     engine drives it imperatively; Tailwind data-[kb-cursor]:* styles it)
//   * wires pointerdown → kb.setCursor (mouse-to-kb cursor sync)
//   * registers/unregisters with the layer in mount order = DOM order
//
// Activate defaults: click for buttons (& most elements), focus for
// inputs/textareas. Pass an explicit `activate` to override.
export type KbItemInit = {
  id: string;
  activate?: () => void;
  group?: string;
};

export function kbItem(node: HTMLElement, init: KbItemInit | undefined) {
  // Pass-through when the consumer didn't supply a kbItem: lets primitives
  // declare `use:kbItem={kbItemInit}` unconditionally without growing
  // optional-action complexity.
  if (!init) return { update() {}, destroy() {} };

  // Capture the lexical layer at mount time, not kb.active. Persistent
  // background layers can remount items while an overlay/menu owns the top
  // of the stack; ownership still belongs to the closest KbLayer boundary.
  const layerEl = node.closest("[data-kb-layer]");
  const layerId = layerEl ? Number(layerEl.getAttribute("data-kb-layer")) : NaN;
  const layer = Number.isFinite(layerId) ? kb.findById(layerId) : null;
  if (!layer) {
    if (import.meta.env.DEV) {
      console.warn(
        `kb: kbItem "${init.id}" found no live KbLayer (nearest [data-kb-layer]=${
          layerEl?.getAttribute("data-kb-layer") ?? "none"
        }). It will not be navigable. Is it rendered inside its KbLayer (incl. across a portal)?`,
      );
    }
    return { update() {}, destroy() {} };
  }

  let current: KbItemInit = init;

  const defaultActivate = () => {
    if (node instanceof HTMLInputElement || node instanceof HTMLTextAreaElement) {
      node.focus();
    } else {
      node.click();
    }
  };

  const item: Item = {
    id: init.id,
    activate: () => (current.activate ?? defaultActivate)(),
    group: init.group,
    node,
  };

  node.setAttribute("data-kb-id", init.id);
  layer.items.set(init.id, item);
  // Claim the cursor if the layer has none (covers "open Settings via ,",
  // any kb-driven overlay that doesn't pre-set a cursor), or apply the
  // attribute if the cursor was already pointing here before this node
  // mounted. Mouse-only opens (kbActive=false) and layers that set their
  // own initial cursor (Prompts, Browser nav landing) skip the claim.
  kb.registerCursor(layer, item);

  // pointerdown moves the kb cursor: fires before :focus so cursor moves
  // before any focus ring competes. Only when this layer is still active;
  // a stale-mounted item under a higher layer shouldn't grab the cursor.
  const onDown = () => {
    if (kb.active?.id === layer.id) kb.setCursor(init.id);
  };
  node.addEventListener("pointerdown", onDown);

  return {
    update(next: KbItemInit) {
      // Id changes are not supported: would orphan the data attribute and
      // the map entry. Consumers that need to swap ids remount via {#key}.
      if (import.meta.env.DEV && next.id !== current.id) {
        console.warn(
          `kb: kbItem id changed "${current.id}" → "${next.id}"; ids are fixed, remount via {#key} to change one.`,
        );
      }
      current = next;
      item.group = next.group;
    },
    destroy() {
      node.removeEventListener("pointerdown", onDown);
      layer.items.delete(init.id);
      kb.clampCursorAfterRemoval(layer, init.id);
    },
  };
}

// The platform's primary modifier: ⌘ (meta) on macOS, ctrl elsewhere. Bindings
// declare `${MOD}+x` for the conventional "command" shortcut; on macOS
// canonicalKey produces "meta+x" so ⌘ matches, "ctrl+x" elsewhere.
const isMac = typeof navigator !== "undefined" && /mac/i.test(navigator.userAgent);
export const MOD: "ctrl" | "meta" = isMac ? "meta" : "ctrl";

// Canonical key string: `[ctrl+][alt+][meta+]<base>`. Shift is folded into
// the produced character (e.g. `?` not `shift+/`) so bindings read like the
// keys you actually type. A shift-produced uppercase letter stays uppercase
// so pager motions like 'G' are bindable; dispatch falls back to the
// lowercase binding when the uppercase form is unbound, so shift on a letter
// stays meaningless to layers that don't bind it. CapsLock uppercase
// (no shift held) still lowercases: 'J' matches its binding ('j');
// punctuation (?, /, !) is unaffected by lowercasing.
function canonicalKey(e: KeyboardEvent): string {
  let key = e.key;
  if (key === " ") key = "space";
  else if (!(e.shiftKey && /^[A-Z]$/.test(key))) key = key.toLowerCase();
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("ctrl");
  if (e.altKey) mods.push("alt");
  if (e.metaKey) mods.push("meta");
  return mods.length ? `${mods.join("+")}+${key}` : key;
}

const ARROW_ALIASES: Record<string, string> = {
  arrowdown: "j",
  arrowup: "k",
  arrowleft: "h",
  arrowright: "l",
};

const KEY_DISPLAY: Record<string, string> = {
  enter: "↵",
  escape: "esc",
  arrowup: "↑",
  arrowdown: "↓",
  arrowleft: "←",
  arrowright: "→",
  space: "␣",
  tab: "⇥",
  backspace: "⌫",
};

// On macOS modifiers render as glyphs with no separator (⌘S, ⌥↵); elsewhere
// they keep the spelled-out `ctrl+`/`alt+`/`meta+` form.
const MOD_DISPLAY: Record<string, string> = isMac ? { ctrl: "⌃", alt: "⌥", meta: "⌘" } : {};

// Pretty key string for the StatusBar / help cheatsheet: splits modifiers,
// renames the base key via KEY_DISPLAY (enter → ↵, space → ␣, …), then renders
// per platform. Exported because the help cheatsheet uses the same alphabet.
export function displayKey(canonical: string): string {
  const parts = canonical.split("+");
  const base = parts.pop()!;
  const baseDisplay = KEY_DISPLAY[base] ?? base;
  if (isMac) return parts.map((m) => MOD_DISPLAY[m] ?? m).join("") + baseDisplay;
  return [...parts, baseDisplay].join("+");
}

if (typeof window !== "undefined") {
  // Capture phase: kb runs BEFORE any browser default (Firefox quick-find
  // on `/`, button-click-on-Enter, scroll-on-Space, etc.). Without this,
  // those defaults can fire before our preventDefault on bubble.
  const onKeydown = (e: KeyboardEvent) => kb.dispatch(e);

  // Pointer interaction = mouse mode. Hides the cursor visual without
  // touching cursor position (kbItem's per-element pointerdown handler
  // still moves the cursor where the user clicked, so the next keypress
  // picks up exactly there). Capture phase so this fires before any
  // element-level handler, but order doesn't actually matter: kbActive
  // is a single boolean, the end state is what counts.
  const onPointerdown = () => kb.setKbActive(false);

  window.addEventListener("keydown", onKeydown, true);
  window.addEventListener("pointerdown", onPointerdown, true);

  // HMR: tear the listeners down on dispose so editing this file in dev
  // doesn't stack duplicate listeners against a fresh KB singleton
  // (phantom double-input that reads as nav bugs).
  if (import.meta.hot) {
    import.meta.hot.dispose(() => {
      window.removeEventListener("keydown", onKeydown, true);
      window.removeEventListener("pointerdown", onPointerdown, true);
    });
  }
}
