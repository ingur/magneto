import { kb } from "./kb.svelte";

// Logic-only j/k/h/l for "rows + bottom button row" layouts (Settings,
// Prompts). Returns plain `() => void` handlers; the caller wraps them with
// their own { label, priority } so each layer decides what shows up in
// the StatusBar (Settings labels j/k as 'navigate'; Prompts labels h/l as
// 'navigate'). No labels here at all.
//
// Behavior:
//   j   walk down through `rowGroup`; from the last row, drop into the
//       bottom button group, landing on whichever button was last active.
//   k   walk up; from a button, jump back to the last row.
//   h   on the right button → move to left, remember.
//   l   on the left button → move to right, remember.
//   first/last  jump to the first/last row (pager motions g/G, home/end);
//       scoped to `rowGroup`, so from a button they climb back into the
//       list at its ends rather than landing on a sibling button.
//
// Confirm-style prompts have no input row, so `kb.lastOf(rowGroup)` returns
// null then and k from buttons stays put (no row to climb up to); j from
// buttons is a no-op anyway (the button-group guard, nowhere to drop into).
export function verticalNavWithButtons(opts: {
  rowGroup: string;
  buttonGroup: string;
  leftButtonId: string;
  rightButtonId: string;
  getLastButton: () => string;
  setLastButton: (id: string) => void;
}): { j(): void; k(): void; h(): void; l(): void; first(): void; last(): void } {
  const { rowGroup, buttonGroup, leftButtonId, rightButtonId } = opts;
  const { getLastButton, setLastButton } = opts;

  return {
    j() {
      const group = kb.cursorGroup();
      if (group === buttonGroup) return;
      if (kb.cursor() === kb.lastOf(rowGroup)) {
        kb.setCursor(getLastButton());
        return;
      }
      kb.next();
    },
    k() {
      if (kb.cursorGroup() === buttonGroup) {
        const last = kb.lastOf(rowGroup);
        if (last) kb.setCursor(last);
        return;
      }
      kb.prev();
    },
    h() {
      if (kb.cursor() === rightButtonId) {
        kb.setCursor(leftButtonId);
        setLastButton(leftButtonId);
      }
    },
    l() {
      if (kb.cursor() === leftButtonId) {
        kb.setCursor(rightButtonId);
        setLastButton(rightButtonId);
      }
    },
    first() {
      kb.first(rowGroup);
    },
    last() {
      kb.last(rowGroup);
    },
  };
}
