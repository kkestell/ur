use tauri::State;
use tauri::ipc::{Channel, Response as ChannelBytes};
use ur_client::{Request, Response, TerminalId};

use crate::gui_state::{GuiState, Selection};
use crate::link::Link;

#[tauri::command]
pub async fn request(link: State<'_, Link>, request: Request) -> Result<Response, String> {
    link.request(request)
        .await
        .map_err(|error| error.to_string())
}

/// Attaches the selected terminal. When there is no selection, or the daemon
/// rejects the attachment, opens a new terminal, attaches it, and selects it.
#[tauri::command]
pub async fn attach_terminal(
    link: State<'_, Link>,
    rows: u16,
    cols: u16,
    output: Channel<ChannelBytes>,
) -> Result<TerminalId, String> {
    let mut gui_state = GuiState::load().map_err(|error| error.to_string())?;
    if let Some(Selection::Terminal(terminal)) = gui_state.selection(link.socket())
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
    gui_state.select(link.socket(), Selection::Terminal(terminal));
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
