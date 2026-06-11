// Settings layer keybindings. j/k walk the rows then drop into the footer; h/l
// move between the footer buttons; enter/space edit the cursored control; esc
// and comma both close through the dirty-check (requestClose). Same rows+footer
// vertical-nav helper Prompts uses.

import { kb, MOD, type Binding } from "@/lib/kb/kb.svelte";
import { verticalNavWithButtons } from "@/lib/kb/vertical-nav";

export type SettingsButtonId = "reset" | "save";

export interface SettingsBindingDeps {
  getLastButton: () => SettingsButtonId;
  setLastButton: (id: SettingsButtonId) => void;
  requestClose: () => void;
  save: () => void;
}

export function settingsBindings(deps: SettingsBindingDeps): Record<string, Binding> {
  const nav = verticalNavWithButtons({
    rowGroup: "settings",
    buttonGroup: "buttons",
    leftButtonId: "reset",
    rightButtonId: "save",
    getLastButton: deps.getLastButton,
    setLastButton: (id) => deps.setLastButton(id as SettingsButtonId),
  });

  return {
    j: { label: "navigate", priority: 30, run: nav.j },
    k: { label: "navigate", priority: 30, run: nav.k },
    h: { run: nav.h },
    l: { run: nav.l },
    g: { run: nav.first },
    G: { run: nav.last },
    home: { run: nav.first },
    end: { run: nav.last },
    enter: { label: "edit", priority: 60, run: () => kb.activate() },
    // Save without scrolling to the footer button (reaches kb even mid-typing).
    [`${MOD}+s`]: { label: "save", priority: 70, run: deps.save },
    space: { run: () => kb.activate() },
    escape: { label: "close", priority: 80, clickable: true, run: deps.requestClose },
    ",": { run: deps.requestClose },
  };
}
