<script lang="ts">
  import type { Snippet } from "svelte";

  // A titled group of Setting rows. The small-caps header is quieter than the
  // row labels; sections are separated by the parent's gap-6.
  interface Props {
    title: string;
    children: Snippet;
  }
  let { title, children }: Props = $props();
</script>

<div class="flex flex-col">
  <div class="mb-1.5 text-xs font-semibold uppercase tracking-wider text-subtle">{title}</div>
  <div class="flex flex-col gap-1.5">{@render children()}</div>
</div>
