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

/// Attaches the GUI's terminal from the GUI state file. When there is none, or
/// the daemon rejects the attachment, opens a new terminal, attaches it, and
/// saves it.
#[tauri::command]
pub async fn attach_terminal(
    link: State<'_, Link>,
    rows: u16,
    cols: u16,
    output: Channel<ChannelBytes>,
) -> Result<TerminalId, String> {
    let mut gui_state = GuiState::load().map_err(|error| error.to_string())?;
    if let Some(terminal) = gui_state.saved(link.socket()).terminal
        && link
            .attach(terminal, rows, cols, output.clone())
            .await
            .is_ok()
    {
        return Ok(terminal);
    }

    let terminal = match link.request(Request::OpenTerminal).await {
        Ok(Response::Opened { terminal }) => terminal,
        Ok(Response::Error { message }) => return Err(message),
        Ok(other) => return Err(format!("unexpected response {other:?}")),
        Err(error) => return Err(error.to_string()),
    };
    link.attach(terminal, rows, cols, output).await?;
    gui_state.saved_mut(link.socket()).terminal = Some(terminal);
    gui_state.save().map_err(|error| error.to_string())?;
    Ok(terminal)
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
