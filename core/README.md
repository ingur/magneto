# magneto-core

Shared library for everything that talks to or supervises the daemon. Used
by the CLI and the desktop app; intentionally free of engine dependencies.

| Module | Owns |
|---|---|
| `protocol` | the wire contract: commands, events, torrent/file state types |
| `config` | config schema, validation, platform config/data paths |
| `client` | one-shot WebSocket requests against a running daemon |
| `supervisor` | daemon lifecycle: spawn, discover, stop, restart, recovery |
