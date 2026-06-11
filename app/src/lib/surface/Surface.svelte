<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import ScrollArea from "@/lib/ui/ScrollArea.svelte";
  import { overlays } from "./overlays.svelte";

  // Shared base for Dialog and Overlay. Owns: backdrop, click-outside,
  // panel chrome with optional title / scroll body / actions snippets.
  // Z-index applied via inline style, never as a `z-${n}` Tailwind class
  // (Tailwind purges what it doesn't see, so dynamic class names break
  // silently in production builds).
  //
  // scrollBody is independent of `actions`: an Overlay without actions
  // still wants its body scrollable; a Dialog with two-button row at the
  // bottom doesn't. Keeping them separate avoids surprising callers.
  //
  // Registers with the overlays counter on mount so future code can read
  // `overlays.any` without depending on individual visibility flags.

  interface Props {
    align: "center" | "top";
    maxWidth: number;
    z: number;
    scrollBody: boolean;
    onClose?: () => void;
    title?: Snippet;
    actions?: Snippet;
    children: Snippet;
  }

  let { align, maxWidth, z, scrollBody, onClose, title, actions, children }: Props = $props();

  // Bump the global counter on mount, decrement on unmount. Use onMount
  // (not $effect): overlays.open() reads + writes overlays.count, which
  // would trip Svelte's read+write self-loop guard inside an effect.
  // onMount runs once and is unrelated to the reactivity graph.
  onMount(() => overlays.open());

  // Pre-compose layout classes so Tailwind sees them statically. Switch
  // by `align` rather than building a class string with interpolation.
  const alignClass = $derived(
    align === "center" ? "grid place-items-center" : "flex items-start justify-center",
  );
</script>

<!-- 16px gap lives as PADDING on the backdrop so the panel can never
     leak past the backdrop bounds. Dismiss on pointerdown that BEGINS on the
     backdrop itself (target === currentTarget), so a text-selection drag that
     starts inside the panel and releases past its edge doesn't close it. -->
<div
  class={["absolute inset-0 bg-backdrop p-4 backdrop-blur-[2px]", alignClass]}
  style="z-index: {z}"
  role="presentation"
  onpointerdown={(e) => {
    if (e.target === e.currentTarget) onClose?.();
  }}
>
  <!-- onkeydown is intentionally a no-op: role="dialog" with tabindex={-1}
       trips Svelte's a11y "interactive role needs a key handler" warning.
       All key handling actually flows through the kb layer (KbLayer pushes
       a layer; bindings live in that layer's bindings.ts), so the panel
       itself has nothing to do with native keydown. -->
  <div
    class="flex max-h-full w-full flex-col overflow-hidden rounded-xl border-2 border-border bg-bg"
    style="max-width: {maxWidth}px"
    role="dialog"
    tabindex={-1}
    onkeydown={() => {}}
  >
    {#if title}
      <div class="shrink-0 px-4 pt-4 pb-3">
        {@render title()}
      </div>
    {/if}

    {#if scrollBody}
      <!-- padBottom={8} when an actions footer exists: the scrollbar
           track shares this number, so the thumb stops 8px above the
           actions divider, same gap Browser keeps above the StatusBar
           (Browser.svelte's ScrollArea also uses padBottom=8). When
           there's no actions footer, fall back to 16 (symmetric with
           padX, since the bottom edge is the panel itself). -->
      <ScrollArea
        class="min-h-0 flex-1"
        padX={16}
        padTop={title ? 0 : 16}
        padBottom={actions ? 8 : 16}
      >
        {@render children()}
      </ScrollArea>
    {:else}
      <!-- Non-scrolling body: stack children directly with the panel's
           rhythm (gap-3 between siblings, p-4 around). Dialog uses this
           path so DialogTitle / DialogDescription / ButtonRow stack with
           the 12px rhythm without extra wrappers. -->
      <div class="flex flex-col gap-3 p-4">
        {@render children()}
      </div>
    {/if}

    {#if actions}
      <!-- border-t-2 border-border matches the panel's outer ring (same
           color + same weight) and the StatusBar separator: one
           consistent treatment for every region break. With the border
           doing the visual section-break work, vertical padding shrinks
           to pt-2 pb-2 (8px each side); the footer ends at 46px instead
           of leaving extra dead space above and below the buttons. -->
      <div class="shrink-0 border-t-2 border-border px-4 pt-2 pb-2">
        {@render actions()}
      </div>
    {/if}
  </div>
</div>
