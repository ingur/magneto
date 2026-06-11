<script lang="ts">
  import { kbItem, type KbItemInit } from "@/lib/kb/kb.svelte";
  import { widths, type Size } from "./size";

  interface Props {
    value?: string;
    placeholder?: string;
    size?: Size;
    el?: HTMLInputElement | undefined;
    kbItem?: KbItemInit;
  }

  let {
    value = $bindable(""),
    placeholder,
    size = "sm",
    el = $bindable(),
    kbItem: kbItemInit,
  }: Props = $props();
</script>

<!-- The ring carries the cursor/focus signal. Hover only changes the
     background, never the ring, so hover and cursor are always visually
     distinct. The kbItem action's default activate is .focus() for inputs
     (handled inside the action), so kb.activate brings the user straight
     into edit mode. Escape blurs (kb sees Escape next and closes the layer). -->
<input
  type="text"
  tabindex={-1}
  bind:this={el}
  bind:value
  use:kbItem={kbItemInit}
  {placeholder}
  onkeydown={(e) => {
    if (e.key === "Enter" || e.key === "Escape") {
      (e.currentTarget as HTMLInputElement).blur();
    }
  }}
  class={[
    "text-fg placeholder:text-subtle h-8 w-full min-w-0 rounded bg-panel px-2 text-sm outline-none ring-1 ring-raised ring-inset focus:ring-2 focus:ring-cursor",
    widths[size],
    "not-data-[kb-cursor]:hover:bg-raised/30",
    "data-[kb-cursor]:ring-2 data-[kb-cursor]:ring-cursor",
  ]}
/>
