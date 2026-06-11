<script lang="ts">
  import { nav } from "@/torrents/nav.svelte";
  import StatusItem from "@/lib/status/StatusItem.svelte";

  // Renders nothing when nothing is marked (so the bar refits), and hides while
  // the search field is being TYPED (it owns the bar's left). Reappears once the
  // filter is committed; marks on the flat results are live. Click clears.
  const count = $derived(nav.selection.size);
</script>

{#if count > 0 && !nav.filter.typing}
  <StatusItem priority={70} class="text-marked">
    <button
      type="button"
      tabindex={-1}
      onclick={() => nav.clearSelection()}
      class="text-marked/80 hover:text-marked cursor-default"
    >
      {count} selected
    </button>
  </StatusItem>
{/if}
