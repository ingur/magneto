<script lang="ts">
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<div class="text-sm font-semibold text-fg">
  {@render children()}
</div>
