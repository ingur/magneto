/// <reference types="svelte" />
/// <reference types="vite/client" />

interface ImportMetaEnv {
  // Browser-dev only: control-WS token for a manually-started daemon. The Tauri
  // host supplies the token at runtime instead (see daemon/tauri.ts).
  readonly VITE_MAGNETO_CONTROL_TOKEN?: string;
}
