<script lang="ts">
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Info from "@lucide/svelte/icons/info";
  import Magnet from "@lucide/svelte/icons/magnet";
  import Minus from "@lucide/svelte/icons/minus";
  import Settings from "@lucide/svelte/icons/settings";
  import X from "@lucide/svelte/icons/x";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { settingsOpen } from "@/settings/open.svelte";
  import { overlays } from "@/lib/surface/overlays.svelte";
  import { menus } from "@/lib/popover/menus.svelte";
  import Menu, { type MenuItem } from "@/lib/ui/controls/Menu.svelte";
  import { toast } from "@/lib/feedback/toasts/toasts.svelte";
  import { daemon } from "@/daemon/client.svelte";
  import { openConfigDir } from "@/daemon/tauri";
  import { showAbout, openRepository } from "@/help/about";
  import { sortOpen } from "./sort-open.svelte";
  import { nav } from "@/torrents/nav.svelte";
  import SortButton from "./SortButton.svelte";

  // Disabled affordance: dim text-disabled when not actionable, default
  // hover:text-fg otherwise. Modal surfaces lock background app chrome;
  // Settings is the exception when it is the only surface, so the same cog
  // can close what it opened.

  const appChromeDisabled = $derived(overlays.any);
  const settingsToggleDisabled = $derived(
    overlays.any && !(settingsOpen.value && overlays.count === 1),
  );
  const backDisabled = $derived(appChromeDisabled || !nav.canBack);
  const forwardDisabled = $derived(appChromeDisabled || !nav.canForward);

  // Magnet color reflects daemon connectivity: green connected, amber
  // connecting/reconnecting, red disconnected. Disabled overlay state still
  // wins (greys everything).
  const magnetColor = $derived.by(() => {
    if (appChromeDisabled) return "text-disabled";
    if (daemon.status === "connected") return "text-success hover:bg-success/20";
    if (daemon.status === "disconnected") return "text-danger hover:bg-danger/20";
    return "text-warning hover:bg-warning/20"; // connecting | reconnecting
  });

  // Right-click menus on chrome: gear → config folder, wordmark → About/repo.
  // Two-step toggle and disabled conditions mirror the row menus.
  let gearMenuOpen = $state(false);
  let gearMenuAnchor = $state<{ kind: "point"; x: number; y: number } | null>(null);
  let wordmarkMenuOpen = $state(false);
  let wordmarkMenuAnchor = $state<{ kind: "point"; x: number; y: number } | null>(null);

  const gearMenuItems: MenuItem[] = [
    {
      id: "open-config",
      label: "Open config folder",
      icon: FolderOpen,
      run: () => void openConfigFolder(),
    },
  ];
  const wordmarkMenuItems: MenuItem[] = [
    { id: "about", label: "About", icon: Info, run: () => void showAbout() },
    {
      id: "repository",
      label: "Open repository",
      icon: ExternalLink,
      run: () => void openRepository(),
    },
  ];

  async function openConfigFolder() {
    try {
      await openConfigDir();
    } catch (e) {
      toast.error(`Couldn't open the config folder: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  function openGearMenu(e: MouseEvent) {
    e.preventDefault();
    if (menus.isAnyOpen) {
      menus.closeAny();
      return;
    }
    if (settingsToggleDisabled) return;
    gearMenuAnchor = { kind: "point", x: e.clientX, y: e.clientY };
    gearMenuOpen = true;
  }

  function openWordmarkMenu(e: MouseEvent) {
    e.preventDefault();
    if (menus.isAnyOpen) {
      menus.closeAny();
      return;
    }
    if (appChromeDisabled) return;
    wordmarkMenuAnchor = { kind: "point", x: e.clientX, y: e.clientY };
    wordmarkMenuOpen = true;
  }

  async function minimize() {
    try {
      await getCurrentWindow().minimize();
    } catch (e) {
      console.warn(e);
    }
  }
  async function close() {
    try {
      await getCurrentWindow().close();
    } catch (e) {
      console.warn(e);
    }
  }

  // Easter egg: clicking the magnet rotates it. Single click is a 90°
  // quarter-turn; a fast follow-up click (within 300ms) snaps to the
  // next full-turn boundary from where the burst started, so a fast
  // double-click lands at exactly +360° (one full turn), not 90° + 360°.
  // Each additional fast click adds another full turn on top, so rapid
  // clicks compound into a brief continuous spin: new targets arrive
  // before the CSS transition settles and the browser smoothly
  // interpolates through them. After the last click, the easing
  // finishes and the icon comes to rest. No rAF, no momentum
  // bookkeeping; the chained transitions do all the work.
  let magnetRotation = $state(0);
  let lastMagnetClick = 0;
  let burstStartRotation = 0;
  let burstClicks = 0;
  function spinMagnet() {
    const now = Date.now();
    const fast = now - lastMagnetClick < 300;
    lastMagnetClick = now;
    if (fast) {
      burstClicks++;
      magnetRotation = burstStartRotation + burstClicks * 360;
    } else {
      burstStartRotation = magnetRotation;
      burstClicks = 0;
      magnetRotation += 90;
    }
  }
</script>

<header
  data-tauri-drag-region
  class="text-muted relative flex h-10 select-none items-center justify-between px-1.5"
>
  <div class="flex items-center gap-0.5">
    <button
      type="button"
      tabindex={-1}
      data-menu-trigger-area
      disabled={settingsToggleDisabled}
      onclick={() => (settingsOpen.value = !settingsOpen.value)}
      oncontextmenu={openGearMenu}
      class={[
        "grid size-7 place-items-center",
        settingsToggleDisabled ? "text-disabled" : "hover:text-fg",
      ]}
    >
      <Settings size={14} strokeWidth={2} />
    </button>
    <SortButton bind:open={sortOpen.value} disabled={appChromeDisabled} />
    <button
      type="button"
      tabindex={-1}
      disabled={backDisabled}
      onclick={() => nav.back()}
      class={["grid size-7 place-items-center", backDisabled ? "text-disabled" : "hover:text-fg"]}
    >
      <ChevronLeft size={14} strokeWidth={2} />
    </button>
    <button
      type="button"
      tabindex={-1}
      disabled={forwardDisabled}
      onclick={() => nav.forward()}
      class={[
        "grid size-7 place-items-center",
        forwardDisabled ? "text-disabled" : "hover:text-fg",
      ]}
    >
      <ChevronRight size={14} strokeWidth={2} />
    </button>
  </div>

  <!-- "magneto" text is the absolute-centered anchor; the magnet logo docks
       to its left so the label stays truly centered on the bar. Magnet and
       label are independent buttons; the label is the home button. -->
  <div class="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2">
    <button
      type="button"
      tabindex={-1}
      disabled={appChromeDisabled}
      onclick={spinMagnet}
      class={[
        "absolute top-[calc(50%+1px)] right-full mr-1.5 grid size-6 -translate-y-1/2 place-items-center rounded",
        magnetColor,
      ]}
    >
      <!-- Span wraps Magnet so the rotation transform animates the icon
           only; the button's hitbox and grid centering stay fixed.
           will-change isolates the spin to its own compositing layer so the
           animation doesn't re-rasterize the centered wordmark into a blur. -->
      <span
        class="inline-block will-change-transform"
        style="transform: rotate({magnetRotation}deg); transition: transform 400ms ease-out;"
      >
        <Magnet size={14} />
      </span>
    </button>
    <button
      type="button"
      tabindex={-1}
      data-menu-trigger-area
      disabled={appChromeDisabled}
      onclick={() => nav.home()}
      oncontextmenu={openWordmarkMenu}
      class={["text-sm", appChromeDisabled ? "text-disabled" : "hover:text-fg"]}
    >
      magneto
    </button>
  </div>

  <div class="flex items-center gap-0.5">
    <button
      type="button"
      tabindex={-1}
      onclick={minimize}
      class="hover:text-fg grid size-7 place-items-center"
    >
      <Minus size={14} strokeWidth={2} />
    </button>
    <button
      type="button"
      tabindex={-1}
      onclick={close}
      class="hover:text-danger grid size-7 place-items-center"
    >
      <X size={14} strokeWidth={2} />
    </button>
  </div>
</header>

{#if gearMenuAnchor}
  <Menu bind:open={gearMenuOpen} anchor={gearMenuAnchor} items={gearMenuItems} />
{/if}
{#if wordmarkMenuAnchor}
  <Menu bind:open={wordmarkMenuOpen} anchor={wordmarkMenuAnchor} items={wordmarkMenuItems} />
{/if}
