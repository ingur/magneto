<script lang="ts">
  import type { Snippet } from "svelte";

  // The atomic unit of the StatusBar. Anything visible in the bar (a stat
  // line, a marked-count, a kb hint) is a StatusItem with a priority.
  // The bar's fit algorithm sorts all StatusItems across both sides by
  // priority and hides the lowest first when space runs out, so a
  // high-priority left item can survive over a low-priority right hint
  // and vice versa, regardless of which side they live on.
  //
  // Conventional priority ranges:
  //   0-30   nice-to-have (sub-stats, well-known navigation hints)
  //   40-60  useful context (counts, action hints)
  //   70-90  important (marked count, close hints)
  //   100    critical (active filter / search input)
  // Default 50.
  interface Props {
    priority?: number;
    class?: string;
    children: Snippet;
  }

  let { priority = 50, class: klass = "", children }: Props = $props();
</script>

<span data-priority={priority} class={["whitespace-nowrap", klass]}>
  {@render children()}
</span>
