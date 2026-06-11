<script lang="ts">
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<!-- text-base (24px line-height) keeps the title section compact and
     consistent with Dialog's rhythm: pt-4 + content + pb-3 → 52px.
     Footer is asymmetric on purpose (border-t-2 + pt-2 + h-7 + pb-2 →
     46px): the border does the section-break work, so padding shrinks. -->
<div class="text-base font-semibold text-fg">
  {@render children()}
</div>
