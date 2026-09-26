use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, anyhow, bail};
use bytes::Bytes;
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ur_client::{Frame, TerminalId, TerminalSummary};

use super::server::Outbox;
use super::state::State;

const INITIAL_ROWS: u16 = 24;
const INITIAL_COLS: u16 = 80;
const ENTER_ALTERNATE_SCREEN: &[u8] = b"\x1b[?1049h";

/// The daemon's terminals by ID, and the next ID. Their terminal summaries
/// live in `State`. Locks are taken in the order `State`, the terminal map,
/// then a terminal's `output`.
pub struct Terminals {
    state: Arc<Mutex<State>>,
    terminals: Mutex<HashMap<TerminalId, Arc<Terminal>>>,
    last_id: AtomicU32,
}

struct Terminal {
    output: Mutex<Output>,
    /// Separate from `output` so a slow write never blocks output.
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

/// What the reader thread and terminal attachments share. Holding one lock
/// for all of it means no output arrives between a screen snapshot and the
/// registration of its outbox.
struct Output {
    master: Box<dyn MasterPty + Send>,
    parser: vt100::Parser<Title>,
    outboxes: Vec<Outbox>,
}

/// Records the terminal title a program sets with OSC 0 or OSC 2.
struct Title {
    changed: Option<String>,
}

impl vt100::Callbacks for Title {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.changed = Some(String::from_utf8_lossy(title).into_owned());
    }
}

impl Terminals {
    pub fn new(state: Arc<Mutex<State>>) -> Terminals {
        Terminals {
            state,
            terminals: Mutex::default(),
            last_id: AtomicU32::default(),
        }
    }

    /// Starts a login shell in the workspace path. portable-pty runs `$SHELL`,
    /// whose file name is the first terminal title. `State` stays locked
    /// until the terminal summary is added, so the workspace cannot be removed
    /// first, and the reader thread starts only after it.
    pub fn open(self: &Arc<Self>, workspace: &str) -> anyhow::Result<TerminalId> {
        let mut state = self.state.lock().unwrap();
        let path = state.workspace_path(workspace)?;
        let pair = native_pty_system()
            .openpty(size(INITIAL_ROWS, INITIAL_COLS))
            .context("opening a PTY")?;
        let mut command = CommandBuilder::new_default_prog();
        command.cwd(path);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let shell = command.get_shell();
        let title = Path::new(&shell)
            .file_name()
            .map_or(shell.clone(), |name| name.to_string_lossy().into_owned());
        let child = pair
            .slave
            .spawn_command(command)
            .context("starting the shell")?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let id = self.last_id.fetch_add(1, Ordering::Relaxed) + 1;
        let terminal = Arc::new(Terminal {
            output: Mutex::new(Output {
                master: pair.master,
                // No scrollback: the screen snapshot never includes it.
                parser: vt100::Parser::new_with_callbacks(
                    INITIAL_ROWS,
                    INITIAL_COLS,
                    0,
                    Title { changed: None },
                ),
                outboxes: Vec::new(),
            }),
            writer: Mutex::new(writer),
            killer: Mutex::new(child.clone_killer()),
        });
        self.terminals.lock().unwrap().insert(id, terminal.clone());
        state.add_terminal(TerminalSummary {
            terminal: id,
            workspace: workspace.to_string(),
            title,
        });
        drop(state);

        let terminals = self.clone();
        std::thread::spawn(move || terminals.read(id, &terminal, reader, child));
        Ok(id)
    }

    /// Resizes the terminal to the daemon client's view, then queues the
    /// screen snapshot as the attachment's first `PTY` frame. The caller sends
    /// the response afterwards, so the snapshot arrives first.
    pub fn attach(
        &self,
        id: TerminalId,
        rows: u16,
        cols: u16,
        outbox: Outbox,
    ) -> anyhow::Result<()> {
        // Holding the map lock keeps the reader thread from removing the
        // terminal until the outbox is registered, so the outbox is sure to
        // receive `terminal_exited`.
        let terminals = self.terminals.lock().unwrap();
        let terminal = terminals.get(&id).ok_or_else(|| unknown(id))?;
        let mut output = terminal.output.lock().unwrap();
        output.resize(rows, cols)?;
        let snapshot = screen_snapshot(output.parser.screen());
        outbox.send(Frame::Pty {
            id,
            bytes: snapshot.into(),
        });
        // Attaching again from the same socket connection replaces the
        // earlier attachment instead of sending the output twice.
        output
            .outboxes
            .retain(|other| !other.same_connection(&outbox));
        output.outboxes.push(outbox);
        Ok(())
    }

    /// Stops sending the terminal's output to this outbox's socket connection.
    pub fn detach(&self, id: TerminalId, outbox: &Outbox) -> anyhow::Result<()> {
        let terminals = self.terminals.lock().unwrap();
        let terminal = terminals.get(&id).ok_or_else(|| unknown(id))?;
        terminal
            .output
            .lock()
            .unwrap()
            .outboxes
            .retain(|other| !other.same_connection(outbox));
        Ok(())
    }

    /// Sends `SIGHUP` to the shell without waiting. The shell passes it to its
    /// jobs, and the kernel sends it to the foreground process group when the
    /// shell exits. The reader thread ends the terminal once the PTY closes.
    pub fn close(&self, id: TerminalId) -> anyhow::Result<()> {
        let terminals = self.terminals.lock().unwrap();
        let terminal = terminals.get(&id).ok_or_else(|| unknown(id))?;
        terminal
            .killer
            .lock()
            .unwrap()
            .kill()
            .with_context(|| format!("closing terminal {id}"))
    }

    pub fn resize(&self, id: TerminalId, rows: u16, cols: u16) -> anyhow::Result<()> {
        let terminals = self.terminals.lock().unwrap();
        let terminal = terminals.get(&id).ok_or_else(|| unknown(id))?;
        terminal.output.lock().unwrap().resize(rows, cols)
    }

    pub fn write(&self, id: TerminalId, bytes: &[u8]) -> anyhow::Result<()> {
        let terminal = self
            .terminals
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| unknown(id))?;
        let mut writer = terminal.writer.lock().unwrap();
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    /// The reader thread: feeds the parser, fans the output out, reports
    /// terminal title changes, and ends the terminal when the shell exits.
    fn read(
        &self,
        id: TerminalId,
        terminal: &Terminal,
        mut reader: Box<dyn Read + Send>,
        mut child: Box<dyn Child + Send + Sync>,
    ) {
        let mut buffer = [0; 8192];
        loop {
            let len = match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(len) => len,
            };
            let title = {
                let mut output = terminal.output.lock().unwrap();
                output.parser.process(&buffer[..len]);
                let frame = Frame::Pty {
                    id,
                    bytes: Bytes::copy_from_slice(&buffer[..len]),
                };
                output.outboxes.retain(|outbox| outbox.send(frame.clone()));
                output.parser.callbacks_mut().changed.take()
            };
            if let Some(title) = title {
                self.state.lock().unwrap().set_terminal_title(id, title);
            }
        }

        let _ = child.wait();
        self.terminals.lock().unwrap().remove(&id);
        let attached = std::mem::take(&mut terminal.output.lock().unwrap().outboxes);
        self.state.lock().unwrap().remove_terminal(id, &attached);
    }
}

impl Output {
    fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        if rows == 0 || cols == 0 {
            bail!("terminal size {rows}x{cols} is empty");
        }
        if self.parser.screen().size() != (rows, cols) {
            self.master
                .resize(size(rows, cols))
                .context("resizing the PTY")?;
            self.parser.screen_mut().set_size(rows, cols);
        }
        Ok(())
    }
}

/// The terminal attachment's first bytes. `state_formatted()` draws the
/// visible grid, cursor, and input modes but does not switch to the alternate
/// screen, so a full-screen application restored without the prefix would
/// leave its last frame in the normal buffer when it quits.
fn screen_snapshot(screen: &vt100::Screen) -> Vec<u8> {
    let mut snapshot = Vec::new();
    if screen.alternate_screen() {
        snapshot.extend_from_slice(ENTER_ALTERNATE_SCREEN);
    }
    snapshot.extend(screen.state_formatted());
    snapshot
}

fn size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn unknown(id: TerminalId) -> anyhow::Error {
    anyhow!("no terminal {id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reproduces_the_screen() {
        let cases: [(&str, &[u8]); 2] = [
            (
                "colored text with the cursor mid-line",
                b"plain\r\n\x1b[1;31mbold red\x1b[0m \x1b[42mgreen\x1b[0m\r\nhalf\x1b[3D",
            ),
            (
                "active alternate screen",
                b"shell prompt\r\n\x1b[?1049h\x1b[H\x1b[2Jfull screen\x1b[5;10Hstatus",
            ),
        ];
        for (name, bytes) in cases {
            let mut original = vt100::Parser::new(10, 40, 0);
            original.process(bytes);
            let mut restored = vt100::Parser::new(10, 40, 0);
            restored.process(&screen_snapshot(original.screen()));

            let (original, restored) = (original.screen(), restored.screen());
            assert_eq!(
                restored.contents_formatted(),
                original.contents_formatted(),
                "{name}: contents"
            );
            assert_eq!(
                restored.cursor_position(),
                original.cursor_position(),
                "{name}: cursor"
            );
            assert_eq!(
                restored.alternate_screen(),
                original.alternate_screen(),
                "{name}: alternate screen"
            );
        }
    }
}
