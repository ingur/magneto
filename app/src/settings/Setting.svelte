<script lang="ts">
  import type { Snippet } from "svelte";

  // One Settings row: a label on the left, its control on the right. Fixed
  // height so the list scrolls predictably; the control wrapper grows so a wide
  // control (FilePicker) fills up to its own max-width.
  interface Props {
    label: string;
    children: Snippet;
  }
  let { label, children }: Props = $props();
</script>

<div class="flex h-9 items-center gap-4">
  <div class="min-w-0 truncate text-sm text-muted">{label}</div>
  <div class="flex min-w-0 flex-1 justify-end">{@render children()}</div>
</div>
