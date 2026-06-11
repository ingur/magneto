// Bridge to the Tauri host. The host owns daemon lifecycle (spawn the daemon,
// discover its port) and the platform APIs (read a dropped file, reveal in the
// file manager, OS notifications, native pickers, external links, app version).
// Every call is a no-op / falls back outside the Tauri host, so browser dev
// degrades gracefully.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

import type { PathKind } from "./protocol";

// Control port the daemon binds by default (config.rs). Used only for browser
// dev outside the Tauri host, where a daemon must be started manually.
const DEFAULT_CONTROL_PORT = 61481;

export function isTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

/** Ensure the daemon is running, and return its control port and the
 *  control-WS token to authenticate the connection. (Stopping the daemon is
 *  the host's job: it happens on app exit, Rust-side.) In browser dev the
 *  token comes from VITE_MAGNETO_CONTROL_TOKEN (set it from the daemon's
 *  daemon.json), and is null if unset; the connection then fails the
 *  daemon's token check. */
export async function ensureDaemon(): Promise<{ port: number; token: string | null }> {
  if (!isTauri()) {
    return {
      port: DEFAULT_CONTROL_PORT,
      token: import.meta.env.VITE_MAGNETO_CONTROL_TOKEN ?? null,
    };
  }
  return await invoke<{ port: number; token: string | null }>("ensure_daemon");
}

/** Drain the host's queue of OS-handed add sources (magnet links / .torrent
 *  paths routed to the app as the system handler). Empty outside Tauri. */
export async function takePendingSources(): Promise<string[]> {
  if (!isTauri()) return [];
  return invoke<string[]>("take_pending_sources");
}

/** Subscribe to the host's ping that new OS-handed sources are queued.
 *  Returns an unsubscriber (the async registration is absorbed here so
 *  consumers get the same shape as daemon.onEvent); inert outside Tauri. */
export function onSourcesReady(handler: () => void): () => void {
  if (!isTauri()) return () => {};
  const ready = listen("sources-ready", handler);
  return () => void ready.then((off) => off());
}

/** Quit the app for real (window close only hides to tray). The host stops
 *  the daemon on the way out. No-op outside Tauri. */
export async function quitApp(): Promise<void> {
  if (!isTauri()) return;
  await invoke("quit_app");
}

/** Read a dropped `.torrent` path and return its base64 add source. */
export async function readTorrentFile(path: string): Promise<string> {
  return invoke<string>("read_torrent_file", { path });
}

/** Reveal a row's downloaded data in the OS file manager: a file is selected
 *  in its folder; a folder is opened. */
export async function revealPath(path: string, kind: PathKind): Promise<void> {
  if (!isTauri()) return;
  if (kind === "file") await revealItemInDir(path);
  else await openPath(path);
}

/** Pick a folder (Settings: downloads directory). Null on cancel / outside Tauri. */
export async function pickFolder(): Promise<string | null> {
  if (!isTauri()) return null;
  const picked = await openDialog({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

/** Pick a single file (Settings: player / fallback executable). */
export async function pickFile(): Promise<string | null> {
  if (!isTauri()) return null;
  const picked = await openDialog({ directory: false, multiple: false });
  return typeof picked === "string" ? picked : null;
}

/** Pick `.torrent` files to add (list context menu). Empty on cancel / outside Tauri. */
export async function pickTorrentFiles(): Promise<string[]> {
  if (!isTauri()) return [];
  const picked = await openDialog({
    multiple: true,
    filters: [{ name: "Torrent files", extensions: ["torrent"] }],
  });
  return Array.isArray(picked) ? picked : picked === null ? [] : [picked];
}

/** Open the config directory (config.toml, theme.toml) in the OS file manager. */
export async function openConfigDir(): Promise<void> {
  if (!isTauri()) return;
  await openPath(await invoke<string>("get_config_dir"));
}

/** Read clipboard text via the plugin; WebKitGTK lets the web clipboard
 *  API write but denies programmatic reads. */
export async function readClipboardText(): Promise<string> {
  if (isTauri()) return readText();
  return navigator.clipboard.readText();
}

/** Whether the app is registered to launch at login. False outside Tauri. */
export async function getAutostart(): Promise<boolean> {
  return isTauri() ? isAutostartEnabled() : false;
}

/** Enable or disable launch at login. No-op outside Tauri. */
export async function setAutostart(on: boolean): Promise<void> {
  if (!isTauri()) return;
  if (on) await enableAutostart();
  else await disableAutostart();
}

/** Open an external URL in the OS default browser. No-op outside Tauri. */
export async function openExternal(url: string): Promise<void> {
  if (isTauri()) await openUrl(url);
}

/** The app version (from the Tauri bundle). "dev" outside Tauri. */
export async function appVersion(): Promise<string> {
  return isTauri() ? getVersion() : "dev";
}

let notifyAllowed = false;

/** Request notification permission once, at launch. */
export async function initNotifications(): Promise<void> {
  if (!isTauri()) return;
  notifyAllowed = (await isPermissionGranted()) || (await requestPermission()) === "granted";
}

/** Fire an OS notification (no-op until permission is granted / outside Tauri). */
export function notify(title: string, body: string): void {
  if (notifyAllowed) sendNotification({ title, body });
}
