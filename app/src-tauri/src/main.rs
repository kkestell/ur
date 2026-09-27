#[cfg(all(feature = "webdriver", target_os = "macos"))]
mod background;
mod commands;
mod gui_state;
mod link;
mod tray;

use link::Link;
use tauri::{Manager, RunEvent, WindowEvent};

fn main() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    #[cfg(feature = "webdriver")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    let app = builder
        .manage(Link::new(ur_client::socket_path()))
        .setup(|app| {
            #[cfg(all(feature = "webdriver", target_os = "macos"))]
            background::show_main_window(app)?;
            tray::build(app)?;
            tauri::async_runtime::spawn(Link::run(app.handle().clone()));
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(focused) => window.state::<Link>().set_focused(*focused),
            WindowEvent::Destroyed => window.state::<Link>().set_focused(false),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::request,
            commands::attach_terminal,
            commands::detach_terminal,
            commands::terminal_input,
            commands::connection,
            commands::layout,
            commands::save_layout,
            commands::sidebar_width,
            commands::save_sidebar_width,
            commands::set_visible,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the ur app");
    #[cfg(all(feature = "webdriver", target_os = "macos"))]
    let app = {
        let mut app = app;
        background::prohibit_activation(&mut app);
        app
    };
    app.run(|app, event| match event {
        // Closing the last window leaves the app running in the menu bar.
        // Only that exit request has no code.
        RunEvent::ExitRequested {
            code: None, api, ..
        } => api.prevent_exit(),
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            if let Err(error) = tray::show_main_window(app) {
                eprintln!("ur-app: opening the window: {error}");
            }
        }
        _ => {}
    });
}
