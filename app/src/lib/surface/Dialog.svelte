<script lang="ts">
  import type { Snippet } from "svelte";
  import Surface from "./Surface.svelte";

  // Centered, small (320), no scroll body. Children stack with the
  // 12px rhythm; DialogTitle / DialogDescription / ButtonRow slot in
  // cleanly. Sits above Overlay (z-30 vs z-20).

  interface Props {
    children: Snippet;
    onClose?: () => void;
  }

  let { children, onClose }: Props = $props();
</script>

<Surface align="center" maxWidth={320} z={30} scrollBody={false} {onClose}>
  {@render children()}
</Surface>
