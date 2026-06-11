<script lang="ts">
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import { kbItem, type KbItemInit } from "@/lib/kb/kb.svelte";
  import { widths, type Size } from "./size";

  interface Props {
    value?: string;
    size?: Size;
    el?: HTMLInputElement | undefined;
    kbItem?: KbItemInit;
    // Caller supplies the browse handler so this control stays free of
    // app-specific dependencies (no toast import, no Tauri dialog import).
    // App passes whatever the integration layer offers: placeholder, native
    // dialog, etc.
    onBrowse?: () => void;
  }

  let {
    value = $bindable(""),
    size = "lg",
    el = $bindable(),
    kbItem: kbItemInit,
    onBrowse,
  }: Props = $props();

  // Merge consumer activate with our default (focus the inner input). The
  // default is what the kbItem action would do if it were on the input
  // directly, but the action lives on the wrapper here so activate has to
  // know to focus the input.
  const action = $derived<KbItemInit | undefined>(
    kbItemInit
      ? { ...kbItemInit, activate: kbItemInit.activate ?? (() => el?.focus()) }
      : undefined,
  );
</script>

<!-- Wrapper carries the kb cursor / focus ring. Inner input + browse button
     share that ring; either focus lights it. Hover only changes the
     background. -->
<div
  use:kbItem={action}
  class={[
    "flex h-8 w-full min-w-0 rounded bg-panel ring-1 ring-raised ring-inset focus-within:ring-2 focus-within:ring-cursor",
    widths[size],
    "not-data-[kb-cursor]:hover:bg-raised/30",
    "data-[kb-cursor]:ring-2 data-[kb-cursor]:ring-cursor",
  ]}
>
  <input
    type="text"
    tabindex={-1}
    bind:this={el}
    bind:value
    onkeydown={(e) => {
      if (e.key === "Enter" || e.key === "Escape") {
        (e.currentTarget as HTMLInputElement).blur();
      }
    }}
    class="text-fg placeholder:text-subtle h-full min-w-0 flex-1 bg-transparent px-2 text-sm outline-none"
  />
  <button
    type="button"
    tabindex={-1}
    onclick={() => onBrowse?.()}
    class="text-muted hover:text-fg grid aspect-square h-full shrink-0 place-items-center transition-colors"
  >
    <Ellipsis size={14} />
  </button>
</div>
