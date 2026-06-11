<script lang="ts">
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<!-- Row of Buttons sitting at the bottom of a Dialog or Overlay footer.
     Centered so single-button rows look intentional; 8px between buttons. -->
<div class="flex justify-center gap-2">
  {@render children()}
</div>
