<script lang="ts" module>
  import type { Component } from "svelte";

  // Menu items: id + label + optional icon + optional tint for destructive
  // actions. `run` does the work; the menu closes itself after activation.
  export type MenuItem = {
    id: string;
    label: string;
    icon?: Component;
    tint?: "danger";
    divider?: boolean; // shows a thin divider ABOVE this item (group break)
    run: () => void;
  };
</script>

<script lang="ts">
  import { kb, kbItem, type Binding } from "@/lib/kb/kb.svelte";
  import { menus } from "@/lib/popover/menus.svelte";
  import Popover from "@/lib/popover/Popover.svelte";
  import { getBounds } from "@/lib/popover/bounds";
  import KbLayer from "@/lib/kb/KbLayer.svelte";

  // Action-menu popover. Two anchor flavors:
  //   { kind: 'rect', el }   - aligns to a trigger button's edges
  //   { kind: 'point', x, y } - cursor / right-click placement
  //
  // Right-click outside is ignored (closeOnRightClickOutside=false) so
  // the global contextmenu coordinator can implement the two-step toggle:
  // a right-click while a menu is open closes it without reopening at the
  // new spot; the user has to right-click again where they actually want
  // the menu.

  type Anchor = { kind: "rect"; el: HTMLElement } | { kind: "point"; x: number; y: number };

  interface Props {
    open?: boolean;
    anchor: Anchor;
    items: MenuItem[];
    bounds?: () => DOMRect; // defaults to viewport
    trigger?: HTMLElement | null;
    // Key that closes the menu when pressed inside it, symmetric with
    // the open shortcut on the parent layer (e.g. `m` for ⋯ menus, `s`
    // for sort). Hidden hint; discoverable via the help overlay.
    toggleKey?: string;
    minWidth?: number;
    onClose?: () => void;
  }

  let {
    open = $bindable(false),
    anchor,
    items,
    bounds = () => getBounds(null),
    trigger = null,
    toggleKey = "m",
    minWidth = 176,
    onClose,
  }: Props = $props();

  // Estimates buffer the worst-case popover dims for the flip-vs-stay
  // decision in positionAnchored / positionAtPoint. 240×160 covers a
  // typical 5-item menu with room to spare; right-click near the right
  // edge picks "open left" rather than spilling past the bound.
  const EST = { width: 240, height: 160 };

  function close() {
    open = false;
  }
  function activate(item: MenuItem) {
    item.run();
    close();
  }

  // Fire onClose on a real open -> close transition (any path: select, esc,
  // toggle key, outside click), not on the initial closed mount.
  let wasOpen = false;
  $effect(() => {
    if (open) {
      wasOpen = true;
    } else if (wasOpen) {
      wasOpen = false;
      onClose?.();
    }
  });

  const bindings: Record<string, Binding> = $derived({
    j: { label: "navigate", priority: 30, run: () => kb.next() },
    k: { label: "navigate", priority: 30, run: () => kb.prev() },
    enter: { label: "select", priority: 60, run: () => kb.activate() },
    space: { run: () => kb.activate() },
    escape: { label: "cancel", priority: 80, clickable: true, run: close },
    [toggleKey]: { run: close },
  });

  // Adapter: Popover accepts a discriminated anchor with `mode`. Menu's
  // rect anchor is always 'anchored' (flips by corner). The conditional
  // is read inside $derived so the popover re-positions if the trigger
  // element swaps (rare, but harmless).
  const popoverAnchor = $derived.by(() =>
    anchor.kind === "rect"
      ? ({ kind: "rect", el: anchor.el, mode: "anchored" } as const)
      : ({ kind: "point", x: anchor.x, y: anchor.y } as const),
  );

  // Register with the global menu coordinator while open, so anything in
  // the app can detect "a menu is open" and close it. Used by the global
  // right-click handler.
  $effect(() => {
    if (!open) return;
    return menus.register(close);
  });
</script>

{#if items.length > 0}
  <Popover
    bind:open
    anchor={popoverAnchor}
    est={EST}
    {bounds}
    closeOnRightClickOutside={false}
    repositionOnScrollResize={false}
    {trigger}
    {minWidth}
  >
    {#snippet children(_args: { close: () => void; maxHeight: number | null })}
      <KbLayer name="menu" {bindings} cursorId={items[0]?.id}>
        <div
          data-menu-instance
          class="flex flex-col overflow-hidden rounded bg-panel shadow ring-1 ring-raised"
        >
          {#each items as item (item.id)}
            {#if item.divider}
              <div class="h-px bg-raised"></div>
            {/if}
            <button
              type="button"
              tabindex={-1}
              use:kbItem={{ id: item.id, activate: () => activate(item) }}
              onclick={() => activate(item)}
              class={[
                "flex h-7 shrink-0 items-center gap-2 px-2 text-left text-xs",
                item.tint === "danger"
                  ? "text-danger not-data-[kb-cursor]:hover:bg-danger/20 data-[kb-cursor]:bg-danger/20"
                  : "text-fg not-data-[kb-cursor]:hover:bg-raised/30 data-[kb-cursor]:bg-cursor/15",
              ]}
            >
              {#if item.icon}
                {@const Icon = item.icon}
                <Icon size={14} class="shrink-0" />
              {/if}
              <span class="truncate">{item.label}</span>
            </button>
          {/each}
        </div>
      </KbLayer>
    {/snippet}
  </Popover>
{/if}
