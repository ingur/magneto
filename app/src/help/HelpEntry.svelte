<script lang="ts">
  import { kbItem, type KbItemInit } from "@/lib/kb/kb.svelte";

  // One cheatsheet row: a description on the left, its key badges on the right.
  // Display-only and non-interactive; kbItem gives it a visual cursor (j/k walk
  // and scroll the rows) without native focus, so it needs no tabindex/role.
  interface Props {
    description: string;
    keys: string[];
    kbItem?: KbItemInit;
  }
  let { description, keys, kbItem: kbItemInit }: Props = $props();
</script>

<div
  class={[
    "-mx-2 flex scroll-my-2 cursor-default items-center gap-3 rounded p-2 first:scroll-mt-7",
    "hover:bg-hover",
    "data-[kb-cursor]:bg-raised/50",
  ]}
  use:kbItem={kbItemInit}
>
  <span class="flex-1 truncate text-sm text-fg">{description}</span>
  <span class="flex shrink-0 items-center gap-1 text-muted">
    {#each keys as key, i (i)}
      {#if i > 0}<span>/</span>{/if}
      <kbd class="rounded bg-panel px-1.5 py-0.5 text-xs text-fg ring-1 ring-raised ring-inset">
        {key}
      </kbd>
    {/each}
  </span>
</div>
