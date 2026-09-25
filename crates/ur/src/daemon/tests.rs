use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionId, SessionUpdate, StopReason,
};
use agent_client_protocol::{Channel, ConnectTo};
use anyhow::anyhow;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;
use ur_client::{
    Client, Entry, Event, PendingPermission, Request, Response, SessionSummary, Status, Workspace,
};
use ur_fake_server::{Hold, fake_server};

const WAIT: Duration = Duration::from_secs(5);

/// The daemon running in this process on a socket and a state file in its own
/// directory.
struct TestDaemon {
    socket: PathBuf,
    state_file: PathBuf,
    _dir: TempDir,
}

impl TestDaemon {
    /// Runs the daemon with the fake server over an in-process channel.
    fn start(hold: &Hold) -> TestDaemon {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        TestDaemon::run_on(dir, state_file, fake(hold))
    }

    /// Runs another daemon with the fake server, on this daemon's state file.
    fn restart(&self, hold: &Hold) -> TestDaemon {
        TestDaemon::run_on(
            tempfile::tempdir().unwrap(),
            self.state_file.clone(),
            fake(hold),
        )
    }

    fn run(server: anyhow::Result<Channel>) -> TestDaemon {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        TestDaemon::run_on(dir, state_file, server)
    }

    fn run_on(dir: TempDir, state_file: PathBuf, server: anyhow::Result<Channel>) -> TestDaemon {
        let socket = dir.path().join("ur.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = server.map(|channel| ("the fake server".to_string(), channel));
        tokio::spawn(super::run(listener, state_file.clone(), server));
        TestDaemon {
            socket,
            state_file,
            _dir: dir,
        }
    }

    async fn connect(&self) -> Client {
        Client::connect(&self.socket).await.unwrap()
    }
}

fn fake(hold: &Hold) -> anyhow::Result<Channel> {
    let (client, agent) = Channel::duplex();
    tokio::spawn(fake_server(hold.clone()).connect_to(agent));
    Ok(client)
}

/// One daemon client's view of a subscribed session: the session snapshot,
/// then each `entry` event.
struct Subscription {
    session: SessionId,
    snapshot: Vec<Entry>,
    transcript: Vec<Entry>,
    events: UnboundedReceiver<Event>,
}

impl Subscription {
    /// Waits until the snapshot and the entries after it make `len` entries,
    /// and returns them.
    async fn wait_for(&mut self, len: usize) -> &[Entry] {
        while self.transcript.len() < len {
            let event = timeout(WAIT, self.events.recv())
                .await
                .unwrap_or_else(|_| panic!("entry {len} arrives: {:?}", self.transcript))
                .expect("the socket connection stays open");
            match event {
                Event::Entry { session, entry } if session == self.session => {
                    self.transcript.push(entry);
                }
                other => panic!("unexpected event {other:?}"),
            }
        }
        &self.transcript
    }
}

/// A watching daemon client: the watch snapshot, then each change.
struct Watch {
    workspaces: Vec<Workspace>,
    sessions: Vec<SessionSummary>,
    events: UnboundedReceiver<Event>,
    _client: Client,
}

impl Watch {
    async fn next(&mut self) -> Event {
        timeout(WAIT, self.events.recv())
            .await
            .expect("an event arrives")
            .expect("the socket connection stays open")
    }

    /// Waits for the next `session_changed` for `session`, skipping other
    /// events, and returns its summary.
    async fn next_change(&mut self, session: &SessionId) -> SessionSummary {
        loop {
            if let Event::SessionChanged { summary } = self.next().await
                && summary.session == *session
            {
                return summary;
            }
        }
    }
}

async fn watch(daemon: &TestDaemon) -> Watch {
    let client = daemon.connect().await;
    let mut events = client.events();
    assert_eq!(
        client.request(Request::Watch).await.unwrap(),
        Response::Done
    );
    match events.try_recv() {
        Ok(Event::WatchSnapshot {
            workspaces,
            sessions,
        }) => Watch {
            workspaces,
            sessions,
            events,
            _client: client,
        },
        other => panic!("expected the watch snapshot before the response, got {other:?}"),
    }
}

/// An absolute path to a directory, for workspaces.
fn workspace_path() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn workspace(name: &str) -> Workspace {
    Workspace {
        name: name.to_string(),
        path: workspace_path().to_path_buf(),
    }
}

async fn add_workspace(client: &Client, name: &str) -> Response {
    let request = Request::AddWorkspace {
        name: name.to_string(),
        path: workspace_path().to_path_buf(),
    };
    client.request(request).await.unwrap()
}

async fn remove_workspace(client: &Client, name: &str) -> Response {
    let request = Request::RemoveWorkspace {
        name: name.to_string(),
    };
    client.request(request).await.unwrap()
}

/// Adds the workspace, then creates a session in it.
async fn new_session(client: &Client, workspace: &str) -> SessionId {
    assert_eq!(add_workspace(client, workspace).await, Response::Done);
    let request = Request::NewSession {
        workspace: workspace.to_string(),
    };
    match client.request(request).await.unwrap() {
        Response::SessionCreated { session } => session,
        other => panic!("unexpected response {other:?}"),
    }
}

async fn subscribe(client: &Client, session: &SessionId) -> Subscription {
    let mut events = client.events();
    let request = Request::Subscribe {
        session: session.clone(),
    };
    assert_eq!(client.request(request).await.unwrap(), Response::Done);
    match events.try_recv() {
        Ok(Event::SessionSnapshot {
            session: snapshot_session,
            transcript,
        }) if snapshot_session == *session => Subscription {
            session: session.clone(),
            snapshot: transcript.clone(),
            transcript,
            events,
        },
        other => panic!("expected the session snapshot before the response, got {other:?}"),
    }
}

/// The session's transcript, from a new daemon client's session snapshot.
async fn transcript(daemon: &TestDaemon, session: &SessionId) -> Vec<Entry> {
    subscribe(&daemon.connect().await, session).await.snapshot
}

async fn prompt(client: &Client, session: &SessionId, text: &str) -> Response {
    let request = Request::Prompt {
        session: session.clone(),
        content: vec![ContentBlock::from(text)],
    };
    client.request(request).await.unwrap()
}

async fn focus(client: &Client, session: &SessionId) -> Response {
    let request = Request::Focus {
        sessions: vec![session.clone()],
    };
    client.request(request).await.unwrap()
}

async fn answer(client: &Client, session: &SessionId, request_id: u32, option: &str) -> Response {
    let request = Request::AnswerPermission {
        session: session.clone(),
        request_id,
        option_id: option.to_string().into(),
    };
    client.request(request).await.unwrap()
}

async fn cancel(client: &Client, session: &SessionId) -> Response {
    let request = Request::Cancel {
        session: session.clone(),
    };
    client.request(request).await.unwrap()
}

/// The pending permission requests of a `NeedsPermission` status.
fn requests(summary: SessionSummary) -> Vec<PendingPermission> {
    match summary.status {
        Status::NeedsPermission { requests } => requests,
        other => panic!("expected NeedsPermission, got {other:?}"),
    }
}

fn tool_call_ids(requests: &[PendingPermission]) -> Vec<String> {
    requests
        .iter()
        .map(|pending| pending.request.tool_call.tool_call_id.to_string())
        .collect()
}

fn idle(stop: StopReason) -> Status {
    Status::Idle {
        last_stop: Some(stop),
    }
}

fn user_prompt(text: &str) -> Entry {
    Entry::UserPrompt {
        content: vec![ContentBlock::from(text)],
    }
}

fn message(text: &str) -> Entry {
    Entry::Update {
        update: Box::new(SessionUpdate::AgentMessageChunk(ContentChunk::new(
            text.into(),
        ))),
    }
}

fn update(entry: &Entry) -> &SessionUpdate {
    match entry {
        Entry::Update { update } => update,
        other => panic!("expected an ACP update entry, got {other:?}"),
    }
}

/// The fake server's first update in every session.
fn is_commands_update(entry: &Entry) -> bool {
    matches!(update(entry), SessionUpdate::AvailableCommandsUpdate(_))
}

#[tokio::test]
async fn subscribe_sends_the_transcript_then_live_entries() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let first = daemon.connect().await;
    let session = new_session(&first, "home").await;
    let mut early = subscribe(&first, &session).await;

    assert_eq!(prompt(&first, &session, "hold").await, Response::Done);
    early.wait_for(3).await;
    let second = daemon.connect().await;
    let mut late = subscribe(&second, &session).await;
    assert_eq!(
        late.snapshot[1..],
        [user_prompt("hold"), message("holding")]
    );
    hold.release();

    let transcript = early.wait_for(4).await.to_vec();
    assert!(is_commands_update(&transcript[0]), "{transcript:?}");
    assert_eq!(
        transcript[1..],
        [user_prompt("hold"), message("holding"), message("released")]
    );
    assert_eq!(late.wait_for(4).await, transcript);
}

#[tokio::test]
async fn overlapping_prompt_is_busy() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut subscription = subscribe(&client, &session).await;
    assert_eq!(prompt(&client, &session, "hold").await, Response::Done);
    subscription.wait_for(3).await;

    assert_eq!(prompt(&client, &session, "again").await, Response::Busy);

    let other = daemon.connect().await;
    let snapshot = subscribe(&other, &session).await.snapshot;
    let prompts: Vec<_> = snapshot
        .iter()
        .filter(|entry| matches!(entry, Entry::UserPrompt { .. }))
        .collect();
    assert_eq!(prompts, [&user_prompt("hold")]);
    hold.release();
    assert_eq!(subscription.wait_for(4).await[3], message("released"));
}

#[tokio::test]
async fn updates_right_after_session_new_are_kept() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;

    // The update can arrive before or after the subscription starts; either
    // way it belongs in the transcript.
    let mut subscription = subscribe(&client, &session).await;
    let transcript = subscription.wait_for(1).await;
    assert!(is_commands_update(&transcript[0]), "{transcript:?}");
}

#[tokio::test]
async fn watch_sends_the_snapshot_then_changes() {
    let daemon = TestDaemon::start(&Hold::default());
    let mut early = watch(&daemon).await;
    assert_eq!((early.workspaces.len(), early.sessions.len()), (0, 0));

    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;

    assert_eq!(
        early.next().await,
        Event::WorkspaceAdded {
            workspace: workspace("home")
        }
    );
    let summary = SessionSummary {
        session,
        workspace: "home".to_string(),
        status: Status::Idle { last_stop: None },
        unread: false,
    };
    assert_eq!(
        early.next().await,
        Event::SessionChanged {
            summary: summary.clone()
        }
    );
    let late = watch(&daemon).await;
    assert_eq!(late.workspaces, [workspace("home")]);
    assert_eq!(late.sessions, [summary]);
}

#[tokio::test]
async fn workspaces_survive_a_daemon_restart() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "one").await, Response::Done);
    assert_eq!(add_workspace(&client, "two").await, Response::Done);
    assert_eq!(remove_workspace(&client, "one").await, Response::Done);

    let restarted = daemon.restart(&hold);
    assert_eq!(watch(&restarted).await.workspaces, [workspace("two")]);
}

#[tokio::test]
async fn workspace_requests_reject_bad_input() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    new_session(&client, "home").await;
    let before = watch(&daemon).await;
    let add = |name: &str, path: PathBuf| Request::AddWorkspace {
        name: name.to_string(),
        path,
    };
    let cases = [
        (
            "a relative path",
            add("relative", PathBuf::from("some/dir")),
            "workspace path some/dir is not absolute",
        ),
        (
            "a path that is not a directory",
            add("file", workspace_path().join("Cargo.toml")),
            "is not a directory",
        ),
        (
            "an empty name",
            add("", workspace_path().to_path_buf()),
            "a workspace needs a name",
        ),
        (
            "a name in use",
            add("home", workspace_path().to_path_buf()),
            "workspace home already exists",
        ),
        (
            "a session in an unknown workspace",
            Request::NewSession {
                workspace: "nowhere".to_string(),
            },
            "no workspace nowhere",
        ),
        (
            "removing an unknown workspace",
            Request::RemoveWorkspace {
                name: "nowhere".to_string(),
            },
            "no workspace nowhere",
        ),
    ];
    for (case, request, error) in cases {
        match client.request(request).await.unwrap() {
            Response::Error { message } => assert!(message.contains(error), "{case}: {message}"),
            other => panic!("{case}: unexpected response {other:?}"),
        }
        let after = watch(&daemon).await;
        assert_eq!(after.workspaces, before.workspaces, "{case}");
        assert_eq!(after.sessions, before.sessions, "{case}");
    }
}

#[tokio::test]
async fn removing_a_workspace_removes_its_sessions() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let doomed = new_session(&client, "doomed").await;
    let kept = new_session(&client, "kept").await;
    let subscriber = daemon.connect().await;
    let mut subscription = subscribe(&subscriber, &doomed).await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &doomed, "tool").await, Response::Done);
    watcher.next_change(&doomed).await;
    requests(watcher.next_change(&doomed).await);

    assert_eq!(remove_workspace(&client, "doomed").await, Response::Done);

    assert_eq!(
        watcher.next().await,
        Event::WorkspaceRemoved {
            name: "doomed".to_string()
        }
    );
    let removed = timeout(WAIT, async {
        loop {
            match subscription.events.recv().await {
                Some(Event::SessionRemoved { session }) => return session,
                Some(_) => {}
                None => panic!("the socket connection stays open"),
            }
        }
    })
    .await
    .expect("session_removed arrives");
    assert_eq!(removed, doomed);
    assert!(
        matches!(prompt(&client, &doomed, "hi").await, Response::Error { .. }),
        "a prompt for the removed session answers an error"
    );
    let after = watch(&daemon).await;
    assert_eq!(after.workspaces, [workspace("kept")]);
    assert_eq!(
        after.sessions,
        [SessionSummary {
            session: kept,
            workspace: "kept".to_string(),
            status: Status::Idle { last_stop: None },
            unread: false,
        }]
    );
}

#[tokio::test]
async fn status_follows_the_turn() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut watcher = watch(&daemon).await;

    assert_eq!(prompt(&client, &session, "tool").await, Response::Done);

    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    let requests = requests(watcher.next_change(&session).await);
    let [pending] = &requests[..] else {
        panic!("expected one request: {requests:?}");
    };
    let options: Vec<_> = pending
        .request
        .options
        .iter()
        .map(|option| option.option_id.to_string())
        .collect();
    assert_eq!(options, ["go", "stop"]);
    assert_eq!(
        answer(&client, &session, pending.request_id, "go").await,
        Response::Done
    );
    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    assert_eq!(
        watcher.next_change(&session).await.status,
        idle(StopReason::EndTurn)
    );
    assert_eq!(
        transcript(&daemon, &session).await.last(),
        Some(&message("selected go"))
    );
}

#[tokio::test]
async fn first_answer_wins() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &session, "tool").await, Response::Done);
    watcher.next_change(&session).await;
    let request_id = requests(watcher.next_change(&session).await)[0].request_id;

    let first = daemon.connect().await;
    let second = daemon.connect().await;
    assert_eq!(
        answer(&first, &session, request_id, "go").await,
        Response::Done
    );
    assert_eq!(
        answer(&second, &session, request_id, "stop").await,
        Response::Error {
            message: format!("permission request {request_id} is resolved")
        }
    );

    while watcher.next_change(&session).await.status != idle(StopReason::EndTurn) {}
    assert_eq!(
        transcript(&daemon, &session).await.last(),
        Some(&message("selected go"))
    );
}

#[tokio::test]
async fn several_pending_requests_are_kept_oldest_first() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &session, "tools").await, Response::Done);
    watcher.next_change(&session).await;
    watcher.next_change(&session).await;
    let both = requests(watcher.next_change(&session).await);
    assert_eq!(tool_call_ids(&both), ["tally-1", "tally-2"]);

    assert_eq!(
        answer(&client, &session, both[1].request_id, "stop").await,
        Response::Done
    );
    let left = requests(watcher.next_change(&session).await);
    assert_eq!(tool_call_ids(&left), ["tally-1"]);
    assert_eq!(
        answer(&client, &session, both[0].request_id, "go").await,
        Response::Done
    );

    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    assert_eq!(
        watcher.next_change(&session).await.status,
        idle(StopReason::EndTurn)
    );
    assert_eq!(
        transcript(&daemon, &session).await.last(),
        Some(&message("tally-1: go, tally-2: stop"))
    );
}

#[tokio::test]
async fn cancel_answers_every_pending_request() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &session, "tools").await, Response::Done);
    watcher.next_change(&session).await;
    watcher.next_change(&session).await;
    assert_eq!(requests(watcher.next_change(&session).await).len(), 2);

    assert_eq!(cancel(&client, &session).await, Response::Done);

    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    // The fake server asks for tally-3 after a cancellation, and the daemon
    // answers it without needing permission again.
    assert_eq!(
        watcher.next_change(&session).await.status,
        idle(StopReason::Cancelled)
    );
    assert_eq!(
        transcript(&daemon, &session).await.last(),
        Some(&message(
            "tally-1: cancelled, tally-2: cancelled, tally-3: cancelled"
        ))
    );
}

#[tokio::test]
async fn unread_follows_focus() {
    // (case, whether a daemon client focuses the session, whether it
    // disconnects before the turn ends, whether the session ends unread)
    let cases = [
        ("a daemon client still focuses it", true, false, false),
        ("nobody focuses it", false, false, true),
        ("its daemon client disconnected", true, true, true),
    ];
    for (case, focused, disconnects, unread) in cases {
        let hold = Hold::default();
        let daemon = TestDaemon::start(&hold);
        let client = daemon.connect().await;
        let session = new_session(&client, "home").await;
        let mut watcher = watch(&daemon).await;
        let focuser = daemon.connect().await;
        if focused {
            assert_eq!(focus(&focuser, &session).await, Response::Done, "{case}");
        }
        assert_eq!(prompt(&client, &session, "hold").await, Response::Done);
        watcher.next_change(&session).await;
        if disconnects {
            // The daemon clears the focus before it closes the socket
            // connection, which ends the events.
            let mut events = focuser.events();
            drop(focuser);
            timeout(WAIT, async { while events.recv().await.is_some() {} })
                .await
                .expect("the daemon closes the socket connection");
        }

        hold.release();

        let summary = watcher.next_change(&session).await;
        assert_eq!(summary.status, idle(StopReason::EndTurn), "{case}");
        assert_eq!(summary.unread, unread, "{case}");
        if unread {
            assert_eq!(focus(&client, &session).await, Response::Done, "{case}");
            assert!(!watcher.next_change(&session).await.unread, "{case}");
        }
    }
}

#[tokio::test]
async fn prompt_errors_fail_the_turn() {
    let cases = [
        (
            "reject",
            user_prompt("reject"),
            "the fake server rejects this prompt",
        ),
        ("fail", message("failing"), "the fake server failed"),
    ];
    for (script, before, error) in cases {
        let daemon = TestDaemon::start(&Hold::default());
        let client = daemon.connect().await;
        let session = new_session(&client, "home").await;
        let mut watcher = watch(&daemon).await;

        assert_eq!(prompt(&client, &session, script).await, Response::Done);

        watcher.next_change(&session).await;
        let Status::Failed { message } = watcher.next_change(&session).await.status else {
            panic!("{script}: expected Failed");
        };
        assert!(message.contains(error), "{script}: {message}");
        let transcript = transcript(&daemon, &session).await;
        assert_eq!(
            transcript[transcript.len() - 2..],
            [before, Entry::TurnError { message }],
            "{script}"
        );
        assert_eq!(prompt(&client, &session, "again").await, Response::Done);
        assert_eq!(
            watcher.next_change(&session).await.status,
            Status::Working,
            "{script}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn session_requests_fail_without_a_server() {
    // The paused clock skips ahead to the initialize timeout. The agent side
    // of this channel stays open and never answers.
    let (silent, _agent) = Channel::duplex();
    let cases = [
        (
            "a config error",
            Err(anyhow!("the config file is missing")),
            "the config file is missing",
        ),
        (
            "a server that never answers initialize",
            Ok(silent),
            "initialize failed for the fake server: no answer after 30 seconds",
        ),
    ];
    for (case, server, reason) in cases {
        let daemon = TestDaemon::run(server);
        let client = daemon.connect().await;
        assert_eq!(
            add_workspace(&client, "home").await,
            Response::Done,
            "{case}"
        );

        let request = Request::NewSession {
            workspace: "home".to_string(),
        };
        match client.request(request).await.unwrap() {
            Response::Error { message } => {
                assert!(message.contains(reason), "{case}: {message}");
            }
            other => panic!("{case}: unexpected response {other:?}"),
        }
        assert!(
            matches!(
                client.request(Request::OpenTerminal).await.unwrap(),
                Response::Opened { .. }
            ),
            "{case}: open_terminal works"
        );
    }
}
