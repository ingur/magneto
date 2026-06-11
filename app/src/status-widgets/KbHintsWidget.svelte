<script lang="ts">
  import { kb } from "@/lib/kb/kb.svelte";
  import Keyhint from "@/lib/ui/Keyhint.svelte";
  import StatusItem from "@/lib/status/StatusItem.svelte";

  // Hints widget: emits one StatusItem per labeled binding on the
  // active kb layer. Each StatusItem inherits the binding's priority,
  // so a critical layer hint (e.g. esc cancel in a confirm dialog)
  // can outrank low-priority left-side stats and survive when space
  // gets tight.
  //
  // Clickable hints (binding declared `clickable: true`) render as a
  // button with hover-brightening label, same affordance as the
  // clickable stats. Inlined markup rather than going through Keyhint, so
  // `hover:text-fg` on the button can brighten the muted label without
  // Keyhint's own muted wrapper shadowing the hover. Non-clickable hints
  // stay informational and use Keyhint unchanged.
</script>

{#each kb.hints as hint (hint.key + hint.label)}
  <StatusItem priority={hint.priority}>
    {#if hint.run}
      <button
        type="button"
        tabindex={-1}
        onclick={hint.run}
        class="text-muted hover:text-fg cursor-default whitespace-nowrap text-xs"
      >
        <span class="text-fg">{hint.key}</span>
        {hint.label}
      </button>
    {:else}
      <Keyhint keys={hint.key} label={hint.label} />
    {/if}
  </StatusItem>
{/each}
