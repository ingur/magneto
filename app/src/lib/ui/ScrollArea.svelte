<script lang="ts">
  import { tick, type Snippet } from "svelte";

  interface Props {
    children: Snippet;
    class?: string;
    contentClass?: string;

    // Content padding
    padX?: number;
    padL?: number;
    padR?: number;
    padTop?: number;
    padBottom?: number;

    // Scrollbar thumb
    thumb?: number;
    thumbInset?: number;
    thumbGap?: number;
    reserveThumbSpace?: boolean;
  }

  // Props split into two groups: content padding, and the thumb's geometry.
  //
  //   Content padding
  //     padX                       shorthand for padL & padR
  //     padL / padR                per-side overrides (default: padX)
  //     padTop / padBottom
  //
  //   Scrollbar thumb
  //     thumb                      thumb width
  //     thumbInset                 space to the RIGHT of the thumb
  //                                (between thumb and container edge)
  //     thumbGap                   space to the LEFT of the thumb
  //                                (between thumb and content, when visible)
  //
  // thumbInset and thumbGap default to padR, so by default the thumb lives
  // in a mirror of the right content pad (symmetric feel). Override them
  // to decouple the two, e.g. hug a thin thumb to the edge while keeping
  // a generous content pad:
  //   <ScrollArea padX={16} thumb={4} thumbInset={4} thumbGap={6}>
  //
  // Content right pad is derived:
  //   thumb hidden                      → padR
  //   thumb visible + reserve space     → thumbInset + thumb + thumbGap
  //   thumb visible + overlay scrollbar → padR
  // Transitions smoothly, but only AFTER first measurement, so opening
  // a ScrollArea with already-overflowing content doesn't animate the
  // padding-right into place on mount.
  //
  // padX default 13 aligns content edges with TopBar icon edges:
  //   bar px-1.5 (6) + (btn 28 − icon 14) / 2 = 6 + 7 = 13
  let {
    children,
    class: klass = "",
    contentClass = "",
    padX = 13,
    padL,
    padR,
    padTop = 0,
    padBottom = 0,
    thumb = 8,
    thumbInset,
    thumbGap,
    reserveThumbSpace = true,
  }: Props = $props();

  const MIN_THUMB = 24;

  const lPad = $derived(padL ?? padX);
  const rPad = $derived(padR ?? padX);
  const tInset = $derived(thumbInset ?? rPad);
  const tGap = $derived(thumbGap ?? rPad);

  let container: HTMLDivElement | undefined = $state();
  let thumbHeight = $state(0);
  let thumbTop = $state(0);
  let visible = $state(false);
  let dragging = $state(false);
  let scrolling = $state(false);
  let mounted = $state(false);

  const rightPad = $derived(visible && reserveThumbSpace ? tInset + thumb + tGap : rPad);
  const active = $derived(dragging || scrolling);

  let scrollIdleTimer: ReturnType<typeof setTimeout> | undefined;

  function update() {
    if (!container) return;
    const { scrollTop, scrollHeight, clientHeight } = container;
    const track = clientHeight - padTop - padBottom;
    if (scrollHeight <= clientHeight + 1 || track <= 0) {
      visible = false;
      return;
    }
    visible = true;
    thumbHeight = Math.min(track, Math.max(MIN_THUMB, (clientHeight / scrollHeight) * track));
    const scrollRange = scrollHeight - clientHeight;
    const ratio = scrollRange > 0 ? scrollTop / scrollRange : 0;
    thumbTop = padTop + ratio * (track - thumbHeight);
  }

  function onScroll() {
    update();
    scrolling = true;
    clearTimeout(scrollIdleTimer);
    scrollIdleTimer = setTimeout(() => (scrolling = false), 150);
  }

  $effect(() => {
    if (!container) return;
    update();
    // Flip on transitions only after the initial measurement has flushed.
    tick().then(() => (mounted = true));
    const ro = new ResizeObserver(update);
    ro.observe(container);
    const mo = new MutationObserver(update);
    mo.observe(container, { childList: true, subtree: true, characterData: true });
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  });

  let dragStartY = 0;
  let dragStartScrollTop = 0;

  function onPointerDown(e: PointerEvent) {
    if (!container) return;
    e.preventDefault();
    dragging = true;
    dragStartY = e.clientY;
    dragStartScrollTop = container.scrollTop;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging || !container) return;
    const { scrollHeight, clientHeight } = container;
    const maxThumbTop = clientHeight - padTop - padBottom - thumbHeight;
    if (maxThumbTop <= 0) return;
    const dy = e.clientY - dragStartY;
    container.scrollTop = dragStartScrollTop + (dy / maxThumbTop) * (scrollHeight - clientHeight);
  }

  function onPointerUp(e: PointerEvent) {
    dragging = false;
    (e.currentTarget as Element).releasePointerCapture(e.pointerId);
  }
</script>

<!-- Root is a flex-col shell sized entirely by the classes the consumer
     passes in (klass): e.g. `flex-1 min-h-0` to fill, or `flex-auto min-h-0`
     to grow-to-content within a cap. The inner scroll container is flex-1
     inside, so it always fills whatever the root actually became: no
     %-height cascade, no h-full gymnastics. -->
<div class={["relative flex flex-col", klass]}>
  <div
    bind:this={container}
    onscroll={onScroll}
    data-scroll-viewport
    class="min-h-0 flex-1 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
    style="scroll-padding-block: {padTop}px {padBottom}px"
  >
    {#if visible}
      <!-- Thumb is a DESCENDANT of the scroll container, so wheel events
           bubble natively into it and the browser's own wheel-scroll animator
           handles smoothness; no manual scrollTop forwarding needed.
           Sticky + h-0 makes an invisible pin at the top of the scrollport;
           the thumb absolute-positions inside. -->
      <div class="pointer-events-none sticky top-0 z-10 h-0">
        <div
          role="presentation"
          data-scroll-thumb
          class={[
            "pointer-events-auto absolute touch-none select-none rounded-full transition-colors duration-150",
            active ? "bg-scrollbar-active" : "bg-scrollbar hover:bg-scrollbar-active",
          ]}
          style="top: {thumbTop}px; right: {tInset}px; width: {thumb}px; height: {thumbHeight}px"
          onpointerdown={onPointerDown}
          onpointermove={onPointerMove}
          onpointerup={onPointerUp}
        ></div>
      </div>
    {/if}

    <div
      class={[mounted ? "transition-[padding-right] duration-150 ease-out" : "", contentClass]}
      style="padding: {padTop}px {rightPad}px {padBottom}px {lPad}px"
    >
      {@render children()}
    </div>
  </div>
</div>
