use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use bytes::Bytes;
use tempfile::TempDir;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::time::{sleep, timeout};
use ur_client::{Client, Event, Request, Response, TerminalId, TerminalSummary};

const WAIT: Duration = Duration::from_secs(5);

/// A daemon on a socket in its own directory, which is also `$HOME`,
/// `$XDG_STATE_HOME`, and the path of its one workspace, `home`. `/bin/sh`
/// keeps user shell configuration out of the terminals.
struct Daemon {
    process: Child,
    socket: PathBuf,
    dir: TempDir,
}

impl Daemon {
    fn start() -> Daemon {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("ur.sock");
        let state = serde_json::json!({
            "workspaces": [{ "name": "home", "path": dir.path() }],
        });
        std::fs::create_dir(dir.path().join("ur")).unwrap();
        std::fs::write(dir.path().join("ur/state.json"), state.to_string()).unwrap();
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
            dir,
        }
    }

    fn home(&self) -> PathBuf {
        self.dir.path().to_path_buf()
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

    async fn wait_for_close(&mut self) {
        let closed = timeout(WAIT, async { while self.pty.recv().await.is_some() {} }).await;
        assert!(closed.is_ok(), "the pty receiver closes");
    }
}

/// Opens a terminal in `home`.
async fn open(client: &Client) -> TerminalId {
    let request = Request::OpenTerminal {
        workspace: "home".to_string(),
    };
    match client.request(request).await.unwrap() {
        Response::Opened { terminal } => terminal,
        other => panic!("open_terminal answered {other:?}"),
    }
}

/// Watches, and returns the watch snapshot's terminals and the later events.
async fn watch(client: &Client) -> (Vec<TerminalSummary>, UnboundedReceiver<Event>) {
    let mut events = client.events();
    assert_eq!(
        client.request(Request::Watch).await.unwrap(),
        Response::Done
    );
    match events.try_recv() {
        Ok(Event::WatchSnapshot { terminals, .. }) => (terminals, events),
        other => panic!("expected the watch snapshot before the response, got {other:?}"),
    }
}

async fn next_event(events: &mut UnboundedReceiver<Event>) -> Event {
    timeout(WAIT, events.recv())
        .await
        .expect("an event arrives")
        .expect("the socket connection is open")
}

/// Skips events until `expected` arrives.
async fn wait_for_event(events: &mut UnboundedReceiver<Event>, expected: Event) {
    while next_event(events).await != expected {}
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
async fn unknown_terminals_and_workspaces_are_errors() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let cases = [
        (
            "attaching an unknown terminal",
            Request::AttachTerminal {
                terminal: 99,
                rows: 24,
                cols: 80,
            },
        ),
        (
            "detaching an unknown terminal",
            Request::DetachTerminal { terminal: 99 },
        ),
        (
            "closing an unknown terminal",
            Request::CloseTerminal { terminal: 99 },
        ),
        (
            "opening a terminal in an unknown workspace",
            Request::OpenTerminal {
                workspace: "nowhere".to_string(),
            },
        ),
    ];
    for (case, request) in cases {
        let response = client.request(request).await.unwrap();
        assert!(
            matches!(response, Response::Error { .. }),
            "{case} answered {response:?}"
        );
    }
}

#[tokio::test]
async fn terminal_starts_in_its_workspace() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let terminal = open(&client).await;
    let mut view = attach(&client, terminal, 24, 200).await;
    run(&client, terminal, "pwd -P");
    let home = daemon.home().canonicalize().unwrap();
    view.wait_for_line(home.to_str().unwrap()).await;
}

#[tokio::test]
async fn watch_shows_terminals_and_their_titles() {
    let daemon = Daemon::start();
    let watcher = daemon.connect().await;
    let (_, mut events) = watch(&watcher).await;
    let client = daemon.connect().await;
    let terminal = open(&client).await;
    let summary = |title: &str| TerminalSummary {
        terminal,
        workspace: "home".to_string(),
        title: title.to_string(),
    };
    assert_eq!(
        next_event(&mut events).await,
        Event::TerminalChanged {
            summary: summary("sh")
        }
    );

    run(&client, terminal, r"printf '\033]2;building\007'");
    assert_eq!(
        next_event(&mut events).await,
        Event::TerminalChanged {
            summary: summary("building")
        }
    );
    let (terminals, _) = watch(&daemon.connect().await).await;
    assert_eq!(terminals, [summary("building")]);
}

#[tokio::test]
async fn close_terminal_stops_its_programs() {
    let daemon = Daemon::start();
    let watcher = daemon.connect().await;
    let (_, mut events) = watch(&watcher).await;
    let client = daemon.connect().await;
    let terminal = open(&client).await;
    let mut view = attach(&client, terminal, 24, 80).await;
    run(&client, terminal, "echo started; sleep 1000");
    view.wait_for_line("started").await;

    assert_eq!(
        client
            .request(Request::CloseTerminal { terminal })
            .await
            .unwrap(),
        Response::Done
    );
    // The PTY closes only once `sleep`, which holds it open, has exited too.
    view.wait_for_close().await;
    wait_for_event(&mut events, Event::TerminalExited { terminal }).await;
    let request = Request::AttachTerminal {
        terminal,
        rows: 24,
        cols: 80,
    };
    let response = client.request(request).await.unwrap();
    assert!(
        matches!(response, Response::Error { .. }),
        "attaching a closed terminal answered {response:?}"
    );
}

#[tokio::test]
async fn removing_a_workspace_stops_its_terminals() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let terminal = open(&client).await;
    let mut view = attach(&client, terminal, 24, 80).await;
    let request = Request::RemoveWorkspace {
        name: "home".to_string(),
    };
    assert_eq!(client.request(request).await.unwrap(), Response::Done);
    view.wait_for_close().await;
}

#[tokio::test]
async fn a_detached_connection_gets_no_output() {
    let daemon = Daemon::start();
    let first = daemon.connect().await;
    let terminal = open(&first).await;
    let mut first_view = attach(&first, terminal, 24, 80).await;
    let detach = Request::DetachTerminal { terminal };
    assert_eq!(first.request(detach.clone()).await.unwrap(), Response::Done);
    // Drop the screen snapshot and any output from before the detach.
    while first_view.pty.try_recv().is_ok() {}

    let second = daemon.connect().await;
    let mut second_view = attach(&second, terminal, 24, 80).await;
    run(&second, terminal, "echo after");
    second_view.wait_for_line("after").await;

    // Output fans out to every attached outbox at once, so a `PTY` frame
    // queued for the first socket connection would arrive before this
    // response.
    assert_eq!(first.request(detach).await.unwrap(), Response::Done);
    assert_eq!(first_view.pty.try_recv(), Err(TryRecvError::Empty));
}

#[tokio::test]
async fn shell_exit_ends_the_terminal() {
    let daemon = Daemon::start();
    let watcher = daemon.connect().await;
    let (_, mut events) = watch(&watcher).await;
    let client = daemon.connect().await;
    let terminal = open(&client).await;
    let mut view = attach(&client, terminal, 24, 80).await;
    run(&client, terminal, "exit");
    view.wait_for_close().await;
    wait_for_event(&mut events, Event::TerminalExited { terminal }).await;

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
