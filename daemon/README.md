# magneto-daemon

The daemon does all the torrent work: it runs the librqbit session, serves
a WebSocket control plane for clients, streams files over HTTP for video
players, and serves DLNA on the local network. Running `magneto-daemon`
starts it in the foreground; the desktop app ships it as a sidecar and
manages it automatically. Logs go to `{data_dir}/magneto.log`.

| Module | Owns |
|---|---|
| `bootstrap` | startup: single-instance lock, config fallback, cleanup |
| `control` | WebSocket listener, token auth, client connections |
| `commands` | request dispatch and the torrent operations |
| `session` | the librqbit engine handle |
| `stream` | HTTP range streaming to players |
| `lan` + `upnp` | DLNA serving and discovery |
| `stats` | per-second state deltas broadcast to clients |
| `fastresume` | engine session persistence |
| `watcher` | magnet metadata resolution timeouts |
| `metadata` | per-file flags and media classification |
