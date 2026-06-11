# magneto (CLI)

A thin command-line client for the magneto daemon. For now it covers quick
adds and daemon control; it shares `magneto-core` with the desktop app and
never links the torrent engine.

```bash
# add sources: magnets, http(s) torrent links, local .torrent files
magneto "magnet:?xt=..." ./some.torrent

# daemon control
magneto daemon start
magneto daemon status
magneto daemon restart
magneto daemon stop
```

Adding a source starts the daemon first if none is running.
