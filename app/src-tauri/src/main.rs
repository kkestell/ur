#[cfg(all(feature = "webdriver", target_os = "macos"))]
mod background;
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
    let app = builder
        .manage(Link::new(ur_client::socket_path()))
        .setup(|app| {
            #[cfg(all(feature = "webdriver", target_os = "macos"))]
            background::show_main_window(app)?;
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
    app.run(|_, _| {});
}
