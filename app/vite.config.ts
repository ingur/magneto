import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

const host = process.env.TAURI_DEV_HOST;

// https://v2.tauri.app/reference/config/
export default defineConfig({
  plugins: [svelte(), tailwindcss()],

  // `@` -> src, so modules import as stable absolute paths (`@/lib/kb`, …).
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },

  // Prevent Vite from clearing Rust errors during `tauri dev`.
  clearScreen: false,
  server: {
    // Tauri expects a fixed port and fails if it is unavailable.
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
