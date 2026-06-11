<script lang="ts">
  import { overlayRegistry } from "./index";

  // Iterates the registry; mounts overlays whose open-flag is true.
  // Adding a new overlay = appending to overlayRegistry. App.svelte
  // doesn't change, this file doesn't change.
</script>

{#each overlayRegistry as overlay (overlay.id)}
  {#if overlay.open.value}
    {@const Component = overlay.Component}
    <Component />
  {/if}
{/each}
