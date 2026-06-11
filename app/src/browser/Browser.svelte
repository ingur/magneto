<script lang="ts">
  import Cable from "@lucide/svelte/icons/cable";
  import ClipboardPaste from "@lucide/svelte/icons/clipboard-paste";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import { tick, untrack } from "svelte";

  import { kb, type LayerHandle } from "@/lib/kb/kb.svelte";
  import { useScrollFollowCursor } from "@/lib/kb/use-scroll-follow-cursor.svelte";
  import KbLayer from "@/lib/kb/KbLayer.svelte";
  import ScrollArea from "@/lib/ui/ScrollArea.svelte";
  import { menus } from "@/lib/popover/menus.svelte";
  import { getBounds } from "@/lib/popover/bounds";
  import Menu, { type MenuItem } from "@/lib/ui/controls/Menu.svelte";
  import { toast } from "@/lib/feedback/toasts/toasts.svelte";

  import { daemon } from "@/daemon/client.svelte";
  import { revealPath } from "@/daemon/tauri";
  import { nav } from "@/torrents/nav.svelte";
  import { data } from "@/torrents/data.svelte";
  import * as actions from "@/torrents/actions";
  import { runAddPicked, runPasteClipboard } from "@/torrents/add";
  import { idToInfoHash } from "@/torrents/ids";
  import type { Row as RowModel } from "@/torrents/projection";
  import Row from "./Row.svelte";
  import { browserBindings } from "./bindings";
  import { DragSelect } from "./drag-select.svelte";

  // ONE persistent KbLayer. Folder navigation swaps the visible rows but
  // never remounts the layer; rows self-register via kbItem in DOM order.
  // Browser owns cursor landing (navTick effect) + scroll-follow; nav owns
  // all data/selection; the keymap lives in ./bindings.

  const rows = $derived(nav.currentRows);
  const connecting = $derived(daemon.status !== "connected" && rows.length === 0);
  const emptyTitle = $derived(
    nav.filter.active
      ? "No matches"
      : nav.pathIds.length === 0
        ? "No torrents yet"
        : "Empty folder",
  );
  const emptyDescription = $derived(
    nav.filter.active
      ? `Nothing matches “${nav.filter.query}”`
      : nav.pathIds.length === 0
        ? "Drag a .torrent or paste a magnet to get started"
        : "There are no files here",
  );

  let handle = $state<LayerHandle | undefined>();

  // Torrent-open is async (get_torrent); subfolders re-project the cached
  // files in-memory. Load before entering so the cursor lands on real rows.
  async function enterRow(row: RowModel) {
    if (!row.enterable) return;
    if (row.kind === "torrent") {
      const infoHash = idToInfoHash(row.id);
      if (infoHash && !data.detail(infoHash)) {
        const pathBefore = nav.pathIds;
        try {
          await data.load(infoHash);
        } catch (e) {
          toast.error(e instanceof Error ? e.message : String(e));
          return;
        }
        // Navigation moved while the fetch was in flight (a second enter);
        // pushing now would corrupt the path. pathIds is reassigned on every
        // nav, so a reference change means we moved.
        if (nav.pathIds !== pathBefore) return;
      }
    }
    nav.enter(row.id, row.id);
  }

  function toggleSelectAll() {
    const ids = rows.map((r) => r.id);
    if (ids.length === 0) return;
    if (ids.every((id) => nav.isMarked(id))) nav.unmarkAll(ids);
    else nav.markAll(ids);
  }

  // l/→ opens the cursored container; a leaf is a clean no-op (never plays).
  // Distinct from enter, which activates (a leaf plays); p is the explicit play.
  function navOpen() {
    const row = rows.find((r) => r.id === kb.cursor());
    if (row?.enterable) void enterRow(row);
  }

  // Rubber-band selection: pointer handlers attach to the list wrapper below;
  // escape routes through the layer bindings so cancel wins over clear.
  const dragSelect = new DragSelect();

  const bindings = browserBindings({
    getHandle: () => handle,
    toggleSelectAll,
    navOpen,
    cancelDragSelect: () => dragSelect.cancel(),
  });

  // Empty-space context menu. Row right-clicks stop propagation, so this only
  // sees the gaps, the space below the list, and the empty state.
  let spaceEl = $state<HTMLDivElement | undefined>();
  let spaceMenuOpen = $state(false);
  let spaceMenuAnchor = $state<{ kind: "point"; x: number; y: number } | null>(null);

  const spaceMenuItems: MenuItem[] = [
    {
      id: "paste-magnet",
      label: "Paste magnet link",
      icon: ClipboardPaste,
      run: () => void runPasteClipboard(),
    },
    {
      id: "add-torrent-file",
      label: "Add torrent file…",
      icon: FilePlus,
      run: () => void runAddPicked(),
    },
    {
      id: "open-downloads",
      label: "Open downloads folder",
      icon: FolderOpen,
      divider: true,
      run: () => void openDownloadsFolder(),
    },
  ];

  async function openDownloadsFolder() {
    const dir = daemon.config?.downloads.dir;
    if (!dir) {
      toast.error("Not connected to the daemon");
      return;
    }
    try {
      await revealPath(dir, "folder");
    } catch (e) {
      toast.error(
        `Couldn't open the downloads folder: ${e instanceof Error ? e.message : String(e)}`,
      );
    }
  }

  const spaceMenuBounds = () => getBounds(spaceEl ?? null, "data-menu-bounds");

  // Plain click = "select only this": collapse the marks (the cursor already
  // moved via kbItem). A band release's synthesized click never gets here;
  // the drag controller swallows it.
  function onListClick(e: MouseEvent) {
    if (e.ctrlKey || e.metaKey || e.shiftKey) return;
    const target = e.target as Element;
    if (target.closest("button") || target.closest("[data-scroll-thumb]")) return;
    nav.clearSelection();
  }

  // Two-step toggle, mirroring the row menus.
  function openSpaceMenu(e: MouseEvent) {
    e.preventDefault();
    if (menus.isAnyOpen) {
      menus.closeAny();
      return;
    }
    spaceMenuAnchor = { kind: "point", x: e.clientX, y: e.clientY };
    spaceMenuOpen = true;
  }

  // Land the cursor on each navigation, AFTER the new rows mount (tick) so
  // their kbItems have registered. setCursorOn clamps null/stale to the first
  // row and bumps cursorTick, which scroll-follow picks up.
  let lastNavTick = -1;
  $effect(() => {
    const navTick = nav.navTick;
    if (!handle || navTick === lastNavTick) return;
    lastNavTick = navTick;
    const h = handle;
    const target = untrack(() => nav.initialCursorId);
    void tick().then(() => kb.setCursorOn(h, target));
  });

  useScrollFollowCursor(() => handle);
</script>

<KbLayer name="browser" {bindings} bind:handle>
  <!-- relative + overflow-hidden: positioning context and clip for the band
       rect, which renders in viewport-local coordinates. -->
  <div
    bind:this={spaceEl}
    data-menu-trigger-area
    role="presentation"
    onclick={onListClick}
    oncontextmenu={openSpaceMenu}
    onpointerdown={dragSelect.onpointerdown}
    onpointermove={dragSelect.onpointermove}
    onpointerup={dragSelect.onpointerup}
    onpointercancel={dragSelect.onpointercancel}
    onlostpointercapture={dragSelect.onlostpointercapture}
    class="relative flex min-h-0 flex-1 flex-col overflow-hidden"
  >
    <!-- padX intentionally left at ScrollArea's default 13 (aligns row content
         with the TopBar icon edges); only padBottom is overridden. -->
    <ScrollArea
      class="min-h-0 flex-1"
      padBottom={8}
      contentClass={rows.length === 0 ? "grid min-h-full" : ""}
    >
      {#if rows.length === 0}
        <div class="place-self-center text-center">
          {#if connecting}
            <!-- status is "disconnected" only for the pre-connect frame at
                 startup and during window close; render it as connecting. -->
            <div class="flex animate-pulse items-center justify-center gap-2 text-sm text-muted">
              <Cable size={16} strokeWidth={2} class="text-subtle" />
              <span>{daemon.status === "reconnecting" ? "Reconnecting…" : "Connecting…"}</span>
            </div>
          {:else}
            <div class="flex items-center justify-center gap-2 text-sm text-muted">
              <FolderOpen size={16} strokeWidth={2} class="text-subtle" />
              <span>{emptyTitle}</span>
            </div>
            <div class="mt-1 text-xs text-subtle">{emptyDescription}</div>
          {/if}
        </div>
      {:else}
        <div class="flex flex-col gap-2">
          {#each rows as row (row.id)}
            <Row
              {row}
              marked={nav.isMarked(row.id)}
              onmark={() => nav.toggleMark(row.id)}
              kbItem={{
                id: row.id,
                activate: row.enterable
                  ? () => void enterRow(row)
                  : () => void actions.runPlay(actions.rowTargets(row, false)),
              }}
            />
          {/each}
        </div>
      {/if}
    </ScrollArea>

    {#if dragSelect.rect}
      <div
        class="border-marked/40 bg-marked/10 pointer-events-none absolute rounded-[2px] border"
        style="left: {dragSelect.rect.left}px; top: {dragSelect.rect.top}px; width: {dragSelect.rect
          .width}px; height: {dragSelect.rect.height}px"
      ></div>
    {/if}
  </div>

  {#if spaceMenuAnchor}
    <Menu
      bind:open={spaceMenuOpen}
      anchor={spaceMenuAnchor}
      bounds={spaceMenuBounds}
      items={spaceMenuItems}
    />
  {/if}
</KbLayer>
