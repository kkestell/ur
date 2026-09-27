//! The menu bar icon, which keeps the app reachable while no window is open.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, WebviewWindowBuilder};

const OPEN: &str = "open";

/// Adds the menu bar icon and its menu.
pub fn build(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN, "Open Ur", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open])?;
    TrayIconBuilder::new()
        .icon(tauri::include_image!("icons/tray.png"))
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            if event.id() == OPEN
                && let Err(error) = show_main_window(app)
            {
                eprintln!("ur-app: opening the window: {error}");
            }
        })
        .build(app)?;
    Ok(())
}

/// Shows and focuses the main window, first creating it from the config if
/// the user closed it.
pub fn show_main_window(app: &AppHandle) -> tauri::Result<()> {
    let window = match app.get_webview_window("main") {
        Some(window) => window,
        None => WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?.build()?,
    };
    window.unminimize()?;
    window.show()?;
    window.set_focus()
}
