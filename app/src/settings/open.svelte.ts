// Settings overlay visibility. A close request (the TopBar cog flipping the
// flag, or any `settingsOpen.value = false`) routes through the registered
// close handler so every close path honors the unsaved-changes dirty-check. The
// overlay only really closes via closeSettings(), once that handler allows it.
// (esc / comma / backdrop call the handler, Settings.requestClose, directly.)

let isOpen = $state(false);
let requestCloseHandler: (() => void) | null = null;

export const settingsOpen = {
  get value(): boolean {
    return isOpen;
  },
  set value(next: boolean) {
    if (next) isOpen = true;
    else if (requestCloseHandler) requestCloseHandler();
    else isOpen = false;
  },
};

/** Settings.svelte registers its dirty-checked close while mounted. */
export function onSettingsCloseRequest(handler: () => void): () => void {
  requestCloseHandler = handler;
  return () => {
    if (requestCloseHandler === handler) requestCloseHandler = null;
  };
}

/** Actually close the overlay, after the dirty-check has passed. */
export function closeSettings(): void {
  isOpen = false;
}
