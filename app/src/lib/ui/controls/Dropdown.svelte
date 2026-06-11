<script lang="ts">
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import { kb, kbItem, type Binding, type KbItemInit, type LayerHandle } from "@/lib/kb/kb.svelte";
  import { useScrollFollowCursor } from "@/lib/kb/use-scroll-follow-cursor.svelte";
  import Popover from "@/lib/popover/Popover.svelte";
  import { getBounds } from "@/lib/popover/bounds";
  import KbLayer from "@/lib/kb/KbLayer.svelte";
  import ScrollArea from "@/lib/ui/ScrollArea.svelte";
  import { widths, type Size } from "./size";

  type DropdownOption = string | { value: string; label: string };

  // Value-picker dropdown. Anchors below the trigger (matches its width)
  // and flips above when there's no room. closeOnRightClickOutside=true
  // (any pointerdown outside dismisses), repositionOnScrollResize=true
  // (so the menu tracks its trigger when ancestors scroll, e.g. inside
  // an Overlay's ScrollArea).
  //
  // Bounds default to the closest [role="dialog"], so dropdowns inside
  // Settings clamp to the panel. Outside of a dialog, they fall back to
  // the viewport. Caller can override.

  interface Props {
    value?: string;
    options?: DropdownOption[];
    open?: boolean;
    size?: Size;
    kbItem?: KbItemInit;
    bounds?: () => DOMRect;
    // Fires with the option under the pointer or kb cursor while the menu
    // is open; fires with null when the menu closes by any path. Callers
    // use this for live preview without committing the value.
    onPreview?: (value: string | null) => void;
  }

  let {
    value = $bindable(""),
    options = [],
    open = $bindable(false),
    size = "sm",
    kbItem: kbItemInit,
    bounds,
    onPreview,
  }: Props = $props();

  let triggerEl: HTMLButtonElement | undefined = $state();
  let menuHandle = $state<LayerHandle | undefined>();

  useScrollFollowCursor(() => menuHandle);

  const items = $derived(
    options.map((opt) => (typeof opt === "string" ? { value: opt, label: opt } : opt)),
  );
  const selectedLabel = $derived(items.find((item) => item.value === value)?.label ?? value);

  const defaultBounds = () => getBounds(triggerEl ?? null, '[role="dialog"]');

  function select(next: string) {
    value = next;
    closeMenu();
  }

  function closeMenu() {
    open = false;
  }

  const menuBindings: Record<string, Binding> = {
    j: { label: "navigate", priority: 30, run: () => kb.next() },
    k: { label: "navigate", priority: 30, run: () => kb.prev() },
    enter: { label: "select", priority: 60, run: () => kb.activate() },
    space: { run: () => kb.activate() },
    escape: { label: "cancel", priority: 80, clickable: true, run: () => closeMenu() },
  };

  // Drive preview off the open flag so it fires on every close path (option
  // click, esc, outside click, trigger toggle): null when closed, the kb cursor
  // while open. Mouse hover fires onPreview separately via option onmouseenter.
  $effect(() => {
    if (!open) {
      onPreview?.(null);
      return;
    }
    const c = kb.cursor();
    if (c) onPreview?.(c);
  });
</script>

<!-- Trigger: standard Setting-control ring pattern. data-kb-cursor stays
     set on the trigger while the menu is open (the parent layer's cursor
     hasn't moved off this dropdown), so the cursor ring naturally remains
     visible; no separate `open` styling needed. -->
<button
  bind:this={triggerEl}
  type="button"
  tabindex={-1}
  use:kbItem={kbItemInit}
  onclick={() => (open = !open)}
  class={[
    "flex h-8 w-full min-w-0 items-center justify-between rounded bg-panel px-2 text-sm text-fg outline-none ring-1 ring-raised ring-inset",
    widths[size],
    "not-data-[kb-cursor]:hover:bg-raised/30",
    "data-[kb-cursor]:ring-2 data-[kb-cursor]:ring-cursor",
  ]}
>
  <span class="truncate">{selectedLabel}</span>
  <ChevronDown
    size={14}
    class={["shrink-0 text-muted transition-transform", open && "rotate-180"]}
  />
</button>

<!-- Menu: portaled by Popover. Plain popup panel, same subtle ring as a
     resting Setting control. Each option self-registers via kbItem on the
     dropdown's pushed layer; cursor highlight is data-[kb-cursor]:bg-cursor/15. -->
{#if triggerEl && items.length > 0}
  <Popover
    bind:open
    anchor={{ kind: "rect", el: triggerEl, mode: "dropdown" }}
    est={{ width: 0, height: 240 }}
    bounds={bounds ?? defaultBounds}
    closeOnRightClickOutside={true}
    repositionOnScrollResize={true}
    trigger={triggerEl}
    matchTriggerWidth={true}
  >
    {#snippet children({ maxHeight }: { close: () => void; maxHeight: number | null })}
      <KbLayer name="dropdown" bindings={menuBindings} cursorId={value} bind:handle={menuHandle}>
        <div
          class="flex flex-col overflow-hidden rounded bg-panel ring-1 ring-raised ring-inset"
          style={maxHeight !== null ? `max-height: ${maxHeight}px` : ""}
        >
          <ScrollArea
            class="min-h-0 flex-1"
            padX={0}
            padTop={0}
            padBottom={0}
            thumb={4}
            thumbInset={0}
            thumbGap={0}
            reserveThumbSpace={false}
          >
            <div class="flex flex-col">
              {#each items as opt (opt.value)}
                <button
                  type="button"
                  tabindex={-1}
                  use:kbItem={{ id: opt.value, activate: () => select(opt.value) }}
                  onclick={() => select(opt.value)}
                  onmouseenter={() => onPreview?.(opt.value)}
                  class={[
                    "flex h-8 shrink-0 items-center px-2 text-left text-sm text-fg",
                    "not-data-[kb-cursor]:hover:bg-raised/30",
                    "data-[kb-cursor]:bg-cursor/15",
                  ]}
                >
                  <span class="truncate">{opt.label}</span>
                </button>
              {/each}
            </div>
          </ScrollArea>
        </div>
      </KbLayer>
    {/snippet}
  </Popover>
{/if}
