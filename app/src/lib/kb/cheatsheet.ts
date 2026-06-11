// Help-cheatsheet projection. Reads a kb Layer's bindings and groups them
// into sections by `category`, merging keys that share a description (so
// j and k both labeled 'navigate' collapse to a single "j / k  navigate"
// row). Bindings without `label` AND without `description` are excluded;
// those are truly internal helpers (e.g. arrow-key aliases handled inside
// kb.dispatch, not declared as bindings at all).
//
// Order is declaration-order in the bindings record:
//   * categories appear in the order their first binding declares them
//   * entries within a category appear in the order their first key
//     declares them
//
// So the bindings.ts file IS the cheatsheet layout spec. No separate
// metadata to keep in sync.

import { displayKey, type Layer } from "./kb.svelte";

const DEFAULT_CATEGORY = "General";

// Display aliases the cheatsheet expands. kb.dispatch aliases arrow keys
// to vim-style hjkl at runtime: bindings are declared under j/k/h/l, but
// a user pressing the arrow key gets the same behavior. We surface the
// arrows alongside the canonical key here. home/end are declared as
// invisible sibling bindings of g/G in each layer; they surface here the
// same way. (Tab / Shift+Tab also alias to j/k inside kb, but we don't
// surface them in the cheatsheet; the glyphs read as ambiguous next to
// vim keys, so they stay implementation detail.) StatusBar hints stay
// terse (just the canonical key) since that path is bandwidth-limited.
const KEY_ALIASES: Record<string, string[]> = {
  j: ["↓"],
  k: ["↑"],
  h: ["←"],
  l: ["→"],
  g: ["home"],
  G: ["end"],
};

export type CheatsheetEntry = {
  // Display-formatted keys (e.g. ['j', '↓', '⇥', 'k', '↑', 'shift+⇥']
  // for navigate, or ['↵', 'l', '→'] for enter-folder).
  keys: string[];
  // Row text; falls back to `label` when `description` was unset.
  description: string;
};

export type CheatsheetSection = {
  category: string;
  entries: CheatsheetEntry[];
};

function expandWithAliases(canonicalKey: string): string[] {
  const aliases = KEY_ALIASES[canonicalKey];
  return aliases ? [displayKey(canonicalKey), ...aliases] : [displayKey(canonicalKey)];
}

export function cheatsheetFor(layer: Layer | null): CheatsheetSection[] {
  if (!layer) return [];

  type SectionAcc = { category: string; entries: Map<string, string[]> };
  const sections: SectionAcc[] = [];
  const sectionByName = new Map<string, SectionAcc>();

  for (const [key, b] of Object.entries(layer.bindings)) {
    const description = b.description ?? b.label;
    if (!description) continue;
    const category = b.category ?? DEFAULT_CATEGORY;

    let s = sectionByName.get(category);
    if (!s) {
      s = { category, entries: new Map() };
      sections.push(s);
      sectionByName.set(category, s);
    }
    let keys = s.entries.get(description);
    if (!keys) {
      keys = [];
      s.entries.set(description, keys);
    }
    keys.push(...expandWithAliases(key));
  }

  return sections.map((s) => ({
    category: s.category,
    entries: Array.from(s.entries, ([description, keys]) => ({ description, keys })),
  }));
}
