// Sort popover visibility, colocated with SortButton. The list view binds
// the `s` hotkey to toggle this; SortButton binds its own `open` prop to it
// so click and hotkey share the same state.

export const sortOpen = $state<{ value: boolean }>({ value: false });
