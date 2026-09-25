use std::io;
use std::path::{Path, PathBuf};

use tauri::ipc::{Channel, Response as ChannelBytes};
use tokio::sync::OnceCell;
use ur_client::{Client, Request, Response, TerminalId};

const SHELL_EXITED: &[u8] = b"\r\n[shell exited]\r\n";

/// The core's owner of `ur_client::Client`. It connects on first use and does
/// not reconnect yet.
pub struct Link {
    socket: PathBuf,
    client: OnceCell<Client>,
}

impl Link {
    pub fn new(socket: PathBuf) -> Link {
        Link {
            socket,
            client: OnceCell::new(),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub async fn request(&self, request: Request) -> io::Result<Response> {
        self.client().await?.request(request).await
    }

    /// Attaches the terminal and forwards its output to `output` until the
    /// terminal exits or the socket connection ends.
    pub async fn attach(
        &self,
        terminal: TerminalId,
        rows: u16,
        cols: u16,
        output: Channel<ChannelBytes>,
    ) -> Result<(), String> {
        let client = self.client().await.map_err(|error| error.to_string())?;
        let mut pty = client.pty(terminal);
        let request = Request::AttachTerminal {
            terminal,
            rows,
            cols,
        };
        match client.request(request).await {
            Ok(Response::Done) => {}
            Ok(Response::Error { message }) => return Err(message),
            Ok(other) => return Err(format!("unexpected response {other:?}")),
            Err(error) => return Err(error.to_string()),
        }
        tokio::spawn(async move {
            while let Some(bytes) = pty.recv().await {
                if output.send(ChannelBytes::new(bytes.to_vec())).is_err() {
                    return;
                }
            }
            let _ = output.send(ChannelBytes::new(SHELL_EXITED.to_vec()));
        });
        Ok(())
    }

    pub async fn terminal_input(&self, terminal: TerminalId, data: String) -> io::Result<()> {
        self.client().await?.pty_input(terminal, data.into())
    }

    async fn client(&self) -> io::Result<&Client> {
        self.client
            .get_or_try_init(|| Client::connect(&self.socket))
            .await
    }
}
