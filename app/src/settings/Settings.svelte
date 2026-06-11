<script lang="ts">
  import { onDestroy, onMount } from "svelte";

  import { daemon } from "@/daemon/client.svelte";
  import { getAutostart, pickFile, pickFolder, setAutostart } from "@/daemon/tauri";
  import { themeMode, type ThemeMode } from "@/theme.svelte";
  import { notifications } from "@/notifications.svelte";
  import { prompt } from "@/lib/feedback/prompts/prompts.svelte";
  import { toast } from "@/lib/feedback/toasts/toasts.svelte";
  import type { LayerHandle } from "@/lib/kb/kb.svelte";
  import KbLayer from "@/lib/kb/KbLayer.svelte";
  import { useScrollFollowCursor } from "@/lib/kb/use-scroll-follow-cursor.svelte";
  import Overlay from "@/lib/surface/Overlay.svelte";
  import OverlayTitle from "@/lib/surface/OverlayTitle.svelte";
  import Button from "@/lib/ui/Button.svelte";
  import ButtonRow from "@/lib/ui/ButtonRow.svelte";
  import Checkbox from "@/lib/ui/controls/Checkbox.svelte";
  import Dropdown from "@/lib/ui/controls/Dropdown.svelte";
  import FilePicker from "@/lib/ui/controls/FilePicker.svelte";
  import TextInput from "@/lib/ui/controls/TextInput.svelte";

  import { closeSettings, onSettingsCloseRequest } from "./open.svelte";
  import { settings } from "./store.svelte";
  import { settingsBindings, type SettingsButtonId } from "./bindings";
  import Setting from "./Setting.svelte";
  import SettingSection from "./SettingSection.svelte";

  let handle = $state<LayerHandle | undefined>();
  let lastButton = $state<SettingsButtonId>("save");

  // Theme is a local pref (not daemon config): a string view over the theme
  // store, bound directly so it applies on select; excluded from working/dirty.
  const themeOptions = [
    { value: "system", label: "System" },
    { value: "dark", label: "Dark" },
    { value: "light", label: "Light" },
  ];
  const theme = {
    get value(): string {
      return themeMode.value;
    },
    set value(v: string) {
      themeMode.value = v as ThemeMode;
    },
  };

  // Autostart is an OS-level local pref: read on open, written live on toggle,
  // never part of working/dirty/save. `appliedAutostart` tracks the last value
  // pushed to the OS so the initial load assignment doesn't fire a redundant set.
  let autostart = $state(false);
  let appliedAutostart = false;
  onMount(async () => {
    appliedAutostart = await getAutostart();
    autostart = appliedAutostart;
  });
  $effect(() => {
    const next = autostart;
    if (next === appliedAutostart) return;
    const prev = appliedAutostart;
    appliedAutostart = next;
    setAutostart(next).catch(() => {
      toast.error("Couldn't change launch at login");
      // OS rejected: revert so the checkbox reflects reality. Resetting both
      // makes the effect's re-run a no-op (next === appliedAutostart).
      appliedAutostart = prev;
      autostart = prev;
    });
  });

  // Seed synchronously so the rows (and their kbItems) exist on first paint;
  // the first registered control claims the cursor. The effect below covers the
  // async-daemon-arrival case (config not yet present at mount).
  settings.begin();
  $effect(() => {
    if (!settings.working && daemon.config) settings.begin();
  });

  useScrollFollowCursor(() => handle);

  // Route every close through the dirty-check: esc/comma/backdrop call
  // requestClose directly; the TopBar cog flips the flag, whose setter delegates
  // here. settings.end() on unmount clears the edit session on every close path.
  const offCloseRequest = onSettingsCloseRequest(requestClose);
  onDestroy(() => {
    offCloseRequest();
    settings.end();
  });

  const bindings = settingsBindings({
    getLastButton: () => lastButton,
    setLastButton: (id) => (lastButton = id),
    requestClose,
    save: onSave,
  });

  async function requestClose() {
    if (settings.dirty) {
      const ok = await prompt({
        type: "confirm",
        title: "Discard changes?",
        description: "Closing discards your unsaved settings.",
        confirmLabel: "Discard",
        cancelLabel: "Keep editing",
        tint: "danger",
      });
      if (!ok) return;
    }
    closeSettings();
  }

  function onReset() {
    lastButton = "reset";
    settings.reset();
    toast.info("Settings reset");
  }
  function onSave() {
    lastButton = "save";
    void settings.save();
  }

  // The native pickers can reject (cancelled is handled as null; a real failure
  // throws, e.g. a misconfigured desktop portal). Toast instead of swallowing.
  async function browse(pick: () => Promise<string | null>, apply: (path: string) => void) {
    try {
      const picked = await pick();
      if (picked && settings.working) apply(picked);
    } catch (e) {
      toast.error(`Couldn't open the file picker: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  const browseDir = () => browse(pickFolder, (p) => (settings.working!.downloadsDir = p));
  const browsePlayer = () => browse(pickFile, (p) => (settings.working!.playerCommand = p));
  const browseFallback = () => browse(pickFile, (p) => (settings.working!.fallbackApp = p));
</script>

<KbLayer name="settings" {bindings} bind:handle>
  <Overlay onClose={requestClose}>
    {#snippet title()}
      <OverlayTitle>Settings</OverlayTitle>
    {/snippet}

    {#if settings.working}
      {@const w = settings.working}
      <div class="flex flex-col gap-6">
        <SettingSection title="General">
          <Setting label="Theme">
            <Dropdown
              bind:value={theme.value}
              options={themeOptions}
              kbItem={{ id: "theme", group: "settings" }}
            />
          </Setting>
          <Setting label="Desktop notifications">
            <Checkbox
              bind:checked={notifications.enabled}
              kbItem={{ id: "notifications", group: "settings" }}
            />
          </Setting>
          <Setting label="Launch at login">
            <Checkbox bind:checked={autostart} kbItem={{ id: "autostart", group: "settings" }} />
          </Setting>
        </SettingSection>

        <SettingSection title="Downloads">
          <Setting label="Directory">
            <FilePicker
              bind:value={w.downloadsDir}
              onBrowse={browseDir}
              kbItem={{ id: "downloads-dir", group: "settings" }}
            />
          </Setting>
          <Setting label="Download by default">
            <Checkbox
              bind:checked={w.autoDownload}
              kbItem={{ id: "auto-download", group: "settings" }}
            />
          </Setting>
          <Setting label="Save by default">
            <Checkbox
              bind:checked={w.persistDefault}
              kbItem={{ id: "persist-default", group: "settings" }}
            />
          </Setting>
          <Setting label="Share by default">
            <Checkbox
              bind:checked={w.shareDefault}
              kbItem={{ id: "share-default", group: "settings" }}
            />
          </Setting>
        </SettingSection>

        <SettingSection title="Player">
          <Setting label="Application">
            <FilePicker
              bind:value={w.playerCommand}
              onBrowse={browsePlayer}
              kbItem={{ id: "player-command", group: "settings" }}
            />
          </Setting>
          <Setting label="Arguments">
            <TextInput
              bind:value={w.playerArgs}
              size="lg"
              kbItem={{ id: "player-args", group: "settings" }}
            />
          </Setting>
          <Setting label="Media extensions">
            <TextInput
              bind:value={w.mediaExtensions}
              size="lg"
              kbItem={{ id: "media-extensions", group: "settings" }}
            />
          </Setting>
          <Setting label="Autoplay">
            <Checkbox bind:checked={w.autoplay} kbItem={{ id: "autoplay", group: "settings" }} />
          </Setting>
        </SettingSection>

        <SettingSection title="Fallback">
          <Setting label="Application">
            <FilePicker
              bind:value={w.fallbackApp}
              onBrowse={browseFallback}
              kbItem={{ id: "fallback-app", group: "settings" }}
            />
          </Setting>
          <Setting label="Arguments">
            <TextInput
              bind:value={w.fallbackArgs}
              size="lg"
              kbItem={{ id: "fallback-args", group: "settings" }}
            />
          </Setting>
        </SettingSection>

        <SettingSection title="Network">
          <Setting label="Control port">
            <TextInput
              bind:value={w.controlPort}
              kbItem={{ id: "control-port", group: "settings" }}
            />
          </Setting>
          <Setting label="LAN port">
            <TextInput bind:value={w.lanPort} kbItem={{ id: "lan-port", group: "settings" }} />
          </Setting>
          <Setting label="Device name">
            <TextInput
              bind:value={w.serverName}
              kbItem={{ id: "server-name", group: "settings" }}
            />
          </Setting>
          <Setting label="DLNA server">
            <Checkbox bind:checked={w.upnpEnabled} kbItem={{ id: "upnp", group: "settings" }} />
          </Setting>
        </SettingSection>
      </div>
    {/if}

    {#snippet actions()}
      <ButtonRow>
        <Button kbItem={{ id: "reset", group: "buttons", activate: onReset }} onclick={onReset}>
          Reset
        </Button>
        <Button
          tint="accent"
          kbItem={{ id: "save", group: "buttons", activate: onSave }}
          onclick={onSave}
        >
          Save
        </Button>
      </ButtonRow>
    {/snippet}
  </Overlay>
</KbLayer>
