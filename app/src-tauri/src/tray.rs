//! Tray residency: closing the window only hides it; the tray menu's Quit
//! (or the frontend's confirmed quit chord) is the real exit. Every quit
//! route funnels through app.exit, so the daemon stop in lib.rs's
//! RunEvent::Exit runs exactly once per exit.

use tauri::menu::{Menu, MenuItem};
use tauri::{AppHandle, Manager};

/// Wire tray residency: close-to-hide on the main window, and the menu on the
/// tray icon declared in tauri.conf.json (declared there, not built here, so
/// the bundlers know to depend on and ship the appindicator libraries on
/// Linux). Tray failures are non-fatal. Without the menu the app still
/// works: relaunching reopens the hidden window via single-instance, and the
/// quit chord still exits.
pub fn init(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let handle = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = handle.hide();
            }
        });
    }
    if let Err(e) = init_menu(app) {
        eprintln!("tray menu setup failed: {e}");
    }
}

fn init_menu(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Magneto", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Quit Magneto", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &exit])?;
    let Some(tray) = app.tray_by_id("main") else {
        // Config-declared tray missing: nothing to attach to.
        eprintln!("tray icon not found; tray menu disabled");
        return Ok(());
    };
    tray.set_menu(Some(menu))?;
    tray.on_menu_event(|app, event| match event.id().as_ref() {
        "open" => show_window(app),
        "exit" => app.exit(0),
        _ => {}
    });
    Ok(())
}

/// Show + focus the main window: tray Open, a second launch, or an OS handler
/// invocation (a magnet click must surface the app it lands in).
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
