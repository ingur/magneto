# magneto-daemon

The daemon does all the torrent work: it runs the librqbit session, serves
a WebSocket control plane for clients, streams files over HTTP for video
players, and serves DLNA on the local network. Running `magneto-daemon`
starts it in the foreground; the desktop app ships it as a sidecar and
manages it automatically. Logs go to `{data_dir}/magneto.log`.

| Module | Owns |
|---|---|
| `bootstrap` | startup: single-instance lock, config fallback, reconcile |
| `control` | WebSocket listener, token auth, client connections |
| `commands` | request dispatch and the torrent operations |
| `session` | the librqbit engine handle |
| `stream` | HTTP range streaming to players |
| `lan` + `upnp` | DLNA serving and discovery |
| `stats` | per-second state deltas broadcast to clients |
| `session_store` | engine session directory: repair and fastresume |
| `check` | waits for a torrent's file check to finish |
| `removal` | the one place a torrent or its bytes are deleted |
| `metadata` | per-file flags and media classification |
