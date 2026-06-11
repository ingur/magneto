<script lang="ts">
  import type { Snippet } from "svelte";

  // Generic StatusBar fit shell. Two sides via the `left` and `right`
  // snippets. Caller renders whatever widgets it wants; widgets emit
  // StatusItem(s), small inline spans tagged with data-priority.
  //
  // The fit algorithm operates on those items uniformly across both sides:
  // when the bar overflows, it hides the lowest-priority items first,
  // regardless of side. Adding a new widget = render it in one of the
  // snippets; the algorithm picks it up automatically via the
  // [data-priority] selector.
  //
  // Why DOM-introspection rather than a registry: widgets self-determine
  // visibility (they render nothing when their state isn't active), so the
  // rendered DOM is already the source of truth for "what's currently
  // showing". Querying it sidesteps a parallel registry that has to be
  // kept in sync with what's actually in the tree.
  //
  // Layout: the left side is `flex-1 min-w-0` (growable + truncating), the right
  // `shrink-0`. A widget may render a greedy element (e.g. a search input) that
  // fills the left WITHOUT a data-priority tag; it then lives OUTSIDE the
  // priority-hide budget and just truncates. Keep such growable elements on the
  // LEFT; an untagged greedy element on the right would never hide and would push
  // the measured hints off-screen.

  interface Props {
    left?: Snippet;
    right?: Snippet;
  }

  let { left, right }: Props = $props();

  const GAP = 12; // matches gap-3 between items within a side
  const PADDING_X = 24; // matches px-3 on both bar edges

  let bar: HTMLElement;
  let leftEl: HTMLElement;
  let rightEl: HTMLElement;

  type Side = "left" | "right";
  type Item = { el: HTMLElement; side: Side; priority: number; width: number };

  function collect(): Item[] {
    const grab = (root: HTMLElement, side: Side): Item[] =>
      Array.from(root.querySelectorAll<HTMLElement>("[data-priority]")).map((el) => ({
        el,
        side,
        priority: Number(el.dataset.priority ?? 50),
        width: 0,
      }));
    return [...grab(leftEl, "left"), ...grab(rightEl, "right")];
  }

  function totalWidth(items: Item[], hidden: Set<HTMLElement>): number {
    const lhs = items.filter((i) => i.side === "left" && !hidden.has(i.el));
    const rhs = items.filter((i) => i.side === "right" && !hidden.has(i.el));
    const sumSide = (xs: Item[]) =>
      xs.reduce((s, i) => s + i.width, 0) + Math.max(0, xs.length - 1) * GAP;
    const sep = lhs.length > 0 && rhs.length > 0 ? GAP : 0;
    return sumSide(lhs) + sumSide(rhs) + sep;
  }

  function refit() {
    if (!bar || !leftEl || !rightEl) return;

    const items = collect();
    if (items.length === 0) return;

    // Reveal everything before measuring: natural width is what we
    // need, not the previously-clipped width.
    items.forEach(({ el }) => (el.style.display = ""));
    items.forEach((i) => (i.width = i.el.offsetWidth));

    const available = bar.clientWidth - PADDING_X;

    // Drop order: lowest priority first. Tied items fall back to DOM order
    // (left side then right side, top-to-bottom within each).
    const order = [...items].sort((a, b) => a.priority - b.priority);
    const hidden = new Set<HTMLElement>();
    let total = totalWidth(items, hidden);
    for (const item of order) {
      if (total <= available) break;
      hidden.add(item.el);
      total = totalWidth(items, hidden);
    }

    items.forEach(({ el }) => {
      if (hidden.has(el)) el.style.display = "none";
    });
  }

  $effect(() => {
    if (!bar) return;
    const ro = new ResizeObserver(() => refit());
    ro.observe(bar);
    // Re-fit when widgets render/un-render or change content (kb hint
    // changes, marked count changes, etc.).
    const mo = new MutationObserver(() => refit());
    mo.observe(bar, { childList: true, subtree: true, characterData: true });
    refit();
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  });
</script>

<footer
  bind:this={bar}
  class="flex h-7 shrink-0 items-center justify-between border-t-2 border-border px-3 text-xs text-muted"
>
  <div bind:this={leftEl} class="flex min-w-0 flex-1 items-center gap-3">
    {#if left}{@render left()}{/if}
  </div>
  <div bind:this={rightEl} class="flex shrink-0 items-center gap-3">
    {#if right}{@render right()}{/if}
  </div>
</footer>
