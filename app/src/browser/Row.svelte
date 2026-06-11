<script lang="ts">
  import Bookmark from "@lucide/svelte/icons/bookmark";
  import BookmarkCheck from "@lucide/svelte/icons/bookmark-check";
  import Download from "@lucide/svelte/icons/download";
  import EllipsisVertical from "@lucide/svelte/icons/ellipsis-vertical";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Film from "@lucide/svelte/icons/film";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Link from "@lucide/svelte/icons/link";
  import Pause from "@lucide/svelte/icons/pause";
  import Upload from "@lucide/svelte/icons/upload";
  import Play from "@lucide/svelte/icons/play";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { tick } from "svelte";

  import { kb, kbItem, type KbItemInit } from "@/lib/kb/kb.svelte";
  import { menus } from "@/lib/popover/menus.svelte";
  import { getBounds } from "@/lib/popover/bounds";
  import Menu, { type MenuItem } from "@/lib/ui/controls/Menu.svelte";

  import * as actions from "@/torrents/actions";
  import type { Row } from "@/torrents/projection";
  import { formatBytes, formatEta, formatPercent, formatSpeed } from "./stats";
  import { registerRowActions } from "./row-actions";

  interface Props {
    row: Row;
    marked: boolean;
    onmark: () => void;
    kbItem: KbItemInit;
  }

  let { row, marked, onmark, kbItem: kbItemInit }: Props = $props();

  // Progress-bar fill by state: progress tokens only, never accents. Queued
  // keeps its partial-progress bar but in the idle tone: only the file actually
  // downloading shows the active fill, so one moving bar reads as "downloading".
  const FILL: Record<Row["state"], string> = {
    initializing: "bg-progress-idle",
    idle: "bg-progress-idle",
    queued: "bg-progress-idle",
    paused: "bg-progress-idle",
    downloading: "bg-progress-active",
    complete: "bg-progress-done",
    error: "bg-progress-error",
  };

  // Container rows (folders, multi-file torrents) get the folder glyph;
  // single-file torrents and plain files get the film glyph.
  const isContainer = $derived(
    row.kind === "folder" || (row.kind === "torrent" && (row.fileCount ?? 0) > 1),
  );

  const persisted = $derived(row.persisted === "all");
  const shared = $derived(row.shared === "all");

  // Compact, color-coded state word so every row (incl. files/folders) reads
  // its state. A downloading torrent shows its speed instead (the word is
  // redundant), and a seeding torrent shows "seeding".
  const STATE_LABEL: Record<Row["state"], { text: string; class: string }> = {
    downloading: { text: "downloading", class: "text-info" },
    queued: { text: "queued", class: "text-subtle" },
    paused: { text: "paused", class: "text-muted" },
    idle: { text: "idle", class: "text-subtle" },
    complete: { text: "complete", class: "text-success" },
    error: { text: "error", class: "text-danger" },
    initializing: { text: "initializing", class: "text-subtle" },
  };

  type Stat = { text: string; class?: string };
  const stats = $derived.by<Stat[]>(() => {
    const out: Stat[] = [{ text: formatBytes(row.size) }];
    if (row.state !== "complete") out.push({ text: formatPercent(row.progress) });

    // Speed + ETA: torrent rows only; folders/files have no attributable speed.
    const showSpeed =
      row.kind === "torrent" && row.state === "downloading" && (row.downloadSpeed ?? 0) > 0;
    if (showSpeed) {
      out.push({ text: `↓ ${formatSpeed(row.downloadSpeed!)}`, class: "text-info" });
      const eta = formatEta(row.remaining ?? 0, row.downloadSpeed!);
      if (eta) out.push({ text: `ETA ${eta}` });
    } else if (!(row.state === "complete" && row.isSeeding)) {
      out.push(STATE_LABEL[row.state]);
    }

    if (row.kind === "torrent" && row.isSeeding) {
      out.push({ text: "↑ seeding", class: "text-success" });
    }

    // Completion counts for aggregate rows (torrents + folders); a folder in
    // mixed activity also surfaces how many descendants are downloading.
    if ((row.kind === "torrent" || row.kind === "folder") && (row.fileCount ?? 0) > 0) {
      out.push({ text: `${row.completeCount ?? 0}/${row.fileCount}` });
      if (row.mixed && (row.downloadingCount ?? 0) > 0) {
        out.push({ text: `${row.downloadingCount}↓`, class: "text-info" });
      }
    }
    return out;
  });

  // Orchestration (toast / confirm / clear-selection) lives in actions so the
  // row buttons and the keyboard bindings invoke identical behavior. targets()
  // resolves at click time: the selection if this row is part of it, else just
  // this row.
  const targets = () => actions.rowTargets(row, marked);
  const playRow = () => actions.runPlay(targets());
  const togglePersist = () => actions.runTogglePersist(targets());

  // A complete torrent has no download direction of its own, but it can still
  // offer "download the rest" (some media never selected) or a seeding toggle.
  const downloadRemaining = $derived(actions.downloadRemaining(row));
  const seedingToggle = $derived(
    row.kind === "torrent" &&
      row.state === "complete" &&
      !downloadRemaining &&
      (row.isSeeding === true || row.isPaused === true),
  );
  // Direction follows what the click will act on (the selection when this row
  // is part of it), so the label always matches the action.
  const togglePauses = $derived(actions.pauseDirection(actions.rowTargets(row, marked)));

  // Persist is the always-visible blue row button (not in the menu). Delete
  // is destructive: bottom, under a divider, the only danger-tinted item.
  const menuItems: MenuItem[] = $derived([
    ...(row.state !== "complete"
      ? [
          {
            id: "pause-resume",
            // An errored row resumes too; the daemon retries with a re-check.
            label: togglePauses
              ? "Pause download"
              : row.state === "error"
                ? "Retry download"
                : "Resume download",
            icon: togglePauses ? Pause : Download,
            run: () => actions.runToggleDownload(targets()),
          } satisfies MenuItem,
        ]
      : downloadRemaining
        ? [
            {
              id: "pause-resume",
              label: "Download remaining",
              icon: Download,
              run: () => actions.runToggleDownload(targets()),
            } satisfies MenuItem,
          ]
        : []),
    ...(seedingToggle
      ? [
          {
            id: "seeding",
            label: row.isSeeding ? "Pause seeding" : "Resume seeding",
            icon: row.isSeeding ? Pause : Upload,
            run: () => actions.runToggleSeeding(targets()),
          } satisfies MenuItem,
        ]
      : []),
    {
      id: "share",
      label: shared ? "Make private" : "Make shared",
      icon: shared ? EyeOff : Eye,
      run: () => actions.runToggleShare(targets()),
    },
    {
      id: "reveal",
      label: "Reveal in folder",
      icon: FolderOpen,
      run: () => actions.runReveal(row),
    },
    {
      id: "copy-magnet",
      label: "Copy magnet link",
      icon: Link,
      run: () => actions.runCopyMagnet(targets()),
    },
    {
      id: "delete",
      label: "Delete",
      icon: Trash2,
      tint: "danger",
      divider: true,
      run: () => actions.runDelete(targets()),
    },
  ]);

  // Brief highlight on an action button when its keyboard shortcut fires, so
  // p/s feel like the cursor briefly landed there (the menu shows its own
  // open-state highlight). Cleared once the 150ms transition has settled.
  let flashed = $state<"play" | "save" | null>(null);
  function flash(action: "play" | "save") {
    flashed = action;
    setTimeout(() => {
      if (flashed === action) flashed = null;
    }, 450);
  }

  // Overflow menu state: anchor flavor only; Menu owns its positioning.
  let menuOpen = $state(false);
  let menuAnchor = $state<
    { kind: "rect"; el: HTMLElement } | { kind: "point"; x: number; y: number } | null
  >(null);
  let ellipsisEl: HTMLButtonElement | undefined = $state();

  function toggleMenuAtEllipsis() {
    if (menuOpen) {
      menuOpen = false;
      return;
    }
    if (!ellipsisEl) return;
    menuAnchor = { kind: "rect", el: ellipsisEl };
    menuOpen = true;
  }

  // Right-click toggles the menu. If any menu is open, close it without
  // reopening; the next right-click opens at the new spot (two-step).
  // stopPropagation keeps it out of the list's empty-space menu handler.
  async function openMenuAtMouse(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (menus.isAnyOpen) {
      menus.closeAny();
      return;
    }
    kb.setCursor(row.id);
    await tick();
    menuAnchor = { kind: "point", x: e.clientX, y: e.clientY };
    menuOpen = true;
  }

  const menuBounds = () => getBounds(ellipsisEl ?? null, "data-menu-bounds");

  // Let the browser layer's `m` binding open the cursored row's menu without
  // a DOM query.
  $effect(() => registerRowActions(row.id, { openMenu: toggleMenuAtEllipsis, flash }));
</script>

<!-- Single-click moves the cursor (kbItem pointerdown); double-click runs the
     row's primary action via kb.activate(id) so mouse and keyboard converge;
     right-click opens the overflow menu. -->
<div
  use:kbItem={kbItemInit}
  data-menu-trigger-area
  role="button"
  tabindex={-1}
  ondblclick={() => kb.activate(row.id)}
  oncontextmenu={openMenuAtMouse}
  class={[
    "flex scroll-mb-2 cursor-default flex-col gap-2 rounded p-2",
    "hover:bg-hover",
    "data-[kb-cursor]:bg-raised/50",
  ]}
>
  {#if row.state === "initializing"}
    <!-- Initializing: a skeleton at the exact row dimensions (no layout shift)
         while the torrent resolves its name + files. -->
    <div class="flex items-center gap-3">
      <div class="flex min-w-0 flex-1 items-center gap-2">
        <div class="size-8 shrink-0 animate-pulse rounded bg-muted/15"></div>
        <div class="flex min-w-0 flex-1 flex-col">
          <div class="text-sm leading-5">
            <span
              class="inline-block h-3.5 w-40 max-w-full animate-pulse rounded bg-muted/25 align-middle"
            ></span>
          </div>
          <div class="text-xs leading-4">
            <span class="inline-block h-2.5 w-24 animate-pulse rounded bg-muted/20 align-middle"
            ></span>
          </div>
        </div>
      </div>
      <div class="flex shrink-0 items-center gap-0.5">
        <div class="size-7 animate-pulse rounded bg-muted/15"></div>
        <div class="size-7 animate-pulse rounded bg-muted/15"></div>
        <div class="size-7 animate-pulse rounded bg-muted/15"></div>
      </div>
    </div>
  {:else}
    <div class="flex items-center gap-3">
      <div class="flex min-w-0 flex-1 items-center gap-2">
        <!-- Band drags may start on the mark square and inherit its additive
             semantics (see drag-select). -->
        <button
          type="button"
          tabindex={-1}
          data-mark-button
          onclick={(e) => {
            e.stopPropagation();
            onmark();
          }}
          ondblclick={(e) => e.stopPropagation()}
          class={[
            "grid size-8 shrink-0 place-items-center rounded",
            marked ? "bg-marked/20 hover:bg-marked/30" : "bg-muted/10 hover:bg-muted/20",
          ]}
        >
          {#if isContainer}
            <Folder size={18} class={marked ? "text-marked" : "text-muted"} />
          {:else}
            <Film size={18} class={marked ? "text-marked" : "text-muted"} />
          {/if}
        </button>

        <div class="flex min-w-0 flex-col">
          <div class="flex min-w-0 items-baseline gap-1.5">
            <span class="min-w-0 truncate text-sm text-fg">{row.name}</span>
            {#if row.pathHint}
              <span class="text-subtle shrink-0 text-xs">{row.pathHint}</span>
            {/if}
          </div>
          <div class="flex gap-3 truncate text-xs text-muted">
            {#each stats as stat}
              <span class={stat.class}>{stat.text}</span>
            {/each}
          </div>
        </div>
      </div>

      <!-- Always three buttons: Play (success), Persist toggle (info), Overflow.
         Pause/resume, share, delete live in the overflow menu. -->
      <div
        class="flex shrink-0 items-center gap-0.5"
        ondblclick={(e) => e.stopPropagation()}
        role="presentation"
      >
        <button
          type="button"
          tabindex={-1}
          onclick={playRow}
          class={[
            "grid size-7 place-items-center rounded text-success transition-colors duration-150 hover:bg-success/20",
            flashed === "play" && "bg-success/20",
          ]}
        >
          <Play size={14} strokeWidth={2} />
        </button>
        <button
          type="button"
          tabindex={-1}
          onclick={togglePersist}
          class={[
            "grid size-7 place-items-center rounded text-info transition-colors duration-150 hover:bg-info/20",
            flashed === "save" && "bg-info/20",
          ]}
        >
          {#if persisted}
            <BookmarkCheck size={14} strokeWidth={2} />
          {:else}
            <Bookmark size={14} strokeWidth={2} />
          {/if}
        </button>
        <button
          bind:this={ellipsisEl}
          type="button"
          tabindex={-1}
          data-menu-trigger
          onclick={toggleMenuAtEllipsis}
          class={[
            "grid size-7 place-items-center rounded transition-colors duration-150 hover:bg-raised/50",
            menuOpen ? "bg-raised/50 text-fg" : "text-muted hover:text-fg",
          ]}
        >
          <EllipsisVertical size={14} strokeWidth={2} />
        </button>
      </div>
    </div>
  {/if}

  <div class="h-1 w-full rounded-full bg-muted/20">
    <div
      class={["h-full rounded-full", FILL[row.state]]}
      style="width: {Math.max(0, Math.min(1, row.progress)) * 100}%"
    ></div>
  </div>
</div>

{#if menuAnchor}
  <Menu
    bind:open={menuOpen}
    anchor={menuAnchor}
    bounds={menuBounds}
    trigger={ellipsisEl}
    items={menuItems}
  />
{/if}
