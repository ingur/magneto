// Browser layer keybindings. Factored out so Browser.svelte reads as "what
// renders + how the cursor moves" and this reads as "what each key does."
// Targets the marked selection if any, else the cursored row (resolveTargets).
//
// Hint order in the StatusBar follows declaration order of labeled bindings:
// navigate → play → save → menu → delete → search → help. Unlabeled bindings
// (h/l/enter/backspace/ctrl+a/`/`,`) stay out of the bar but appear in help.

import { kb, MOD, type Binding, type LayerHandle } from "@/lib/kb/kb.svelte";

import { settingsOpen } from "@/settings/open.svelte";
import { sortOpen } from "@/chrome/sort-open.svelte";
import { helpOpen } from "@/help/open.svelte";
import { nav } from "@/torrents/nav.svelte";
import * as actions from "@/torrents/actions";
import { ensureFuseLoaded } from "@/torrents/filter.svelte";
import { zoom } from "@/zoom";
import { rowActionsFor } from "./row-actions";

export interface BrowserBindingDeps {
  getHandle: () => LayerHandle | undefined;
  toggleSelectAll: () => void;
  navOpen: () => void;
  // True when an active rubber-band drag was cancelled; escape stops there.
  cancelDragSelect: () => boolean;
}

const NAV = "Navigation";
const SEL = "Selection";
const ACT = "Actions";
const DISC = "Discovery";

export function browserBindings(deps: BrowserBindingDeps): Record<string, Binding> {
  const { getHandle, toggleSelectAll, navOpen, cancelDragSelect } = deps;

  // Briefly light the cursored row's button when its shortcut fires. The action
  // still targets the selection/cursor via resolveTargets; only the flash is
  // cursor-scoped (a bulk action has no single button to light).
  const flashCursor = (action: "play" | "save") => {
    const id = kb.cursor();
    if (id) rowActionsFor(id)?.flash(action);
  };

  return {
    j: { label: "navigate", priority: 30, category: NAV, run: () => kb.next() },
    k: { label: "navigate", priority: 30, category: NAV, run: () => kb.prev() },
    // Pager motions. No StatusBar label (same treatment as h/l). The shared
    // description merges both into one cheatsheet row ("g / home / G / end
    // jump"), mirroring j/k's single "navigate" row; home/end appear there
    // via the cheatsheet's KEY_ALIASES.
    g: { description: "jump", category: NAV, run: () => kb.first() },
    G: { description: "jump", category: NAV, run: () => kb.last() },
    home: { run: () => kb.first() },
    end: { run: () => kb.last() },

    // `p` always plays (even on a folder/torrent; the daemon expands the
    // playable files). Distinct from enter/l, which navigate.
    p: {
      label: "play",
      priority: 70,
      category: ACT,
      run: () => {
        actions.runPlay(actions.resolveTargets());
        flashCursor("play");
      },
    },
    // Reachable from the row's overflow menu, so kept out of the StatusBar.
    d: {
      category: ACT,
      description: "toggle download",
      run: () => actions.runToggleDownload(actions.resolveTargets()),
    },
    s: {
      label: "save",
      priority: 60,
      category: ACT,
      description: "toggle save",
      run: () => {
        actions.runTogglePersist(actions.resolveTargets());
        flashCursor("save");
      },
    },
    // mod+s aliases s, invisible (no label/description) so the cheatsheet's
    // "toggle save" row stays a clean single `s`.
    [`${MOD}+s`]: {
      category: ACT,
      run: () => {
        actions.runTogglePersist(actions.resolveTargets());
        flashCursor("save");
      },
    },
    m: {
      label: "menu",
      priority: 50,
      category: ACT,
      description: "toggle menu",
      run: () => {
        const id = kb.cursor();
        if (id) rowActionsFor(id)?.openMenu();
      },
    },
    // Yank: copy a shareable magnet link for the selection/cursor. Listed after
    // the menu in the cheatsheet; no StatusBar label (help only).
    y: {
      category: ACT,
      description: "copy magnet",
      run: () => actions.runCopyMagnet(actions.resolveTargets()),
    },
    x: {
      label: "delete",
      priority: 70,
      category: ACT,
      run: () => {
        // Guard: act only while the browser layer is on top (a confirm prompt
        // pushes its own layer, so a stray x can't reach here mid-prompt).
        const handle = getHandle();
        if (!handle || !kb.isActive(handle)) return;
        actions.runDelete(actions.resolveTargets());
      },
    },

    space: {
      description: "toggle selected",
      category: SEL,
      run: () => {
        const id = kb.cursor();
        if (id) nav.toggleMark(id);
      },
    },
    [`${MOD}+a`]: { description: "select all", category: SEL, run: toggleSelectAll },
    escape: {
      description: "clear",
      category: SEL,
      run: () => {
        if (cancelDragSelect()) return;
        if (nav.filter.active) nav.clearFilter();
        else nav.clearSelection();
      },
    },

    // enter activates the cursored row (folder / multi-file torrent opens, a
    // leaf file or single-file torrent plays), same primitive as double-click.
    enter: { description: "open", category: NAV, run: () => kb.activate() },
    // l/→ is navigation-only: open a container, no-op on a leaf (never plays).
    l: { description: "go into", category: NAV, run: navOpen },
    h: { description: "go back", category: NAV, run: () => nav.back() },
    backspace: { description: "go back", category: NAV, run: () => nav.back() },

    // UI zoom, one row at the top of Discovery. App.svelte also handles it
    // globally (overlays included); this entry runs it in the browser layer.
    [`${MOD}+=`]: { description: "zoom", category: DISC, run: () => zoom.in() },
    [`${MOD}+-`]: { description: "zoom", category: DISC, run: () => zoom.out() },
    [`${MOD}+0`]: { description: "zoom", category: DISC, run: () => zoom.reset() },

    "/": {
      label: "search",
      priority: 60,
      category: DISC,
      clickable: true,
      run: () => {
        ensureFuseLoaded();
        if (nav.filter.active) nav.editFilter();
        else nav.startFilter();
      },
    },
    // Backtick toggles the sort menu, kept out of the StatusBar (the
    // SortButton icon already lives in the TopBar); a help-only nicety.
    "`": { description: "sort", category: DISC, run: () => (sortOpen.value = !sortOpen.value) },
    ",": { description: "settings", category: DISC, run: () => (settingsOpen.value = true) },
    "?": {
      label: "help",
      priority: 80,
      category: DISC,
      clickable: true,
      run: () => (helpOpen.value = true),
    },
  };
}
