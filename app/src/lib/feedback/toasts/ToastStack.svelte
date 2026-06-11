<script lang="ts">
  import { fade, fly } from "svelte/transition";
  import { toasts } from "./toasts.svelte";
  import Toast from "./Toast.svelte";

  // Stack lives bottom-right of the parent positioning context (in our
  // case the Browser+overlay region inside App.svelte), so toasts never
  // intrude on the StatusBar. flex-col-reverse means new toasts appear
  // at the bottom of the stack, so the eye lands on the most recent.
  // pointer-events: none on the wrapper lets clicks pass through the
  // empty area; each toast re-enables pointer events on itself.
</script>

<!-- items-end keeps each toast at its own content width: flex-col's
     default `align-items: stretch` would resize a short toast to match
     a longer one in the stack, which reads as inconsistent sizing. -->
<div
  class="pointer-events-none absolute right-2 bottom-2 z-40 flex flex-col-reverse items-end gap-2"
>
  {#each toasts.list as t (t.id)}
    <div
      class="pointer-events-auto"
      in:fly={{ x: 200, duration: 180 }}
      out:fade={{ duration: 120 }}
    >
      <Toast toast={t} onDismiss={() => toasts.dismiss(t.id)} />
    </div>
  {/each}
</div>
