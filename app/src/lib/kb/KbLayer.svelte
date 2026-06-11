<script lang="ts">
  import { onDestroy, untrack, type Snippet } from "svelte";
  import { kb, type Binding, type Capture, type LayerHandle } from "./kb.svelte";

  // Wraps a region of the UI in a kb scope. Push happens on script run,
  // pop on destroy, so layer lifetime exactly matches the {#if}/component
  // lifetime that renders this. `bindings`, `capture`, and `cursorId`
  // flow back into the layer reactively. Items self-register from the
  // rendered tree via the `kbItem` action on each interactive element.
  //
  // Each effect's body is wrapped in `untrack` so it tracks only its prop,
  // not the kb internals it mutates. Without this, kb.setCursor (called
  // from a key binding) would write layer.cursorId, causing the cursorId
  // effect to re-fire and reset cursor back to the prop value, silently
  // undoing every h/l/j/k key press in layers that pass `cursorId`.
  interface Props {
    name: string;
    bindings: Record<string, Binding>;
    cursorId?: string | null;
    capture?: Capture;
    handle?: LayerHandle;
    children: Snippet;
  }

  let {
    name,
    bindings,
    cursorId,
    capture = "matched",
    handle = $bindable(),
    children,
  }: Props = $props();

  // Push reads props once at component init. The three sync effects below
  // keep the layer in sync after that. untrack here makes the intentional
  // initial read explicit (silences svelte/state_referenced_locally).
  handle = untrack(() => kb.push({ name, bindings, cursorId, capture }));

  $effect(() => {
    const b = bindings;
    untrack(() => kb.setBindings(handle!, b));
  });
  $effect(() => {
    const c = capture;
    untrack(() => kb.setCapture(handle!, c));
  });
  $effect(() => {
    const c = cursorId;
    if (c === undefined) return;
    untrack(() => kb.setCursorOn(handle!, c));
  });

  onDestroy(() => kb.remove(handle!));
</script>

<div data-kb-layer={handle!.id} class="contents">
  {@render children()}
</div>
