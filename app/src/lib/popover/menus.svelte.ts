// Tiny coordination store so anything in the app can ask "is a popover
// menu open right now?" and close it. Each Menu component registers its
// `close` callback on mount and unregisters on destroy. Only one menu
// is visible at a time, so a single closer slot is enough.
//
// Usage:
//   menus.register(close)              // returns unregister fn; call from $effect
//   menus.isAnyOpen                    // reactive boolean
//   menus.closeAny()                   // close whichever menu is open
//   menus.installContextHandler()      // global right-click → close menus,
//                                      // installed once from App's onMount

class Menus {
  closer = $state<(() => void) | null>(null);

  get isAnyOpen(): boolean {
    return this.closer !== null;
  }

  register(close: () => void): () => void {
    this.closer = close;
    return () => {
      if (this.closer === close) this.closer = null;
    };
  }

  closeAny() {
    this.closer?.();
  }

  // Block the browser's native context menu app-wide. Right-click is
  // ours alone, only triggering our own menus. If the right-click
  // landed on a row tagged data-menu-trigger-area, the row's
  // oncontextmenu already handles it; if it landed inside a menu
  // (data-menu-instance) we leave the menu alone; otherwise close any
  // open menu.
  installContextHandler() {
    const onCtx = (e: MouseEvent) => {
      e.preventDefault();
      const target = e.target as Element;
      if (target.closest("[data-menu-trigger-area]")) return;
      if (target.closest("[data-menu-instance]")) return;
      this.closeAny();
    };
    window.addEventListener("contextmenu", onCtx);
    return () => window.removeEventListener("contextmenu", onCtx);
  }
}

export const menus = new Menus();
