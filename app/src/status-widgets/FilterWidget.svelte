<script lang="ts">
  import { kb, type Binding } from "@/lib/kb/kb.svelte";
  import { nav } from "@/torrents/nav.svelte";
  import KbLayer from "@/lib/kb/KbLayer.svelte";

  // Owns the status bar's left while filtering (the count widgets hide). Typing
  // is not captured (the kb engine's text-input awareness routes keys to the
  // field), while a thin "filter" layer supplies the apply/clear hints through
  // the normal hint system.

  let input = $state<HTMLInputElement>();

  // Force kb mode while the search field is engaged so the cursored match stays
  // highlighted. Depends on `typing` so re-editing via the committed-query
  // button (a mouse click that flipped kb mode off) re-forces it.
  $effect(() => {
    if (nav.filter.active && nav.filter.typing) kb.setKbActive(true);
  });
  $effect(() => {
    if (nav.filter.typing) input?.focus();
  });

  // The pushed `filter` layer supplies the apply/clear HINTS; the real keyboard
  // handler is the input's onKeydown below. The kb engine routes Enter/Escape to
  // a focused text input before any layer binding, so these `run`s only fire
  // from a click (escape's clickable "clear"). Don't delete onKeydown "to
  // dedupe": the layer bindings can't fire from the keyboard while typing.
  const bindings: Record<string, Binding> = {
    enter: { label: "apply", run: () => nav.commitFilter() },
    escape: { label: "clear", clickable: true, run: () => nav.clearFilter() },
  };

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") nav.commitFilter();
    else if (e.key === "Escape") nav.clearFilter();
  }
</script>

{#if nav.filter.active}
  {#if nav.filter.typing}
    <KbLayer name="filter" {bindings}>
      <div class="flex min-w-0 flex-1 items-center gap-1.5 text-xs">
        <span class="text-muted shrink-0">search:</span>
        <input
          bind:this={input}
          type="text"
          tabindex={-1}
          value={nav.filter.query}
          oninput={(e) => nav.setQuery(e.currentTarget.value)}
          onkeydown={onKeydown}
          class="text-fg min-w-0 flex-1 bg-transparent outline-none"
        />
      </div>
    </KbLayer>
  {:else}
    <button
      type="button"
      tabindex={-1}
      onclick={() => nav.editFilter()}
      class="flex min-w-0 items-center gap-1.5 text-xs"
    >
      <span class="text-muted shrink-0">search:</span>
      <span class="text-fg truncate">{nav.filter.query}</span>
    </button>
  {/if}
{/if}
