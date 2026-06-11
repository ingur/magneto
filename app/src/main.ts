import { mount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";

import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/600.css";
import "@fontsource/jetbrains-mono/700.css";
import "./app.css";

import App from "./App.svelte";
import { isTauri } from "./daemon/tauri";
import { initTheme } from "./theme.svelte";
import { initZoom } from "./zoom";

// Resolve the active palette + zoom before mount; the theme.css bg/fg fallback
// covers any pre-JS frame. Reveal in finally: a failed boot must still show
// the window, a hidden one can't surface any error.
async function bootstrap() {
  try {
    await initTheme();
    initZoom();
    mount(App, { target: document.getElementById("app")! });
  } finally {
    await revealWindow();
  }
}

// The window starts hidden (tauri.conf.json `visible: false`) so the webview's
// white default never flashes; themed CSS is in place before this runs. Do NOT
// gate the reveal on rAF: WebKitGTK produces no frames for a hidden window.
async function revealWindow() {
  if (!isTauri()) return;
  try {
    const win = getCurrentWindow();
    await win.show();
    await win.setFocus();
  } catch (e) {
    console.warn(e);
  }
}

bootstrap();
