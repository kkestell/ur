use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use bytes::Bytes;
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{sleep, timeout};
use ur_client::{Client, Request, Response, TerminalId};

const WAIT: Duration = Duration::from_secs(5);

/// A daemon on a socket in its own directory, which is also `$HOME` and
/// `$XDG_STATE_HOME`. `/bin/sh` keeps user shell configuration out of the
/// terminals.
struct Daemon {
    process: Child,
    socket: PathBuf,
    _dir: TempDir,
}

impl Daemon {
    fn start() -> Daemon {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("ur.sock");
        let process = Command::new(env!("CARGO_BIN_EXE_ur"))
            .arg("daemon")
            .env("UR_SOCKET", &socket)
            .env("HOME", dir.path())
            .env("XDG_STATE_HOME", dir.path())
            .env("SHELL", "/bin/sh")
            .spawn()
            .unwrap();
        Daemon {
            process,
            socket,
            _dir: dir,
        }
    }

    async fn connect(&self) -> Client {
        timeout(WAIT, async {
            loop {
                match Client::connect(&self.socket).await {
                    Ok(client) => return client,
                    Err(_) => sleep(Duration::from_millis(20)).await,
                }
            }
        })
        .await
        .expect("the daemon accepts a connection")
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// A terminal attachment as a daemon client sees it.
struct View {
    pty: UnboundedReceiver<Bytes>,
    parser: vt100::Parser,
}

impl View {
    /// Feeds output to the parser until a screen row reads exactly `line`.
    async fn wait_for_line(&mut self, line: &str) {
        let found = timeout(WAIT, async {
            while !self.has_line(line) {
                let bytes = self.pty.recv().await.expect("the terminal is running");
                self.parser.process(&bytes);
            }
        })
        .await;
        assert!(
            found.is_ok(),
            "no line {line:?} on screen:\n{}",
            self.parser.screen().contents()
        );
    }

    fn has_line(&self, line: &str) -> bool {
        let contents = self.parser.screen().contents();
        contents.lines().any(|row| row.trim_end() == line)
    }
}

async fn open(client: &Client) -> TerminalId {
    match client.request(Request::OpenTerminal).await.unwrap() {
        Response::Opened { terminal } => terminal,
        other => panic!("open_terminal answered {other:?}"),
    }
}

async fn attach(client: &Client, terminal: TerminalId, rows: u16, cols: u16) -> View {
    let pty = client.pty(terminal);
    let request = Request::AttachTerminal {
        terminal,
        rows,
        cols,
    };
    let response = client.request(request).await.unwrap();
    assert_eq!(response, Response::Done);
    View {
        pty,
        parser: vt100::Parser::new(rows, cols, 0),
    }
}

fn run(client: &Client, terminal: TerminalId, command: &str) {
    let input = Bytes::from(format!("{command}\r"));
    client.pty_input(terminal, input).unwrap();
}

#[tokio::test]
async fn attach_sends_the_screen_then_live_output() {
    let daemon = Daemon::start();
    let first = daemon.connect().await;
    let terminal = open(&first).await;
    let mut first_view = attach(&first, terminal, 24, 80).await;
    run(&first, terminal, "echo one");
    first_view.wait_for_line("one").await;

    let second = daemon.connect().await;
    let mut second_view = attach(&second, terminal, 24, 80).await;
    let snapshot = second_view.pty.recv().await.unwrap();
    second_view.parser.process(&snapshot);
    assert!(
        second_view.has_line("one"),
        "the first frame is the screen snapshot:\n{}",
        second_view.parser.screen().contents()
    );

    run(&second, terminal, "echo two");
    second_view.wait_for_line("two").await;
}

#[tokio::test]
async fn terminal_outlives_its_socket_connection() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let terminal = open(&client).await;
    let mut view = attach(&client, terminal, 24, 80).await;
    run(&client, terminal, "echo kept");
    view.wait_for_line("kept").await;
    drop(view);
    drop(client);

    let client = daemon.connect().await;
    let mut view = attach(&client, terminal, 24, 80).await;
    view.wait_for_line("kept").await;
    run(&client, terminal, "echo still running");
    view.wait_for_line("still running").await;
}

#[tokio::test]
async fn attach_resizes_the_terminal() {
    let daemon = Daemon::start();
    let first = daemon.connect().await;
    let terminal = open(&first).await;
    attach(&first, terminal, 24, 80).await;

    let second = daemon.connect().await;
    let mut view = attach(&second, terminal, 30, 100).await;
    run(&second, terminal, "stty size");
    view.wait_for_line("30 100").await;
}

#[tokio::test]
async fn attaching_an_unknown_terminal_is_an_error() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let request = Request::AttachTerminal {
        terminal: 99,
        rows: 24,
        cols: 80,
    };
    let response = client.request(request).await.unwrap();
    assert!(
        matches!(response, Response::Error { .. }),
        "attaching an unknown terminal answered {response:?}"
    );
}

#[tokio::test]
async fn shell_exit_ends_the_terminal() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let terminal = open(&client).await;
    let mut view = attach(&client, terminal, 24, 80).await;
    run(&client, terminal, "exit");
    let closed = timeout(WAIT, async { while view.pty.recv().await.is_some() {} }).await;
    assert!(
        closed.is_ok(),
        "the pty receiver closes after the shell exits"
    );

    let request = Request::AttachTerminal {
        terminal,
        rows: 24,
        cols: 80,
    };
    let response = client.request(request).await.unwrap();
    assert!(
        matches!(response, Response::Error { .. }),
        "attaching an exited terminal answered {response:?}"
    );
}
