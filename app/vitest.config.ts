import { fileURLToPath } from "node:url";

import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";

// Vitest needs the Svelte plugin to compile the `$state` runes in
// `*.svelte.ts` engine modules (e.g. lib/kb/kb.svelte.ts), and jsdom for the
// DOM the kb engine drives (compareDocumentPosition, data-* attributes). The
// `@` alias mirrors vite.config.ts so `*.svelte.ts` modules tested here can
// value-import `@/...` (e.g. nav → @/daemon/client.svelte).
export default defineConfig({
  plugins: [svelte({ hot: false })],
  resolve: {
    conditions: ["browser"],
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
  },
});
