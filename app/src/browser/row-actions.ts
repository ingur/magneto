// Per-row imperatives the browser layer invokes against the cursored row:
// `openMenu` opens that row's overflow popover (`m`), and `flash` briefly lights
// a row's action button when its keyboard shortcut fires (`p`/`s`), a per-row
// visual the headless action seam can't do. The actions themselves still target
// the cursor OR the marked selection through the seam; only the feedback is
// cursor-scoped.
//
// A plain Map, not $state: bindings invoke it imperatively (a keypress runs
// a function), so reactivity isn't needed and would add register churn.

export type RowActions = {
  openMenu: () => void;
  flash: (action: "play" | "save") => void;
};

const map = new Map<string, RowActions>();

export function registerRowActions(id: string, actions: RowActions): () => void {
  map.set(id, actions);
  return () => {
    // Identity check guards an unmount-after-remount race: if a remount
    // overwrote our entry, our cleanup must not drop theirs.
    if (map.get(id) === actions) map.delete(id);
  };
}

export function rowActionsFor(id: string): RowActions | null {
  return map.get(id) ?? null;
}
