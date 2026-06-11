<script lang="ts">
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<p class="text-xs text-muted whitespace-pre-line">
  {@render children()}
</p>
