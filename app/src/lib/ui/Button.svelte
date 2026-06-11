<script lang="ts">
  import type { Snippet } from "svelte";
  import { kbItem, type KbItemInit } from "@/lib/kb/kb.svelte";

  // Tint names the action's INTENT, not a color; the theme maps it to a
  // role token. Extend the union + the maps below in one line if a future
  // consumer needs another intent (e.g. a warning-tinted action).
  type Tint = "accent" | "info" | "danger";

  interface Props {
    children: Snippet;
    tint?: Tint;
    onclick?: () => void;
    kbItem?: KbItemInit;
  }

  let { children, tint, onclick, kbItem: kbItemInit }: Props = $props();

  // All buttons are filled: a tinted button uses its accent role, a neutral
  // one uses the raised surface as the gray fill. No border, the fill IS
  // the button; text-on-accent keeps the label legible over any fill.
  const tinted: Record<Tint, string> = {
    accent: "bg-accent text-on-accent",
    info: "bg-info text-on-accent",
    danger: "bg-danger text-on-accent",
  };
  const tintedHover: Record<Tint, string> = {
    accent: "hover:bg-accent/80",
    info: "hover:bg-info/80",
    danger: "hover:bg-danger/80",
  };
  const neutral = "bg-raised text-fg";
  const neutralHover = "hover:bg-raised/80";
</script>

<!-- inline-flex centers content in BOTH axes, with a tiny gap between
     siblings (auto-applies when an icon sits next to label text). Without
     this, a button with a leading icon would baseline-align rather than
     center-align: text would sit at the icon's baseline (visibly low),
     and the same text in a text-only button would sit at the button's
     own baseline (visibly higher), so two adjacent buttons would have
     misaligned labels. items-center fixes both cases identically.
     gap-1.5 collapses to nothing when there's only a single child, so
     text-only buttons are unaffected.

     Cursor = 2px cursor-color ring with a 2px page-bg offset gap, which
     keeps the ring visible over any fill, including filled buttons where
     a plain ring would need the gap to read clearly. The offset is the
     page bg (behind the button), not on-accent. Hover and cursor are
     different visual layers (bg vs ring) so they coexist when both apply.
     The kbItem action wires pointerdown→setCursor and toggles
     data-kb-cursor; the cursor visual is purely a CSS concern below. -->
<button
  type="button"
  tabindex={-1}
  use:kbItem={kbItemInit}
  {onclick}
  class={[
    "inline-flex h-7 items-center justify-center gap-1.5 rounded px-3 text-xs outline-none",
    tint ? tinted[tint] : neutral,
    tint ? tintedHover[tint] : neutralHover,
    "data-[kb-cursor]:ring-2 data-[kb-cursor]:ring-cursor data-[kb-cursor]:ring-offset-2 data-[kb-cursor]:ring-offset-bg",
  ]}
>
  {@render children()}
</button>
