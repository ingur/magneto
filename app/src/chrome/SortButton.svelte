<script lang="ts">
  import Activity from "@lucide/svelte/icons/activity";
  import ArrowDownAZ from "@lucide/svelte/icons/arrow-down-a-z";
  import ArrowDownNarrowWide from "@lucide/svelte/icons/arrow-down-narrow-wide";
  import ArrowDownWideNarrow from "@lucide/svelte/icons/arrow-down-wide-narrow";
  import ArrowDownZA from "@lucide/svelte/icons/arrow-down-z-a";
  import ArrowUpDown from "@lucide/svelte/icons/arrow-up-down";
  import type { Component } from "svelte";
  import { menus } from "@/lib/popover/menus.svelte";
  import { nav } from "@/torrents/nav.svelte";
  import { sortLabels, type SortMode } from "@/torrents/types";
  import Menu, { type MenuItem } from "@/lib/ui/controls/Menu.svelte";

  // Sort button: sits in TopBar nav cluster, opens a popover listing
  // sort modes. `open` is bindable so a hotkey in the list view can toggle
  // the same popover via `bind:open`. The button's icon reflects the active
  // sort mode, so the bar itself shows you what's active without a
  // checkmark in the popover.

  interface Props {
    open?: boolean;
    disabled?: boolean;
  }

  let { open = $bindable(false), disabled = false }: Props = $props();

  let triggerEl: HTMLButtonElement | undefined = $state();
  const buttonDisabled = $derived(disabled && !open);

  // 'added' (default) gets the generic ArrowUpDown, so first-time users see
  // a recognisable "sort" icon rather than wondering "why a clock?". The
  // explicit modes get glyphs that mirror their label (A→Z, Z→A, etc.).
  const SORT_ICONS: Record<SortMode, Component> = {
    added: ArrowUpDown,
    "name-asc": ArrowDownAZ,
    "name-desc": ArrowDownZA,
    "size-desc": ArrowDownWideNarrow,
    "size-asc": ArrowDownNarrowWide,
    status: Activity,
  };

  const Icon = $derived(SORT_ICONS[nav.sortMode]);

  // Same icons reused on the menu rows; matches the ⋯ menu's icon-on-left
  // pattern so the bar icon and its menu read as the same vocabulary.
  const items: MenuItem[] = $derived(
    (Object.keys(sortLabels) as SortMode[]).map((mode) => ({
      id: mode,
      label: sortLabels[mode],
      icon: SORT_ICONS[mode],
      run: () => {
        nav.sortMode = mode;
      },
    })),
  );

  function toggle() {
    if (buttonDisabled) return;
    if (open) {
      open = false;
      return;
    }
    if (menus.isAnyOpen) menus.closeAny();
    open = true;
  }
</script>

<button
  bind:this={triggerEl}
  type="button"
  tabindex={-1}
  disabled={buttonDisabled}
  onclick={toggle}
  class={[
    "grid size-7 place-items-center",
    buttonDisabled ? "text-disabled" : open ? "text-fg" : "hover:text-fg",
  ]}
>
  <Icon size={14} strokeWidth={2} />
</button>

{#if triggerEl}
  <Menu
    bind:open
    anchor={{ kind: "rect", el: triggerEl }}
    trigger={triggerEl}
    toggleKey="`"
    minWidth={144}
    {items}
  />
{/if}
