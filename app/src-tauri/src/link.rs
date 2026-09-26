use std::collections::{BTreeSet, HashMap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use tauri::ipc::{Channel, Response as ChannelBytes};
use tauri::{AppHandle, Emitter, Manager};
use tokio::task::AbortHandle;
use ur_client::{Client, Event, Request, Response, TerminalId};

use crate::gui_state::Connection;

/// The wait between connection attempts.
const RETRY_DELAY: Duration = Duration::from_millis(500);

/// The core's owner of `ur_client::Client`. `run()` reconnects to the daemon
/// and replays the desired set after a disconnect.
pub struct Link {
    socket: PathBuf,
    desired: Mutex<Desired>,
    client: Mutex<Option<Client>>,
    /// The task forwarding each attached terminal's output.
    attachments: Mutex<HashMap<TerminalId, AbortHandle>>,
}

/// The desired set: what `run()` restores after reconnecting. The focus sent
/// to the daemon is `visible` while the window is focused and nothing
/// otherwise.
struct Desired {
    watch: bool,
    subscribed: BTreeSet<String>,
    visible: Vec<String>,
    /// Whether the window has focus. It does when the window opens.
    focused: bool,
}

impl Default for Desired {
    fn default() -> Desired {
        Desired {
            watch: false,
            subscribed: BTreeSet::new(),
            visible: Vec::new(),
            focused: true,
        }
    }
}

impl Desired {
    /// The sessions to focus: the visible ones while the window has focus.
    fn focus(&self) -> Vec<String> {
        if self.focused {
            self.visible.clone()
        } else {
            Vec::new()
        }
    }
}

impl Link {
    pub fn new(socket: PathBuf) -> Link {
        Link {
            socket,
            desired: Mutex::new(Desired::default()),
            client: Mutex::new(None),
            attachments: Mutex::new(HashMap::new()),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The reconnect loop: connects, retrying every `RETRY_DELAY`, replays the
    /// desired set, forwards events to the webview until the socket connection
    /// ends, and starts over. Emits `connection` at each change.
    pub async fn run(app: AppHandle) {
        let link = app.state::<Link>();
        loop {
            let client = loop {
                match Client::connect(&link.socket).await {
                    Ok(client) => break client,
                    Err(_) => tokio::time::sleep(RETRY_DELAY).await,
                }
            };
            // Take the receiver before any request so no snapshot is missed.
            let mut events = client.events();
            *link.client.lock().unwrap() = Some(client.clone());
            link.emit_connection(&app, true);

            let (watch, subscribed, focus) = {
                let desired = link.desired.lock().unwrap();
                (desired.watch, desired.subscribed.clone(), desired.focus())
            };
            let mut replay = Vec::new();
            if watch {
                replay.push(Request::Watch);
            }
            replay.extend(subscribed.into_iter().map(|session| Request::Subscribe {
                session: session.into(),
            }));
            for request in replay {
                let client = client.clone();
                // The daemon answers each with a fresh snapshot, which arrives
                // as an event; the response is not needed.
                tokio::spawn(async move {
                    let _ = client.request(request).await;
                });
            }
            // The daemon has listed every saved session before it accepts a
            // socket connection, so the focus can name one right away.
            Link::send_focus(client.clone(), focus);

            while let Some(event) = events.recv().await {
                if let Event::SessionRemoved { session } = &event {
                    link.desired
                        .lock()
                        .unwrap()
                        .subscribed
                        .remove(&session.to_string());
                }
                let name = match &event {
                    Event::WatchSnapshot { .. }
                    | Event::CapabilitiesChanged { .. }
                    | Event::WorkspaceAdded { .. }
                    | Event::WorkspaceRemoved { .. }
                    | Event::SessionChanged { .. }
                    | Event::SessionDeleted { .. }
                    | Event::TerminalChanged { .. }
                    | Event::TerminalExited { .. } => "watch",
                    Event::SessionSnapshot { .. }
                    | Event::Entry { .. }
                    | Event::ConfigOptionsChanged { .. }
                    | Event::SessionRemoved { .. } => "session",
                };
                if let Err(error) = app.emit(name, &event) {
                    eprintln!("ur-app: emitting {name}: {error}");
                }
            }

            *link.client.lock().unwrap() = None;
            link.emit_connection(&app, false);
        }
    }

    /// Records the sessions the webview shows and sends the focus.
    pub fn set_visible(&self, sessions: Vec<String>) {
        let focus = {
            let mut desired = self.desired.lock().unwrap();
            desired.visible = sessions;
            desired.focus()
        };
        if let Ok(client) = self.client() {
            Link::send_focus(client, focus);
        }
    }

    /// Records whether the window has focus and sends the focus.
    pub fn set_focused(&self, focused: bool) {
        let focus = {
            let mut desired = self.desired.lock().unwrap();
            desired.focused = focused;
            desired.focus()
        };
        if let Ok(client) = self.client() {
            Link::send_focus(client, focus);
        }
    }

    /// Sends `focus` in a spawned task. A failure, such as naming a session
    /// the daemon no longer has, changes nothing in the daemon and the next
    /// selection change sends a fresh one, so it is only logged. Spawned on
    /// Tauri's runtime, since the window event handler runs outside Tokio.
    fn send_focus(client: Client, sessions: Vec<String>) {
        tauri::async_runtime::spawn(async move {
            let request = Request::Focus {
                sessions: sessions.into_iter().map(Into::into).collect(),
            };
            match client.request(request).await {
                Ok(Response::Error { message }) => eprintln!("ur-app: focus: {message}"),
                Ok(_) => {}
                Err(error) => eprintln!("ur-app: focus: {error}"),
            }
        });
    }

    pub fn connection(&self) -> Connection {
        Connection {
            connected: self.client.lock().unwrap().is_some(),
            socket: self.socket.display().to_string(),
        }
    }

    /// Sends the request. `watch` and `subscribe` join the desired set first
    /// and answer `Done` while disconnected, because the replay after the next
    /// connect sends them. Every other request fails while disconnected.
    pub async fn request(&self, request: Request) -> io::Result<Response> {
        let replayed = match &request {
            Request::Watch => {
                self.desired.lock().unwrap().watch = true;
                true
            }
            Request::Subscribe { session } => {
                self.desired
                    .lock()
                    .unwrap()
                    .subscribed
                    .insert(session.to_string());
                true
            }
            _ => false,
        };
        match self.client() {
            Ok(client) => client.request(request).await,
            Err(_) if replayed => Ok(Response::Done),
            Err(error) => Err(error),
        }
    }

    /// Attaches the terminal and forwards its output to `output` until the
    /// terminal exits, the socket connection ends, or `detach()`. Attaching
    /// the same terminal again closes the earlier `pty` receiver, which ends
    /// the earlier forwarding task.
    pub async fn attach(
        &self,
        terminal: TerminalId,
        rows: u16,
        cols: u16,
        output: Channel<ChannelBytes>,
    ) -> Result<(), String> {
        let client = self.client().map_err(|error| error.to_string())?;
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
        let task = tokio::spawn(async move {
            while let Some(bytes) = pty.recv().await {
                if output.send(ChannelBytes::new(bytes.to_vec())).is_err() {
                    return;
                }
            }
        });
        self.attachments
            .lock()
            .unwrap()
            .insert(terminal, task.abort_handle());
        Ok(())
    }

    /// Stops forwarding the terminal's output and sends `detach_terminal`.
    /// Aborting the forwarding task drops its `pty` receiver, so `Client`
    /// drops the route. While disconnected, there is no terminal attachment
    /// to end.
    pub async fn detach(&self, terminal: TerminalId) -> Result<(), String> {
        if let Some(task) = self.attachments.lock().unwrap().remove(&terminal) {
            task.abort();
        }
        let Ok(client) = self.client() else {
            return Ok(());
        };
        match client.request(Request::DetachTerminal { terminal }).await {
            Ok(Response::Done) => Ok(()),
            Ok(Response::Error { message }) => Err(message),
            Ok(other) => Err(format!("unexpected response {other:?}")),
            Err(error) => Err(error.to_string()),
        }
    }

    pub async fn terminal_input(&self, terminal: TerminalId, data: String) -> io::Result<()> {
        self.client()?.pty_input(terminal, data.into())
    }

    fn client(&self) -> io::Result<Client> {
        self.client.lock().unwrap().clone().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "not connected to the daemon")
        })
    }

    fn emit_connection(&self, app: &AppHandle, connected: bool) {
        let connection = Connection {
            connected,
            socket: self.socket.display().to_string(),
        };
        if let Err(error) = app.emit("connection", connection) {
            eprintln!("ur-app: emitting connection: {error}");
        }
    }
}
