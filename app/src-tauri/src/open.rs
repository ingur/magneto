// Opens folders and URLs with the OS default handler.
//
// The opener plugin spawns `xdg-open` fire-and-forget with the inherited
// environment, which breaks inside an AppImage: AppRun prepends the bundle
// to PATH (the bundled xdg-open shadows the system one) and exports
// LD_LIBRARY_PATH plus GTK/GLib module paths pointing into the bundle, so
// host programs inheriting them load mismatched libraries and crash, with
// the detached spawn reporting success regardless. These commands spawn the
// opener with a sanitized environment instead: every entry pointing into
// $APPDIR is dropped. Outside an AppImage ($APPDIR unset) the environment
// passes through unchanged.

#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

/// Open an existing filesystem path (in practice a folder; files are
/// revealed via the opener plugin) with the OS default handler.
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    std::fs::metadata(&path).map_err(|e| format!("{path}: {e}"))?;
    #[cfg(target_os = "linux")]
    if open_sanitized(&path) {
        return Ok(());
    }
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())
}

/// Open a URL with the OS default application for its scheme.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    if open_sanitized(&url) {
        return Ok(());
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| e.to_string())
}

/// Launch the system opener with a sanitized environment (core::spawn_env;
/// PATH is among the sanitized values and Command resolves programs through
/// the child's PATH, so the launchers themselves resolve outside the
/// bundle). `gio open` first: it resolves the default handler the way the
/// desktop does and prefers D-Bus activation, so the handler runs with the
/// session's own environment rather than this process's, and its exit
/// status honestly reports "no handler". `xdg-open` second: its generic
/// mode can stay in the foreground for as long as the handler runs, so it
/// is spawned, never awaited. Returns false when no launcher accepted the
/// target, letting the caller fall back to the opener plugin.
#[cfg(target_os = "linux")]
fn open_sanitized(target: &str) -> bool {
    let env = magneto_core::spawn_env::sanitized();

    let opened = Command::new("gio")
        .args(["open", target])
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if opened {
        return true;
    }

    Command::new("xdg-open")
        .arg(target)
        .env_clear()
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}
