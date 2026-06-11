// Help layer keybindings. j/k walk the cheatsheet rows then drop into the footer;
// h/l move between the footer buttons (About / Repository); enter/space activate
// the cursored button; esc and ? both close. ? keeps its "help" label so the
// StatusBar hint stays stable and clicking it toggles Help shut.

import { kb, type Binding } from "@/lib/kb/kb.svelte";
import { verticalNavWithButtons } from "@/lib/kb/vertical-nav";

export type HelpButtonId = "about" | "repo";

export interface HelpBindingDeps {
  getLastButton: () => HelpButtonId;
  setLastButton: (id: HelpButtonId) => void;
  close: () => void;
}

export function helpBindings(deps: HelpBindingDeps): Record<string, Binding> {
  const nav = verticalNavWithButtons({
    rowGroup: "help",
    buttonGroup: "buttons",
    leftButtonId: "about",
    rightButtonId: "repo",
    getLastButton: deps.getLastButton,
    setLastButton: (id) => deps.setLastButton(id as HelpButtonId),
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
    enter: { run: () => kb.activate() },
    space: { run: () => kb.activate() },
    escape: { label: "close", priority: 80, clickable: true, run: deps.close },
    "?": { label: "help", priority: 80, clickable: true, run: deps.close },
  };
}
