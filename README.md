<p align="center">
  <h1 align="center">magneto</h1>
</p>

![magneto](https://github.com/user-attachments/assets/70fea78c-507a-46fb-936f-6f4f89b28c12)

<p align="center">
  A media-focused torrent client. Stream magnet links and torrent files
  instantly in your own video player, on any device in your home.
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/ingur/magneto/release.svg?font=jetbrains-mono&mode=dark">
    <img src="https://shieldcn.dev/github/ingur/magneto/release.svg?font=jetbrains-mono&mode=light" alt="Version">
  </picture>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/badge/runs%20on-linux-green.svg?logo=linux&font=jetbrains-mono&mode=dark">
    <img src="https://shieldcn.dev/badge/runs%20on-linux-green.svg?logo=linux&font=jetbrains-mono&mode=light" alt="Runs on Linux">
  </picture>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/badge/runs%20on-windows-blue.svg?logo=windows&font=jetbrains-mono&mode=dark">
    <img src="https://shieldcn.dev/badge/runs%20on-windows-blue.svg?logo=windows&font=jetbrains-mono&mode=light" alt="Runs on Windows">
  </picture>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/badge/runs%20on-macos-black.svg?logo=apple&font=jetbrains-mono&mode=dark">
    <img src="https://shieldcn.dev/badge/runs%20on-macos-black.svg?logo=apple&font=jetbrains-mono&mode=light" alt="Runs on macOS">
  </picture>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/ingur/magneto/license.svg?font=jetbrains-mono&mode=dark">
    <img src="https://shieldcn.dev/github/ingur/magneto/license.svg?font=jetbrains-mono&mode=light" alt="License">
  </picture>
</p>


## Features

- Instantly stream magnet links / torrent files in any video player that
  supports network streams
- Stream to any device on your local network (smart TV, phone, laptop) via DLNA
- Supports batch torrents / playlists
- Supports fallback torrent applications for non-media torrents
- Custom tauri desktop app for Linux / Windows / macOS
- Seamless support for both keyboard driven and mouse driven control
- Customizable themes with a `theme.toml` (hot-reloaded)
- Written in Rust, powered by [librqbit](https://github.com/ikatson/rqbit)

## Install

Download the latest release from the
[releases page](https://github.com/ingur/magneto/releases):
`.exe` (Windows), `.deb` / `.rpm` / `.AppImage` (Linux).
macOS is supported through a local build, see
[building from source](#building-from-source).

> [!NOTE]
> Windows warns about unsigned apps on first run ("More info", then "Run
> anyway") and asks once to allow network access for the daemon. Machines
> with Smart App Control enabled block unsigned apps entirely; it can be
> turned off under Windows Security > App & browser control.

> [!NOTE]
> The AppImage needs FUSE (`fuse3`, preinstalled on most desktop distros).
> Without it, run with `--appimage-extract-and-run`.

## Quick Start

1. Install magneto and set your video player in the settings.
2. Click a magnet link, or drop / paste torrents into the app.
3. Pick what you want to watch and start streaming.

## Why?

- I wanted to be able to click a magnet link and instantly start watching.
- I never quite liked existing solutions, wanted to build something custom.
- I wanted a custom client to sit in between myself and my fallback torrent app.
- I wanted to stream on all my devices on my local network: my smart TV, my
  phone, my laptop, anything.

## How It Works

magneto is a Tauri desktop app with a Svelte UI, built around a small Rust
daemon that does the torrent work with librqbit. The app starts and
supervises the daemon and talks to it over a local WebSocket; the daemon
streams files over HTTP for video players and serves DLNA on the local
network.

| Crate | Role |
|---|---|
| [`core`](core/) | shared library: protocol, config, client, daemon supervision |
| [`daemon`](daemon/) | torrents, streaming, DLNA, persistence |
| [`cli`](cli/) | thin command-line client |
| [`app`](app/) | desktop app |

## Building from source

Requirements: Rust (stable), Node 22+, pnpm, plus the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) on Linux.

```bash
cd app
pnpm install
pnpm tauri dev    # development
pnpm tauri build  # installers for your platform
```

## License

[GPL-3.0](LICENSE)
