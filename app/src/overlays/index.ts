import type { Component } from "svelte";

import { settingsOpen } from "@/settings/open.svelte";
import Settings from "@/settings/Settings.svelte";
import { helpOpen } from "@/help/open.svelte";
import Help from "@/help/Help.svelte";

// Overlay registry. Each entry pairs a global open-flag with the component
// rendered while it's true. OverlayHost iterates this list, so App.svelte
// stays oblivious to which overlays exist; adding one is just an entry
// here (plus its open.svelte / Component pair under the feature dir).

export type OverlayDef = {
  id: string;
  open: { value: boolean };
  Component: Component;
};

export const overlayRegistry: OverlayDef[] = [
  { id: "settings", open: settingsOpen, Component: Settings },
  { id: "help", open: helpOpen, Component: Help },
];
