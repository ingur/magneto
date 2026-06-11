// Runtime UI zoom (ctrl+= / ctrl+- / ctrl+0). Tauri's webview setZoom scales the
// whole view uniformly (text, spacing, AND the px-sized lucide icons), which a
// CSS root-font scale can't. A local pref persisted to localStorage; gated on
// isTauri so browser dev no-ops. Mirrors the theme module's shape.

import { getCurrentWebview } from "@tauri-apps/api/webview";

import { isTauri } from "./daemon/tauri";

const STORAGE_KEY = "magneto:zoom";
const MIN = 0.5;
const MAX = 2;
const STEP = 0.1;

let level = readLevel();

function readLevel(): number {
  try {
    const v = Number(localStorage.getItem(STORAGE_KEY));
    return Number.isFinite(v) && v >= MIN && v <= MAX ? v : 1;
  } catch {
    // localStorage blocked (private mode); reads throw at import time, which
    // would kill bootstrap; default instead.
    return 1;
  }
}

function apply(): void {
  if (isTauri()) void getCurrentWebview().setZoom(level);
}

function set(next: number): void {
  level = Math.min(MAX, Math.max(MIN, Math.round(next * 100) / 100));
  try {
    localStorage.setItem(STORAGE_KEY, String(level));
  } catch {
    // localStorage blocked (private mode); zoom stays in-memory for the session.
  }
  apply();
}

export const zoom = {
  in: () => set(level + STEP),
  out: () => set(level - STEP),
  reset: () => set(1),
};

// Apply the saved level on launch (before mount, alongside the theme).
export function initZoom(): void {
  apply();
}
