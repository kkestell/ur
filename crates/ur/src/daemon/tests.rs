use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, ContentChunk, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionValue, SessionId, SessionUpdate, StopReason,
};
use agent_client_protocol::{Channel, ConnectTo};
use anyhow::anyhow;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::AbortHandle;
use tokio::time::{sleep, timeout};
use ur_client::{
    Client, Entry, Event, PendingPermission, Request, Response, SessionSummary, Status, Workspace,
};
use ur_fake_server::{Hold, SavedHistory, fake_server};

const WAIT: Duration = Duration::from_secs(5);

/// The reason a session fails when `TestDaemon::kill_server()` ends its turn.
const SERVER_EXIT: &str = "the fake server closed the ACP connection";

/// Starts the server for the supervisor, over an in-process channel.
type Launch = Box<dyn Fn() -> Channel + Send>;

/// The daemon running in this process on a socket and a state file in its own
/// directory.
struct TestDaemon {
    socket: PathBuf,
    state_file: PathBuf,
    /// The fake server's saved history, shared by every fake server this
    /// daemon and its restarts start.
    history: SavedHistory,
    /// The running fake server's task.
    server: Arc<Mutex<Option<AbortHandle>>>,
    _dir: TempDir,
}

impl TestDaemon {
    /// Runs the daemon with the fake server.
    fn start(hold: &Hold) -> TestDaemon {
        TestDaemon::start_with(hold, SavedHistory::default())
    }

    fn start_with(hold: &Hold, history: SavedHistory) -> TestDaemon {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        TestDaemon::run_fake(dir, state_file, hold, history)
    }

    /// Runs another daemon with the fake server, on this daemon's state file
    /// and saved history.
    fn restart(&self, hold: &Hold) -> TestDaemon {
        TestDaemon::run_fake(
            tempfile::tempdir().unwrap(),
            self.state_file.clone(),
            hold,
            self.history.clone(),
        )
    }

    fn run_fake(
        dir: TempDir,
        state_file: PathBuf,
        hold: &Hold,
        history: SavedHistory,
    ) -> TestDaemon {
        let server = Arc::new(Mutex::new(None));
        let launch: Launch = Box::new({
            let hold = hold.clone();
            let history = history.clone();
            let server = server.clone();
            move || {
                let (client, agent) = Channel::duplex();
                let task =
                    tokio::spawn(fake_server(hold.clone(), history.clone()).connect_to(agent));
                *server.lock().unwrap() = Some(task.abort_handle());
                client
            }
        });
        TestDaemon::run_on(dir, state_file, history, server, Ok(launch))
    }

    fn run(launch: anyhow::Result<Launch>) -> TestDaemon {
        let dir = tempfile::tempdir().unwrap();
        let state_file = dir.path().join("state.json");
        TestDaemon::run_on(
            dir,
            state_file,
            SavedHistory::default(),
            Arc::default(),
            launch,
        )
    }

    fn run_on(
        dir: TempDir,
        state_file: PathBuf,
        history: SavedHistory,
        server: Arc<Mutex<Option<AbortHandle>>>,
        launch: anyhow::Result<Launch>,
    ) -> TestDaemon {
        let socket = dir.path().join("ur.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let launch = launch.map(|launch| ("the fake server".to_string(), launch));
        tokio::spawn(super::run(listener, state_file.clone(), launch));
        TestDaemon {
            socket,
            state_file,
            history,
            server,
            _dir: dir,
        }
    }

    async fn connect(&self) -> Client {
        Client::connect(&self.socket).await.unwrap()
    }

    /// Aborts the running fake server, which closes its ACP connection the
    /// way a server exit does. The supervisor starts another after
    /// `RESTART_DELAY`.
    fn kill_server(&self) {
        self.server
            .lock()
            .unwrap()
            .take()
            .expect("the fake server is running")
            .abort();
    }
}

/// One daemon client's view of a subscribed session: the session snapshot,
/// then each `entry` and `config_options_changed` event.
struct Subscription {
    session: SessionId,
    snapshot: Vec<Entry>,
    transcript: Vec<Entry>,
    config_options: Vec<SessionConfigOption>,
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
            self.record(event);
        }
        &self.transcript
    }

    /// Waits for a fresh session snapshot, keeping the entries before it, and
    /// returns it.
    async fn next_snapshot(&mut self) -> &[Entry] {
        loop {
            let event = timeout(WAIT, self.events.recv())
                .await
                .unwrap_or_else(|_| panic!("a session snapshot arrives: {:?}", self.transcript))
                .expect("the socket connection stays open");
            match event {
                Event::SessionSnapshot {
                    session,
                    transcript,
                    config_options,
                } if session == self.session => {
                    self.snapshot = transcript.clone();
                    self.transcript = transcript;
                    self.config_options = config_options;
                    return &self.transcript;
                }
                other => self.record(other),
            }
        }
    }

    /// Waits for the next `config_options_changed`, keeping the entries before
    /// it, and returns the config options.
    async fn next_config_options(&mut self) -> &[SessionConfigOption] {
        loop {
            let event = timeout(WAIT, self.events.recv())
                .await
                .unwrap_or_else(|_| panic!("config options arrive: {:?}", self.transcript))
                .expect("the socket connection stays open");
            let changed = matches!(event, Event::ConfigOptionsChanged { .. });
            self.record(event);
            if changed {
                return &self.config_options;
            }
        }
    }

    fn record(&mut self, event: Event) {
        match event {
            Event::Entry { session, entry } if session == self.session => {
                self.transcript.push(entry);
            }
            Event::ConfigOptionsChanged {
                session,
                config_options,
            } if session == self.session => self.config_options = config_options,
            other => panic!("unexpected event {other:?}"),
        }
    }
}

/// A watching daemon client: the watch snapshot, then each change.
struct Watch {
    workspaces: Vec<Workspace>,
    sessions: Vec<SessionSummary>,
    capabilities: Option<Box<AgentCapabilities>>,
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

    /// Waits for the session to stop being `Working`, and returns its status.
    async fn next_turn_end(&mut self, session: &SessionId) -> Status {
        loop {
            let status = self.next_change(session).await.status;
            if status != Status::Working {
                return status;
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
            capabilities,
            terminals: _,
        }) => Watch {
            workspaces,
            sessions,
            capabilities,
            events,
            _client: client,
        },
        other => panic!("expected the watch snapshot before the response, got {other:?}"),
    }
}

/// An absolute path to a directory for the workspace. Each name has its own,
/// so `session/list` for one workspace does not return another's sessions.
fn workspace_path(name: &str) -> PathBuf {
    static ROOT: LazyLock<TempDir> = LazyLock::new(|| tempfile::tempdir().unwrap());
    let path = ROOT.path().join(name);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn workspace(name: &str) -> Workspace {
    Workspace {
        name: name.to_string(),
        path: workspace_path(name),
    }
}

async fn add_workspace(client: &Client, name: &str) -> Response {
    let request = Request::AddWorkspace {
        name: name.to_string(),
        path: workspace_path(name),
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
    create_session(client, workspace).await
}

async fn create_session(client: &Client, workspace: &str) -> SessionId {
    let request = Request::NewSession {
        workspace: workspace.to_string(),
    };
    match client.request(request).await.unwrap() {
        Response::SessionCreated { session } => session,
        other => panic!("unexpected response {other:?}"),
    }
}

/// Creates a session in the workspace once the supervisor has started the
/// server again.
async fn create_session_after_restart(client: &Client, workspace: &str) -> SessionId {
    timeout(WAIT, async {
        loop {
            let request = Request::NewSession {
                workspace: workspace.to_string(),
            };
            match client.request(request).await.unwrap() {
                Response::SessionCreated { session } => return session,
                Response::Error { .. } => sleep(Duration::from_millis(50)).await,
                other => panic!("unexpected response {other:?}"),
            }
        }
    })
    .await
    .expect("the server starts again")
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
            config_options,
        }) if snapshot_session == *session => Subscription {
            session: session.clone(),
            snapshot: transcript.clone(),
            transcript,
            config_options,
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

async fn delete(client: &Client, session: &SessionId) -> Response {
    let request = Request::DeleteSession {
        session: session.clone(),
    };
    client.request(request).await.unwrap()
}

async fn set_pace(client: &Client, session: &SessionId, pace: &str) -> Response {
    let request = Request::SetConfigOption {
        session: session.clone(),
        config_id: "pace".into(),
        value: SessionConfigOptionValue::value_id(pace.to_string()),
    };
    client.request(request).await.unwrap()
}

/// The current value of the fake server's `pace` config option.
fn pace(config_options: &[SessionConfigOption]) -> String {
    match config_options {
        [option] => match &option.kind {
            SessionConfigKind::Select(select) => select.current_value.to_string(),
            other => panic!("expected a select config option, got {other:?}"),
        },
        other => panic!("expected one config option, got {other:?}"),
    }
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

/// The summary of a session that was just created or listed.
fn new_summary(session: &SessionId, workspace: &str) -> SessionSummary {
    SessionSummary {
        session: session.clone(),
        workspace: workspace.to_string(),
        status: Status::Idle { last_stop: None },
        unread: false,
        title: None,
        updated_at: None,
    }
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

/// A replayed user message.
fn user_message(text: &str) -> Entry {
    Entry::Update {
        update: Box::new(SessionUpdate::UserMessageChunk(ContentChunk::new(
            text.into(),
        ))),
    }
}

fn turn_error(message: &str) -> Entry {
    Entry::TurnError {
        message: message.to_string(),
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
    let summary = new_summary(&session, "home");
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
            add(
                "file",
                Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
            ),
            "is not a directory",
        ),
        (
            "an empty name",
            add("", workspace_path("empty")),
            "a workspace needs a name",
        ),
        (
            "a name in use",
            add("home", workspace_path("home")),
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
    assert_eq!(after.sessions, [new_summary(&kept, "kept")]);
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
async fn unread_follows_focus_and_prompts() {
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
            // A prompt clears it, and the turn it starts ends unread again.
            assert_eq!(prompt(&client, &session, "again").await, Response::Done);
            let summary = watcher.next_change(&session).await;
            assert_eq!(summary.status, Status::Working, "{case}");
            assert!(!summary.unread, "{case}");
            assert!(watcher.next_change(&session).await.unread, "{case}");

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
    // The paused clock skips ahead to the initialize timeout, and restarts
    // time out the same way. The agent side of each channel stays open and
    // never answers.
    let agents = Arc::new(Mutex::new(Vec::new()));
    let silent: Launch = Box::new(move || {
        let (client, agent) = Channel::duplex();
        agents.lock().unwrap().push(agent);
        client
    });
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
    for (case, launch, reason) in cases {
        let daemon = TestDaemon::run(launch);
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
                client
                    .request(Request::OpenTerminal {
                        workspace: "home".to_string()
                    })
                    .await
                    .unwrap(),
                Response::Opened { .. }
            ),
            "{case}: open_terminal works"
        );
    }
}

#[tokio::test]
async fn saved_sessions_are_listed_at_startup() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    let first = new_session(&client, "home").await;
    let second = create_session(&client, "home").await;
    let third = new_session(&client, "work").await;

    let restarted = daemon.restart(&hold);

    // Two sessions in one workspace take two pages of `session/list`.
    assert_eq!(
        watch(&restarted).await.sessions,
        [
            new_summary(&first, "home"),
            new_summary(&second, "home"),
            new_summary(&third, "work"),
        ]
    );
}

#[tokio::test]
async fn subscribing_loads_a_saved_session() {
    // The hold is never released, as if the daemon were killed during the
    // turn.
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut subscription = subscribe(&client, &session).await;
    assert_eq!(prompt(&client, &session, "hold").await, Response::Done);
    subscription.wait_for(3).await;

    let restarted = daemon.restart(&hold);

    let transcript = transcript(&restarted, &session).await;
    assert!(is_commands_update(&transcript[0]), "{transcript:?}");
    assert_eq!(transcript[1..], [user_message("hold"), message("holding")]);
}

#[tokio::test]
async fn prompting_loads_a_saved_session_first() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &session, "hi").await, Response::Done);
    assert_eq!(
        watcher.next_turn_end(&session).await,
        idle(StopReason::EndTurn)
    );

    let restarted = daemon.restart(&hold);
    let client = restarted.connect().await;
    let mut watcher = watch(&restarted).await;
    assert_eq!(prompt(&client, &session, "again").await, Response::Done);

    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    assert_eq!(
        watcher.next_turn_end(&session).await,
        idle(StopReason::EndTurn)
    );
    let transcript = transcript(&restarted, &session).await;
    assert!(is_commands_update(&transcript[0]), "{transcript:?}");
    assert_eq!(
        transcript[1..],
        [
            user_message("hi"),
            message("you "),
            message("said: "),
            message("hi"),
            user_prompt("again"),
            message("you "),
            message("said: "),
            message("again"),
        ]
    );
}

#[tokio::test]
async fn a_failed_load_is_retried_by_the_next_prompt() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(
        prompt(&client, &session, "unloadable").await,
        Response::Done
    );
    watcher.next_turn_end(&session).await;

    let restarted = daemon.restart(&hold);
    let client = restarted.connect().await;
    let mut watcher = watch(&restarted).await;
    let subscription = subscribe(&client, &session).await;

    assert_eq!(subscription.snapshot, []);
    let error = "the fake server cannot load this session";
    let failed = |status: &Status| {
        matches!(status, Status::Failed { message }
            if message.starts_with("session/load failed:") && message.contains(error))
    };
    let status = watcher.next_change(&session).await.status;
    assert!(failed(&status), "{status:?}");
    assert_eq!(
        watch(&restarted).await.sessions.len(),
        1,
        "the session stays in watch"
    );
    assert_eq!(prompt(&client, &session, "again").await, Response::Done);
    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    let status = watcher.next_change(&session).await.status;
    assert!(failed(&status), "{status:?}");
}

#[tokio::test]
async fn session_titles_come_from_updates_and_the_list() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut watcher = watch(&daemon).await;

    assert_eq!(prompt(&client, &session, "title").await, Response::Done);

    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    assert_eq!(
        watcher.next_change(&session).await.title.as_deref(),
        Some("tallies")
    );
    let restarted = daemon.restart(&hold);
    let sessions = watch(&restarted).await.sessions;
    assert_eq!(sessions[0].title.as_deref(), Some("tallies"));
}

#[tokio::test]
async fn adding_a_workspace_lists_its_saved_sessions() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    assert_eq!(remove_workspace(&client, "home").await, Response::Done);
    let mut watcher = watch(&daemon).await;

    assert_eq!(add_workspace(&client, "home").await, Response::Done);

    assert_eq!(
        watcher.next().await,
        Event::WorkspaceAdded {
            workspace: workspace("home")
        }
    );
    assert_eq!(
        watcher.next_change(&session).await,
        new_summary(&session, "home")
    );
}

#[tokio::test]
async fn server_exit_fails_the_running_turn() {
    // (script, whether it waits for a permission answer)
    let cases = [("hold", false), ("tool", true)];
    for (script, asks) in cases {
        let daemon = TestDaemon::start(&Hold::default());
        let client = daemon.connect().await;
        let session = new_session(&client, "home").await;
        let mut watcher = watch(&daemon).await;
        assert_eq!(prompt(&client, &session, script).await, Response::Done);
        assert_eq!(
            watcher.next_change(&session).await.status,
            Status::Working,
            "{script}"
        );
        let request_id = if asks {
            Some(requests(watcher.next_change(&session).await)[0].request_id)
        } else {
            None
        };

        daemon.kill_server();

        let summary = watcher.next_change(&session).await;
        assert_eq!(
            summary.status,
            Status::Failed {
                message: SERVER_EXIT.to_string()
            },
            "{script}"
        );
        assert!(summary.unread, "{script}");
        let transcript = transcript(&daemon, &session).await;
        let errors = transcript
            .iter()
            .filter(|entry| matches!(entry, Entry::TurnError { .. }))
            .count();
        assert_eq!(errors, 1, "{script}: {transcript:?}");
        assert_eq!(
            transcript.last(),
            Some(&turn_error(SERVER_EXIT)),
            "{script}"
        );
        if let Some(request_id) = request_id {
            assert_eq!(
                answer(&client, &session, request_id, "go").await,
                Response::Error {
                    message: format!("permission request {request_id} is resolved")
                },
                "{script}"
            );
        }
    }
}

#[tokio::test]
async fn subscribed_sessions_reload_after_the_server_restarts() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut subscription = subscribe(&client, &session).await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &session, "hold").await, Response::Done);
    subscription.wait_for(3).await;

    daemon.kill_server();

    assert_eq!(subscription.wait_for(4).await[3], turn_error(SERVER_EXIT));
    let snapshot = subscription.next_snapshot().await.to_vec();
    assert!(is_commands_update(&snapshot[0]), "{snapshot:?}");
    assert_eq!(
        snapshot[1..],
        [
            user_message("hold"),
            message("holding"),
            turn_error(SERVER_EXIT)
        ]
    );
    let failed = Status::Failed {
        message: SERVER_EXIT.to_string(),
    };
    assert_eq!(watcher.next_turn_end(&session).await, failed);
    assert_eq!(watch(&daemon).await.sessions[0].status, failed);
    assert_eq!(prompt(&client, &session, "again").await, Response::Done);
    assert_eq!(
        watcher.next_turn_end(&session).await,
        idle(StopReason::EndTurn)
    );
}

#[tokio::test]
async fn without_history_capabilities_old_sessions_cannot_be_prompted() {
    let daemon = TestDaemon::start_with(&Hold::default(), SavedHistory::unadvertised());
    let client = daemon.connect().await;
    let old = new_session(&client, "home").await;
    let before = subscribe(&client, &old).await.wait_for(1).await.to_vec();

    daemon.kill_server();

    let new = create_session_after_restart(&client, "home").await;
    assert_eq!(
        prompt(&client, &old, "hi").await,
        Response::Error {
            message: format!("the server cannot load session {old}")
        }
    );
    assert_eq!(transcript(&daemon, &old).await, before);
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &new, "hi").await, Response::Done);
    assert_eq!(watcher.next_turn_end(&new).await, idle(StopReason::EndTurn));
}

#[tokio::test]
async fn deleting_a_session_removes_it() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let kept = create_session(&client, "home").await;
    let mut subscription = subscribe(&client, &session).await;
    subscription.wait_for(1).await;
    let mut watcher = watch(&daemon).await;

    assert_eq!(delete(&client, &session).await, Response::Done);

    assert_eq!(
        watcher.next().await,
        Event::SessionDeleted {
            session: session.clone()
        }
    );
    let removed = timeout(WAIT, subscription.events.recv())
        .await
        .expect("session_removed arrives");
    assert_eq!(
        removed,
        Some(Event::SessionRemoved {
            session: session.clone()
        })
    );
    let restarted = daemon.restart(&hold);
    assert_eq!(
        watch(&restarted).await.sessions,
        [new_summary(&kept, "home")]
    );

    let daemon = TestDaemon::start_with(&Hold::default(), SavedHistory::unadvertised());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    assert_eq!(
        delete(&client, &session).await,
        Response::Error {
            message: "the server cannot delete sessions".to_string()
        }
    );
}

#[tokio::test]
async fn deleting_a_running_session_cancels_its_turn_first() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &session, "tool").await, Response::Done);
    watcher.next_change(&session).await;
    requests(watcher.next_change(&session).await);

    // Polling the first delete once sends it, so it reaches the daemon before
    // the second.
    let mut first = Box::pin(delete(&client, &session));
    assert!(futures::poll!(&mut first).is_pending());
    assert_eq!(delete(&client, &session).await, Response::Busy);

    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    assert_eq!(
        watcher.next_change(&session).await.status,
        idle(StopReason::Cancelled)
    );
    assert_eq!(
        watcher.next().await,
        Event::SessionDeleted {
            session: session.clone()
        }
    );
    assert_eq!(first.await, Response::Done);
}

#[tokio::test]
async fn set_config_option_changes_the_config_options() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut subscription = subscribe(&client, &session).await;
    subscription.wait_for(1).await;
    assert_eq!(pace(&subscription.config_options), "steady");

    assert_eq!(set_pace(&client, &session, "brisk").await, Response::Done);
    assert_eq!(pace(subscription.next_config_options().await), "brisk");

    match set_pace(&client, &session, "sluggish").await {
        Response::Error { message } => assert!(
            message.starts_with("session/set_config_option failed:"),
            "{message}"
        ),
        other => panic!("unexpected response {other:?}"),
    }
    // The daemon queues any event before the response.
    assert!(subscription.events.try_recv().is_err());
}

#[tokio::test]
async fn config_option_updates_change_the_config_options() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    let mut subscription = subscribe(&client, &session).await;

    assert_eq!(prompt(&client, &session, "pace").await, Response::Done);

    assert_eq!(pace(subscription.next_config_options().await), "brisk");
}

#[tokio::test]
async fn a_load_restores_the_config_options() {
    let hold = Hold::default();
    let daemon = TestDaemon::start(&hold);
    let client = daemon.connect().await;
    let session = new_session(&client, "home").await;
    assert_eq!(set_pace(&client, &session, "brisk").await, Response::Done);

    let restarted = daemon.restart(&hold);

    let subscription = subscribe(&restarted.connect().await, &session).await;
    assert_eq!(pace(&subscription.config_options), "brisk");
}

#[tokio::test]
async fn watch_reports_the_capabilities() {
    let daemon = TestDaemon::start(&Hold::default());
    let mut watcher = watch(&daemon).await;
    let advertised = |capabilities: &AgentCapabilities| {
        capabilities.session_capabilities.delete.is_some() && capabilities.prompt_capabilities.image
    };
    let capabilities = watcher.capabilities.take().expect("the server started");
    assert!(advertised(&capabilities), "{capabilities:?}");

    daemon.kill_server();

    let capabilities = loop {
        if let Event::CapabilitiesChanged { capabilities } = watcher.next().await {
            break capabilities;
        }
    };
    assert!(advertised(&capabilities), "{capabilities:?}");
}
