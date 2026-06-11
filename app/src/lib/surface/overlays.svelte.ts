// Tiny counter: `overlays.any` is true while any Surface is mounted.
// Surfaces register on mount, unregister on destroy. Future hotkeys
// (e.g. command palette) can read this to decide whether to fire when
// a dialog/overlay is visible.
//
// Intentionally NOT a registry of which overlays are open; that would
// couple visibility flags. Each overlay still owns its own *open.svelte.ts
// module; this counter just answers "is there ANY surface up?".

class Overlays {
  count = $state(0);
  open() {
    this.count++;
    return () => {
      this.count--;
    };
  }
  get any(): boolean {
    return this.count > 0;
  }
}

export const overlays = new Overlays();
