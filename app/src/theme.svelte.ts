// Runtime theme application. The Tauri Rust layer owns theme.toml (generate,
// validate, hot-reload) and ships both resolved palettes; this module picks the
// active variant (System/Dark/Light, a local pref) and writes the leaf
// --t-<role> CSS variables onto <html>. theme.css maps --color-<role> to those,
// so utilities update with no Tailwind rebuild. See app/src-tauri/src/theme.rs.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { isTauri } from "./daemon/tauri";
import { toast } from "./lib/feedback/toasts/toasts.svelte";

type Palette = Record<string, string>;
type Theme = { dark: Palette; light: Palette };
export type ThemeMode = "system" | "dark" | "light";

const STORAGE_KEY = "magneto:theme-mode";

// theme.css :root holds a valid bg/fg fallback, so empty palettes pre-load are safe.
let palettes: Theme = { dark: {}, light: {} };
let mode = $state<ThemeMode>(readMode());
const prefersDark = window.matchMedia("(prefers-color-scheme: dark)");

function readMode(): ThemeMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === "dark" || v === "light" || v === "system" ? v : "system";
  } catch {
    // localStorage blocked (private mode); reads throw at import time, which
    // would kill bootstrap; default instead.
    return "system";
  }
}

function resolvedDark(): boolean {
  return mode === "dark" || (mode === "system" && prefersDark.matches);
}

function apply(): void {
  const palette = resolvedDark() ? palettes.dark : palettes.light;
  const root = document.documentElement;
  for (const role in palette) root.style.setProperty(`--t-${role}`, palette[role]);
  root.style.colorScheme = resolvedDark() ? "dark" : "light";
}

/** Active theme mode. Reading is reactive; assigning persists + re-applies. */
export const themeMode = {
  get value(): ThemeMode {
    return mode;
  },
  set value(next: ThemeMode) {
    mode = next;
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // localStorage unavailable (private mode); mode stays in-memory.
    }
    apply();
  },
};

export async function initTheme(): Promise<void> {
  if (!isTauri()) return; // browser dev: the theme.css bg/fg fallback applies
  try {
    prefersDark.addEventListener("change", () => {
      if (mode === "system") apply();
    });
    await listen<Theme>("theme://changed", (e) => {
      palettes = e.payload;
      apply();
    });
    await listen<string>("theme://error", (e) => {
      toast.error(`Couldn't load the theme: ${e.payload}`);
    });
    palettes = await invoke<Theme>("get_theme"); // pull AFTER listeners are armed
    apply();
  } catch (e) {
    console.error("theme init failed; using CSS fallback", e);
  }
}
