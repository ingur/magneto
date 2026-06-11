<script lang="ts">
  import type { Snippet } from "svelte";
  import Surface from "./Surface.svelte";

  // Top-aligned, large (720), scrollable body. Optional title / actions
  // snippets render outside the scroll region (always visible).

  interface Props {
    children: Snippet;
    title?: Snippet;
    actions?: Snippet;
    onClose?: () => void;
  }

  let { children, title, actions, onClose }: Props = $props();
</script>

<Surface align="top" maxWidth={720} z={20} scrollBody={true} {onClose} {title} {actions}>
  {@render children()}
</Surface>
