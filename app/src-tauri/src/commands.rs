use tauri::State;
use tauri::ipc::{Channel, Response as ChannelBytes};
use ur_client::{Request, Response, TerminalId};

use crate::gui_state::{Connection, GuiState, Selection};
use crate::link::Link;

#[tauri::command]
pub async fn request(link: State<'_, Link>, request: Request) -> Result<Response, String> {
    link.request(request)
        .await
        .map_err(|error| error.to_string())
}

/// Attaches the terminal and forwards its output to `output`.
#[tauri::command]
pub async fn attach_terminal(
    link: State<'_, Link>,
    terminal: TerminalId,
    rows: u16,
    cols: u16,
    output: Channel<ChannelBytes>,
) -> Result<(), String> {
    link.attach(terminal, rows, cols, output).await
}

/// Ends the terminal attachment. The terminal keeps running.
#[tauri::command]
pub async fn detach_terminal(link: State<'_, Link>, terminal: TerminalId) -> Result<(), String> {
    link.detach(terminal).await
}

#[tauri::command]
pub async fn terminal_input(
    link: State<'_, Link>,
    terminal: TerminalId,
    data: String,
) -> Result<(), String> {
    link.terminal_input(terminal, data)
        .await
        .map_err(|error| error.to_string())
}

/// Reports the sessions the webview shows. The core combines them with the
/// window's focus and sends `focus` to the daemon.
#[tauri::command]
pub fn set_visible(link: State<'_, Link>, sessions: Vec<String>) {
    link.set_visible(sessions);
}

/// The current connection, for a webview that started after the `connection`
/// event it would have needed.
#[tauri::command]
pub fn connection(link: State<'_, Link>) -> Connection {
    link.connection()
}

/// The saved selection.
#[tauri::command]
pub fn selection(link: State<'_, Link>) -> Result<Option<Selection>, String> {
    let gui_state = GuiState::load().map_err(|error| error.to_string())?;
    Ok(gui_state.saved(link.socket()).selection)
}

/// Saves the selection.
#[tauri::command]
pub fn select(link: State<'_, Link>, selection: Selection) -> Result<(), String> {
    let mut gui_state = GuiState::load().map_err(|error| error.to_string())?;
    gui_state.saved_mut(link.socket()).selection = Some(selection);
    gui_state.save().map_err(|error| error.to_string())
}
