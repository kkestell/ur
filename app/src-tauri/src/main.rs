mod commands;
mod gui_state;
mod link;

use link::Link;

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "webdriver")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .manage(Link::new(ur_client::socket_path()))
        .setup(|app| {
            tauri::async_runtime::spawn(Link::run(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::request,
            commands::attach_terminal,
            commands::terminal_input,
            commands::connection,
            commands::selection,
            commands::select,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the ur app");
}
