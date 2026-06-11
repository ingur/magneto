# magneto app

The desktop app: a Tauri 2 shell hosting a Svelte 5 frontend that talks to
the magneto daemon over its WebSocket control protocol. It ships the daemon
as a bundled sidecar, supervises its lifecycle, and registers as the OS
handler for magnet links and `.torrent` files.

## Stack

- **Svelte 5** (runes) + **Vite** + **Tailwind CSS 4**, plain SPA via
  `mount()`, no SvelteKit.
- **Tauri 2** (`src-tauri/`, crate `magneto-app`).
- JetBrains Mono self-hosted via `@fontsource` (offline, no font CDN).

## Daemon model

The app links `magneto-core` only; the torrent engine never enters this
build (`cargo tree -p magneto-app` shows no librqbit/axum). The daemon ships
as a Tauri sidecar (`bundle.externalBin`) and is found at runtime as a
sibling of the app executable, in bundles and dev builds both.

On launch the frontend invokes `ensure_daemon`; the Rust side connects to a
running daemon or spawns the sidecar, and returns the control port plus
auth token. The frontend then opens `ws://127.0.0.1:<port>/ws` and
reconnects with backoff on every drop. Quitting the app stops the daemon;
closing the window only hides to tray and keeps it running.

`scripts/stage-daemon.mjs` builds the daemon and stages it at
`src-tauri/binaries/magneto-daemon-<target-triple>`. It runs automatically
via the `beforeDevCommand` / `beforeBuildCommand` hooks.

## OS handler intake

Magnet clicks and `.torrent` opens arrive as argv: a cold start parses its
own, a second launch is forwarded by the single-instance plugin. Sources
wait in a host-side queue until the daemon socket is connected, then run
the normal add flow. macOS delivers opens as app events instead.

## Theme

Semantic role tokens end to end: the Rust side owns a user-editable
`theme.toml` in the config dir (generated with defaults, validated,
hot-reloaded), and the frontend picks the variant (System/Dark/Light) and
writes `--t-<role>` CSS variables. Utilities update live, no rebuild.

## Commands

Run from `app/` (the devenv provides pnpm/Node/Rust + GTK/WebKit):

| Command | Description |
|---|---|
| `pnpm tauri dev` | stage the daemon, start Vite, launch the app |
| `pnpm tauri build` | release bundles (stages a release daemon) |
| `pnpm test` | vitest suite |
| `pnpm check` | Svelte/TypeScript typecheck |
| `pnpm icons` | regenerate `src-tauri/icons/*` from `app-icon.svg` |

Outside Tauri (`pnpm dev` in a browser) the app falls back to the default
control port and expects a daemon started manually (`magneto daemon start`).
