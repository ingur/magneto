<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import { kbItem, type KbItemInit } from "@/lib/kb/kb.svelte";

  interface Props {
    checked?: boolean;
    kbItem?: KbItemInit;
  }

  let { checked = $bindable(false), kbItem: kbItemInit }: Props = $props();
</script>

<!-- Square control; click toggles, the kbItem action wires pointerdown→cursor.
     The default activate (kb.activate) calls .click() on this button → toggles.
     Checked tints the box with the success role (bg-success/20 + a success
     check), so "yes/enabled" reads at a glance instead of leaning on a small
     icon. The ring carries the cursor signal; resting hover only changes the
     background, so cursor and hover stay distinct. -->
<button
  type="button"
  tabindex={-1}
  use:kbItem={kbItemInit}
  onclick={() => (checked = !checked)}
  class={[
    "grid size-8 place-items-center rounded ring-1 ring-inset",
    checked
      ? "bg-success/20 ring-success/30"
      : "bg-panel ring-raised not-data-[kb-cursor]:hover:bg-raised/30",
    "data-[kb-cursor]:ring-2 data-[kb-cursor]:ring-cursor",
  ]}
>
  {#if checked}
    <Check size={14} class="text-success" />
  {/if}
</button>
