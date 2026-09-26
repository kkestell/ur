mod commands;
mod gui_state;
mod link;

use link::Link;
use tauri::{Manager, WindowEvent};

fn main() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init());
    #[cfg(feature = "webdriver")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .manage(Link::new(ur_client::socket_path()))
        .setup(|app| {
            tauri::async_runtime::spawn(Link::run(app.handle().clone()));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Focused(focused) = event {
                window.state::<Link>().set_focused(*focused);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::request,
            commands::attach_terminal,
            commands::detach_terminal,
            commands::terminal_input,
            commands::connection,
            commands::layout,
            commands::save_layout,
            commands::set_visible,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the ur app");
}
