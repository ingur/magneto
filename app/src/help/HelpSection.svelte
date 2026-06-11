<script lang="ts">
  import type { Snippet } from "svelte";

  // A help category: an uppercase header (mirrors Settings' section grouping)
  // over its cheatsheet rows.
  interface Props {
    title: string;
    children: Snippet;
  }
  let { title, children }: Props = $props();
</script>

<section class="flex flex-col gap-1">
  <h2 class="text-xs uppercase tracking-wider text-subtle">{title}</h2>
  <div class="flex flex-col">{@render children()}</div>
</section>
