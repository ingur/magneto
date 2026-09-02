mod open;
mod sources;
mod theme;
#[cfg(desktop)]
mod tray;

/// What `ensure_daemon` returns: the control port the frontend opens its
/// WebSocket against, and the control-WS token read from the descriptor
/// (None when reaching a daemon that predates token auth, or if the
/// descriptor can't be read).
#[derive(serde::Serialize)]
struct EnsureDaemon {
    port: u16,
    token: Option<String>,
}

/// Ensure the daemon is running. Connects to an already-running daemon if
/// present, otherwise spawns the bundled `magneto-daemon` and waits for it to bind.
#[tauri::command]
async fn ensure_daemon() -> Result<EnsureDaemon, String> {
    let data_dir = magneto_core::config::data_dir().map_err(|e| e.to_string())?;
    let exe = magneto_core::supervisor::daemon_exe()
        .ok_or("could not locate the magneto-daemon binary")?;
    if !exe.exists() {
        return Err(format!("daemon binary not found at {}", exe.display()));
    }
    let (port, _started) = magneto_core::supervisor::ensure_running_with_exe(&data_dir, &exe)
        .await
        .map_err(|e| e.to_string())?;
    let token = magneto_core::supervisor::read_descriptor(&data_dir).and_then(|d| d.control_token);
    Ok(EnsureDaemon { port, token })
}

/// Read a dropped `.torrent` file and return its bytes as base64 (STANDARD),
/// the add source for a local file. `add_torrent` decodes it with the same
/// engine; mirrors the CLI's prepare_source file branch.
///
/// This is a read-any-path primitive exposed to the webview, so it is fenced to
/// the one shape the add flow needs: a `.torrent` extension, a bencode-dict
/// first byte, and a size cap. Hardening, not validation; the daemon parses.
#[tauri::command]
fn read_torrent_file(path: String) -> Result<String, String> {
    use base64::Engine;
    const MAX_TORRENT_BYTES: u64 = 10 * 1024 * 1024;

    let p = std::path::Path::new(&path);
    if !sources::is_torrent_file(p) {
        return Err("not a .torrent file".into());
    }
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if meta.len() > MAX_TORRENT_BYTES {
        return Err("torrent file too large".into());
    }
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    if bytes.first() != Some(&b'd') {
        return Err("not a valid torrent file".into());
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// Absolute path of the config directory (config.toml, theme.toml), created if
/// missing so "Open config folder" always has something to open.
#[tauri::command]
fn get_config_dir() -> Result<String, String> {
    let dir = magneto_core::config::config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().into_owned())
}

/// Quit for real (the keyboard path; window close only hides to tray).
/// Mirrors tray Quit: every exit route funnels through RunEvent::Exit,
/// where the daemon is stopped.
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // WebKitGTK's DMABUF renderer produces a blank window on some
    // NVIDIA/compositor combinations. Opt out unless the user has set a value
    // themselves. Delete this block once upstream WebKitGTK no longer needs it.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // Safety: runs before any other thread exists.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    // Managed up front so no plugin callback can race a queue() before the
    // state exists.
    let mut builder = tauri::Builder::default().manage(sources::Pending::default());

    // single-instance must be the FIRST plugin registered. A second launch
    // (including the OS invoking the magnet/.torrent handler) is intercepted
    // and routed here: its argv is parsed for add sources and the existing
    // window is shown instead of spawning a duplicate.
    #[cfg(desktop)]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let sources = sources::parse_args(argv.into_iter().skip(1))
                .into_iter()
                .map(|s| sources::absolutize(s, &cwd))
                .collect();
            sources::queue(app, sources);
            tray::show_window(app);
        }));
    }

    let app = builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            ensure_daemon,
            theme::get_theme,
            read_torrent_file,
            get_config_dir,
            open::open_path,
            open::open_url,
            quit_app,
            sources::take_pending_sources,
            sources::requeue_sources
        ])
        .setup(|app| {
            theme::init(app.handle());
            #[cfg(desktop)]
            tray::init(app.handle());

            // Register this executable as the magnet handler. Linux: dev
            // builds and real AppImage runs only. The AppImage runtime sets
            // $APPIMAGE and the plugin points the written handler at it, so
            // registration survives the transient mount (and heals a moved
            // AppImage). Without $APPIMAGE (deb/rpm/nix, or a repackaged
            // AppImage running from an extraction) current_exe may not be
            // launchable from outside this process, and the packaged desktop
            // entry already covers the schemes, so registering would only
            // hijack mimeapps.list with a broken handler. Windows: debug
            // builds only; the installer registers release builds. Off the
            // startup path: it shells out to xdg-mime/update-desktop-database,
            // and no part of THIS launch consumes the result.
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            {
                #[cfg(target_os = "linux")]
                let register =
                    cfg!(debug_assertions) || std::env::var_os("APPIMAGE").is_some();
                #[cfg(windows)]
                let register = true;
                if register {
                    use tauri_plugin_deep_link::DeepLinkExt;
                    let handle = app.handle().clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if let Err(e) = handle.deep_link().register_all() {
                            eprintln!("deep-link registration failed: {e}");
                        }
                    });
                }
            }

            // Cold start as the OS handler (Windows/Linux): the magnet URI or
            // .torrent path arrives in our own argv. args_os: a non-Unicode
            // argument must be skipped, not panic the launch (Linux filenames
            // are raw bytes). Relative .torrent paths resolve against the
            // launch cwd now, like the second-launch path, so a queued source
            // never depends on the cwd at read time. macOS never uses argv,
            // see RunEvent::Opened.
            let cwd = std::env::current_dir().unwrap_or_default();
            let sources = sources::parse_args(
                std::env::args_os().skip(1).filter_map(|a| a.into_string().ok()),
            )
            .into_iter()
            .map(|s| sources::absolutize(s, &cwd.to_string_lossy()))
            .collect();
            sources::queue(app.handle(), sources);

            // Persist/restore window size + position across launches. VISIBLE
            // is excluded: the window starts hidden (tauri.conf.json) and the
            // frontend shows it after the first themed paint. Restoring a
            // saved "visible" here would flash the unpainted webview.
            #[cfg(desktop)]
            {
                use tauri_plugin_window_state::StateFlags;
                app.handle().plugin(
                    tauri_plugin_window_state::Builder::default()
                        .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                        .build(),
                )?;
            }
            // Launch-at-login, toggled from Settings (LaunchAgent on macOS).
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None,
            ))?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // The handle is only used on macOS (the Opened arm); elsewhere it's idle.
    app.run(|_app, event| match event {
        // macOS delivers handler invocations (magnet URLs and .torrent file
        // opens alike) as open events, never via argv.
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Opened { urls } => {
            sources::queue(_app, sources::parse_args(urls.iter().map(|u| u.to_string())));
            tray::show_window(_app);
        }
        // Stop the daemon only after the event loop has ended: the webview is
        // gone, so its reconnect loop can't respawn the daemon mid-exit.
        // request_stop (not stop): the exiting app never rebinds the port,
        // so waiting for it to free would only slow every quit. One short
        // retry covers a daemon that is mid-spawn or mid-restart and not yet
        // discoverable. Bounded so a wedged daemon can't trap process exit;
        // an already-gone daemon is a no-op. OS shutdown doesn't reach this,
        // the daemon catches its own termination signal there.
        tauri::RunEvent::Exit => {
            tauri::async_runtime::block_on(async {
                let Ok(data_dir) = magneto_core::config::data_dir() else {
                    return;
                };
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    if magneto_core::supervisor::request_stop(&data_dir).await.is_err() {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        let _ = magneto_core::supervisor::request_stop(&data_dir).await;
                    }
                })
                .await;
            });
        }
        _ => {}
    });
}
