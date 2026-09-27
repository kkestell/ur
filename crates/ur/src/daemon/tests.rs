use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, ContentChunk, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionValue, SessionUpdate, StopReason,
};
use agent_client_protocol::{Channel, ConnectTo};
use anyhow::anyhow;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::AbortHandle;
use tokio::time::{sleep, timeout};
use ur_client::{
    Client, Entry, Event, PendingPermission, Request, Response, ServerIcon, SessionKey,
    SessionSummary, Status, Workspace, WorkspaceColor,
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
    control: Option<Arc<super::ServerControl>>,
    config_file: Option<PathBuf>,
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
        let launch = fake_launch(hold, &history, &server);
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
            control: None,
            config_file: None,
            _dir: dir,
        }
    }

    fn managed() -> TestDaemon {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("ur.sock");
        let state_file = dir.path().join("state.json");
        let config_file = dir.path().join("config.json");
        let listener = UnixListener::bind(&socket).unwrap();
        let state = Arc::new(Mutex::new(super::state::State::new(Vec::new())));
        let control = Arc::new(super::ServerControl {
            state: state.clone(),
            tasks: Mutex::new(Default::default()),
            configuration: Mutex::new(super::Config {
                servers: Vec::new(),
            }),
            config_file: config_file.clone(),
            launches: Mutex::new(Default::default()),
        });
        let terminals = Arc::new(super::terminal::Terminals::new(state.clone()));
        tokio::spawn(super::server::serve(
            listener,
            terminals,
            state,
            state_file.clone(),
            Some(control.clone()),
        ));
        TestDaemon {
            socket,
            state_file,
            history: SavedHistory::default(),
            server: Arc::default(),
            control: Some(control),
            config_file: Some(config_file),
            _dir: dir,
        }
    }

    fn launch(&self, command: &str, history: &SavedHistory) {
        let history = history.clone();
        self.control
            .as_ref()
            .unwrap()
            .launches
            .lock()
            .unwrap()
            .insert(
                command.into(),
                Arc::new(move || {
                    let (client, agent) = Channel::duplex();
                    tokio::spawn(fake_server(Hold::default(), history.clone()).connect_to(agent));
                    client
                }),
            );
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

/// Starts a fake server with `hold` and `history`, and keeps its task in
/// `server`.
fn fake_launch(
    hold: &Hold,
    history: &SavedHistory,
    server: &Arc<Mutex<Option<AbortHandle>>>,
) -> Launch {
    let hold = hold.clone();
    let history = history.clone();
    let server = server.clone();
    Box::new(move || {
        let (client, agent) = Channel::duplex();
        let task = tokio::spawn(fake_server(hold.clone(), history.clone()).connect_to(agent));
        *server.lock().unwrap() = Some(task.abort_handle());
        client
    })
}

/// Two independent ACP connections whose fake servers both issue fake-1.
fn two_servers() -> (TestDaemon, Arc<Mutex<Option<AbortHandle>>>) {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("ur.sock");
    let state_file = dir.path().join("state.json");
    let listener = UnixListener::bind(&socket).unwrap();
    let first = Arc::new(Mutex::new(None));
    let second = Arc::new(Mutex::new(None));
    let first_history = SavedHistory::default();
    let second_history = SavedHistory::default();
    let first_launch = fake_launch(&Hold::default(), &first_history, &first);
    let second_launch = fake_launch(&Hold::default(), &second_history, &second);
    let state_file_for_serve = state_file.clone();
    tokio::spawn(async move {
        let state = Arc::new(Mutex::new(super::state::State::new(Vec::new())));
        for (id, name) in [("test", "Test"), ("beta", "Beta")] {
            state
                .lock()
                .unwrap()
                .configure_server(&super::ServerConfig {
                    id: id.into(),
                    name: name.into(),
                    icon: ServerIcon::Claude,
                    command: name.into(),
                    args: Vec::new(),
                });
        }
        let (_, first_ready) =
            super::acp::spawn_supervisor("test".into(), "Test".into(), first_launch, state.clone());
        let (_, second_ready) = super::acp::spawn_supervisor(
            "beta".into(),
            "Beta".into(),
            second_launch,
            state.clone(),
        );
        tokio::join!(first_ready.notified(), second_ready.notified());
        let terminals = Arc::new(super::terminal::Terminals::new(state.clone()));
        super::server::serve(listener, terminals, state, state_file_for_serve, None)
            .await
            .unwrap();
    });
    (
        TestDaemon {
            socket,
            state_file,
            history: first_history,
            server: first,
            control: None,
            config_file: None,
            _dir: dir,
        },
        second,
    )
}

/// One daemon client's view of a subscribed session: the session snapshot,
/// then each `entry` and `config_options_changed` event.
struct Subscription {
    session: SessionKey,
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
    async fn next_change(&mut self, session: &SessionKey) -> SessionSummary {
        loop {
            if let Event::SessionChanged { summary } = self.next().await
                && summary.session == *session
            {
                return summary;
            }
        }
    }

    /// Waits for the session to stop being `Working`, and returns its status.
    async fn next_turn_end(&mut self, session: &SessionKey) -> Status {
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
            servers,
            terminals: _,
            config_error: _,
        }) => Watch {
            workspaces,
            sessions,
            capabilities: servers
                .first()
                .and_then(|server| server.capabilities.clone()),
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
        color: WorkspaceColor::Blue,
    }
}

async fn add_workspace(client: &Client, name: &str) -> Response {
    let request = Request::AddWorkspace {
        name: name.to_string(),
        path: workspace_path(name),
        color: WorkspaceColor::Blue,
    };
    client.request(request).await.unwrap()
}

async fn set_workspace_color(client: &Client, name: &str, color: WorkspaceColor) -> Response {
    let request = Request::SetWorkspaceColor {
        name: name.to_string(),
        color,
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
async fn new_session(client: &Client, workspace: &str) -> SessionKey {
    assert_eq!(add_workspace(client, workspace).await, Response::Done);
    create_session(client, workspace).await
}

async fn create_session(client: &Client, workspace: &str) -> SessionKey {
    create_session_on(client, "test", workspace).await
}

async fn create_session_on(client: &Client, server: &str, workspace: &str) -> SessionKey {
    let request = Request::NewSession {
        server: server.into(),
        workspace: workspace.to_string(),
    };
    match client.request(request).await.unwrap() {
        Response::SessionCreated { session } => session,
        other => panic!("unexpected response {other:?}"),
    }
}

/// Creates a session in the workspace once the supervisor has started the
/// server again.
async fn create_session_after_restart(client: &Client, workspace: &str) -> SessionKey {
    timeout(WAIT, async {
        loop {
            let request = Request::NewSession {
                server: "test".into(),
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

async fn subscribe(client: &Client, session: &SessionKey) -> Subscription {
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
async fn transcript(daemon: &TestDaemon, session: &SessionKey) -> Vec<Entry> {
    subscribe(&daemon.connect().await, session).await.snapshot
}

async fn prompt(client: &Client, session: &SessionKey, text: &str) -> Response {
    let request = Request::Prompt {
        session: session.clone(),
        content: vec![ContentBlock::from(text)],
    };
    client.request(request).await.unwrap()
}

async fn focus(client: &Client, session: &SessionKey) -> Response {
    let request = Request::Focus {
        sessions: vec![session.clone()],
    };
    client.request(request).await.unwrap()
}

async fn answer(client: &Client, session: &SessionKey, request_id: u32, option: &str) -> Response {
    let request = Request::AnswerPermission {
        session: session.clone(),
        request_id,
        option_id: option.to_string().into(),
    };
    client.request(request).await.unwrap()
}

async fn cancel(client: &Client, session: &SessionKey) -> Response {
    let request = Request::Cancel {
        session: session.clone(),
    };
    client.request(request).await.unwrap()
}

async fn delete(client: &Client, session: &SessionKey) -> Response {
    let request = Request::DeleteSession {
        session: session.clone(),
    };
    client.request(request).await.unwrap()
}

async fn add_server(client: &Client, name: &str, command: &str) -> String {
    let response = client
        .request(Request::AddServer {
            name: name.into(),
            icon: ServerIcon::Claude,
            command: command.into(),
            args: Vec::new(),
        })
        .await
        .unwrap();
    match response {
        Response::ServerAdded { server } => server,
        other => panic!("unexpected add response {other:?}"),
    }
}

async fn update_server(client: &Client, server: &str, name: &str, command: &str) -> Response {
    client
        .request(Request::UpdateServer {
            server: server.into(),
            name: name.into(),
            icon: ServerIcon::Claude,
            command: command.into(),
            args: Vec::new(),
        })
        .await
        .unwrap()
}

async fn remove_server(client: &Client, server: &str) -> Response {
    client
        .request(Request::RemoveServer {
            server: server.into(),
        })
        .await
        .unwrap()
}

async fn ready_server(daemon: &TestDaemon, id: &str) {
    timeout(WAIT, async {
        loop {
            let client = daemon.connect().await;
            let mut events = client.events();
            assert_eq!(
                client.request(Request::Watch).await.unwrap(),
                Response::Done
            );
            if let Some(Event::WatchSnapshot { servers, .. }) = events.recv().await
                && servers
                    .iter()
                    .any(|server| server.id == id && server.connected)
            {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("server connects");
}

async fn set_pace(client: &Client, session: &SessionKey, pace: &str) -> Response {
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
fn new_summary(session: &SessionKey, workspace: &str) -> SessionSummary {
    SessionSummary {
        server: "test".into(),
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
        Event::WorkspaceChanged {
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
    assert_eq!(
        set_workspace_color(&client, "two", WorkspaceColor::Peach).await,
        Response::Done
    );

    let restarted = daemon.restart(&hold);
    let two = Workspace {
        color: WorkspaceColor::Peach,
        ..workspace("two")
    };
    assert_eq!(watch(&restarted).await.workspaces, [two]);
}

#[tokio::test]
async fn setting_a_workspace_color_updates_watchers() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let mut watcher = watch(&daemon).await;

    assert_eq!(
        set_workspace_color(&client, "home", WorkspaceColor::Green).await,
        Response::Done
    );

    assert_eq!(
        watcher.next().await,
        Event::WorkspaceChanged {
            workspace: Workspace {
                color: WorkspaceColor::Green,
                ..workspace("home")
            }
        }
    );
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
        color: WorkspaceColor::Blue,
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
                server: "test".into(),
                workspace: "nowhere".to_string(),
            },
            "no workspace nowhere",
        ),
        (
            "a color for an unknown workspace",
            Request::SetWorkspaceColor {
                name: "nowhere".to_string(),
                color: WorkspaceColor::Red,
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
            "no server test",
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
            server: "test".into(),
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
        Event::WorkspaceChanged {
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
        if let Event::ServersChanged { servers, .. } = watcher.next().await
            && let Some(capabilities) = servers
                .first()
                .and_then(|server| server.capabilities.clone())
        {
            break capabilities;
        }
    };
    assert!(advertised(&capabilities), "{capabilities:?}");
}

#[tokio::test]
async fn overlapping_acp_ids_have_independent_transcripts() {
    let (daemon, _) = two_servers();
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let first = create_session_on(&client, "test", "home").await;
    let second = create_session_on(&client, "beta", "home").await;
    assert_ne!(first, second);
    assert_eq!(
        serde_json::from_str::<(String, String)>(&first.0)
            .unwrap()
            .1,
        "fake-1"
    );
    assert_eq!(
        serde_json::from_str::<(String, String)>(&second.0)
            .unwrap()
            .1,
        "fake-1"
    );
    let first_client = daemon.connect().await;
    let second_client = daemon.connect().await;
    let mut first_sub = subscribe(&first_client, &first).await;
    let mut second_sub = subscribe(&second_client, &second).await;
    assert_eq!(prompt(&client, &first, "alpha").await, Response::Done);
    assert_eq!(prompt(&client, &second, "beta").await, Response::Done);
    first_sub.wait_for(2).await;
    second_sub.wait_for(2).await;
    assert_ne!(first_sub.transcript, second_sub.transcript);
    assert!(format!("{:?}", first_sub.transcript).contains("alpha"));
    assert!(format!("{:?}", second_sub.transcript).contains("beta"));
}

#[tokio::test]
async fn config_options_route_to_the_owning_server() {
    let (daemon, _) = two_servers();
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let first = create_session_on(&client, "test", "home").await;
    let second = create_session_on(&client, "beta", "home").await;
    assert_eq!(set_pace(&client, &second, "brisk").await, Response::Done);
    assert_eq!(
        pace(&subscribe(&client, &first).await.config_options),
        "steady"
    );
    assert_eq!(
        pace(&subscribe(&client, &second).await.config_options),
        "brisk"
    );
}

#[tokio::test]
async fn deleting_one_servers_session_keeps_the_other() {
    let (daemon, _) = two_servers();
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let first = create_session_on(&client, "test", "home").await;
    let second = create_session_on(&client, "beta", "home").await;
    assert_eq!(delete(&client, &second).await, Response::Done);
    let snapshot = watch(&daemon).await;
    assert!(
        snapshot
            .sessions
            .iter()
            .any(|session| session.session == first)
    );
    assert!(
        !snapshot
            .sessions
            .iter()
            .any(|session| session.session == second)
    );
}

#[tokio::test]
async fn restarting_one_server_keeps_the_others_connection() {
    let (daemon, beta_task) = two_servers();
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let first = create_session_on(&client, "test", "home").await;
    let second = create_session_on(&client, "beta", "home").await;
    beta_task.lock().unwrap().take().unwrap().abort();
    assert_eq!(
        prompt(&client, &first, "still connected").await,
        Response::Done
    );
    timeout(WAIT, async {
        loop {
            match prompt(&client, &second, "after restart").await {
                Response::Done => break,
                Response::Error { .. } | Response::Busy => sleep(Duration::from_millis(50)).await,
                other => panic!("unexpected response {other:?}"),
            }
        }
    })
    .await
    .expect("Beta reconnects");
}

#[tokio::test]
async fn focus_and_unread_are_independent_for_overlapping_ids() {
    let (daemon, _) = two_servers();
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let first = create_session_on(&client, "test", "home").await;
    let second = create_session_on(&client, "beta", "home").await;
    assert_eq!(focus(&client, &first).await, Response::Done);
    assert_eq!(prompt(&client, &first, "alpha").await, Response::Done);
    assert_eq!(prompt(&client, &second, "beta").await, Response::Done);
    let summaries = timeout(WAIT, async {
        loop {
            let summaries = watch(&daemon).await.sessions;
            if summaries.len() == 2
                && summaries
                    .iter()
                    .all(|summary| matches!(summary.status, Status::Idle { last_stop: Some(_) }))
            {
                break summaries;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !summaries
            .iter()
            .find(|summary| summary.session == first)
            .unwrap()
            .unread
    );
    assert!(
        summaries
            .iter()
            .find(|summary| summary.session == second)
            .unwrap()
            .unread
    );
}

#[tokio::test]
async fn permission_answers_route_to_the_owning_server() {
    let (daemon, _) = two_servers();
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let first = create_session_on(&client, "test", "home").await;
    let second = create_session_on(&client, "beta", "home").await;
    assert_eq!(prompt(&client, &first, "tool").await, Response::Done);
    assert_eq!(prompt(&client, &second, "tool").await, Response::Done);
    let summaries = timeout(WAIT, async {
        loop {
            let summaries = watch(&daemon).await.sessions;
            if summaries.len() == 2
                && summaries
                    .iter()
                    .all(|summary| matches!(summary.status, Status::NeedsPermission { .. }))
            {
                break summaries;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let beta = summaries
        .into_iter()
        .find(|summary| summary.session == second)
        .unwrap();
    let request_id = requests(beta)[0].request_id;
    assert_eq!(
        answer(&client, &second, request_id, "go").await,
        Response::Done
    );
    let snapshot = watch(&daemon).await;
    assert!(matches!(
        snapshot
            .sessions
            .iter()
            .find(|summary| summary.session == first)
            .unwrap()
            .status,
        Status::NeedsPermission { .. }
    ));
    assert_eq!(cancel(&client, &first).await, Response::Done);
}

#[tokio::test]
async fn removing_a_workspace_removes_sessions_from_both_servers() {
    let (daemon, _) = two_servers();
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    create_session_on(&client, "test", "home").await;
    create_session_on(&client, "beta", "home").await;
    assert_eq!(remove_workspace(&client, "home").await, Response::Done);
    assert!(watch(&daemon).await.sessions.is_empty());
}

#[tokio::test]
async fn adding_a_workspace_lists_saved_sessions_from_both_servers() {
    let (daemon, _) = two_servers();
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let first = create_session_on(&client, "test", "home").await;
    let second = create_session_on(&client, "beta", "home").await;
    assert_eq!(remove_workspace(&client, "home").await, Response::Done);
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let sessions = timeout(WAIT, async {
        loop {
            let sessions = watch(&daemon).await.sessions;
            if sessions.len() == 2 {
                break sessions;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(sessions.iter().any(|summary| summary.session == first));
    assert!(sessions.iter().any(|summary| summary.session == second));
}

#[tokio::test]
async fn renaming_a_server_and_changing_its_icon_keeps_its_session() {
    let daemon = TestDaemon::managed();
    daemon.launch("/fake/alpha", &SavedHistory::default());
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let mut watcher = watch(&daemon).await;

    let id = add_server(&client, "Alpha", "/fake/alpha").await;
    ready_server(&daemon, &id).await;
    let session = create_session_on(&client, &id, "home").await;
    let renamed = client
        .request(Request::UpdateServer {
            server: id.clone(),
            name: "Renamed".into(),
            icon: ServerIcon::Codex,
            command: "/fake/alpha".into(),
            args: Vec::new(),
        })
        .await
        .unwrap();
    assert_eq!(renamed, Response::Done);

    assert_eq!(
        prompt(&client, &session, "still here").await,
        Response::Done
    );
    assert_eq!(watch(&daemon).await.sessions.len(), 1);
    let mut saw_change = false;
    while let Ok(event) = watcher.events.try_recv() {
        if let Event::ServersChanged { servers, .. } = event {
            saw_change |= servers.iter().any(|server| {
                server.id == id
                    && server.name == "Renamed"
                    && server.icon == ServerIcon::Codex
                    && server.connected
            });
        }
    }
    assert!(saw_change);
}

#[tokio::test]
async fn removing_a_server_fails_its_turn_and_removes_only_its_session() {
    let daemon = TestDaemon::managed();
    daemon.launch("/fake/alpha", &SavedHistory::default());
    daemon.launch("/fake/beta", &SavedHistory::default());
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let alpha = add_server(&client, "Alpha", "/fake/alpha").await;
    let beta = add_server(&client, "Beta", "/fake/beta").await;
    ready_server(&daemon, &alpha).await;
    ready_server(&daemon, &beta).await;
    let removed = create_session_on(&client, &alpha, "home").await;
    let kept = create_session_on(&client, &beta, "home").await;
    let subscriber_client = daemon.connect().await;
    let mut subscription = subscribe(&subscriber_client, &removed).await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &removed, "hold").await, Response::Done);
    subscription.wait_for(3).await;

    assert_eq!(remove_server(&client, &alpha).await, Response::Done);

    assert_eq!(
        watcher.next_turn_end(&removed).await,
        Status::Failed {
            message: "the server was removed".into()
        }
    );
    loop {
        if watcher.next().await
            == (Event::SessionDeleted {
                session: removed.clone(),
            })
        {
            break;
        }
    }
    assert_eq!(
        subscription.wait_for(4).await[3],
        turn_error("the server was removed")
    );
    assert_eq!(
        subscription.events.recv().await,
        Some(Event::SessionRemoved {
            session: removed.clone()
        })
    );
    assert_eq!(
        watch(&daemon)
            .await
            .sessions
            .iter()
            .map(|s| &s.session)
            .collect::<Vec<_>>(),
        vec![&kept]
    );
    assert_eq!(prompt(&client, &kept, "works").await, Response::Done);
}

#[tokio::test]
async fn removing_a_server_resolves_pending_permission() {
    let daemon = TestDaemon::managed();
    daemon.launch("/fake/alpha", &SavedHistory::default());
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let id = add_server(&client, "Alpha", "/fake/alpha").await;
    ready_server(&daemon, &id).await;
    let session = create_session_on(&client, &id, "home").await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &session, "tool").await, Response::Done);
    assert_eq!(watcher.next_change(&session).await.status, Status::Working);
    let request_id = requests(watcher.next_change(&session).await)[0].request_id;
    assert_eq!(remove_server(&client, &id).await, Response::Done);
    assert_eq!(
        answer(&client, &session, request_id, "go").await,
        Response::Error {
            message: format!("no session {session}")
        }
    );
    assert!(watch(&daemon).await.sessions.is_empty());
}

#[tokio::test]
async fn removing_a_server_answers_waiting_and_in_flight_deletes() {
    for in_flight in [false, true] {
        let daemon = TestDaemon::managed();
        let history = SavedHistory::default();
        let pending = history.hold_delete();
        if in_flight {
            pending.pause();
        }
        daemon.launch("/fake/alpha", &history);
        let client = daemon.connect().await;
        assert_eq!(add_workspace(&client, "home").await, Response::Done);
        let id = add_server(&client, "Alpha", "/fake/alpha").await;
        ready_server(&daemon, &id).await;
        let session = create_session_on(&client, &id, "home").await;
        let mut watcher = watch(&daemon).await;
        if !in_flight {
            assert_eq!(prompt(&client, &session, "hold").await, Response::Done);
            assert_eq!(watcher.next_change(&session).await.status, Status::Working);
        }
        let mut deleting = Box::pin(delete(&client, &session));
        assert!(futures::poll!(&mut deleting).is_pending());
        if in_flight {
            timeout(WAIT, pending.wait_started()).await.unwrap();
        }
        assert_eq!(remove_server(&client, &id).await, Response::Done);
        assert!(
            matches!(
                timeout(WAIT, deleting).await.unwrap(),
                Response::Error { .. }
            ),
            "in_flight={in_flight}"
        );
        assert!(watch(&daemon).await.sessions.is_empty());
    }
}

#[tokio::test]
async fn launch_edit_reloads_from_the_new_connection() {
    let daemon = TestDaemon::managed();
    let history = SavedHistory::default();
    daemon.launch("/fake/old", &history);
    daemon.launch("/fake/new", &history);
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let id = add_server(&client, "Alpha", "/fake/old").await;
    ready_server(&daemon, &id).await;
    let session = create_session_on(&client, &id, "home").await;
    let mut watcher = watch(&daemon).await;
    assert_eq!(prompt(&client, &session, "before").await, Response::Done);
    assert_eq!(
        watcher.next_turn_end(&session).await,
        idle(StopReason::EndTurn)
    );
    let subscriber_client = daemon.connect().await;
    let mut subscriber = subscribe(&subscriber_client, &session).await;
    assert_eq!(
        update_server(&client, &id, "Alpha", "/fake/new").await,
        Response::Done
    );
    ready_server(&daemon, &id).await;
    let replay = subscriber.next_snapshot().await.to_vec();
    assert!(replay.contains(&user_message("before")), "{replay:?}");
    assert_eq!(prompt(&client, &session, "after").await, Response::Done);
    assert_eq!(
        watcher.next_turn_end(&session).await,
        idle(StopReason::EndTurn)
    );
    let transcript = transcript(&daemon, &session).await;
    assert!(transcript.contains(&user_message("before")));
    assert!(transcript.contains(&user_prompt("after")));
}

#[tokio::test]
async fn launch_edit_settles_a_load_and_ignores_the_retired_result() {
    let daemon = TestDaemon::managed();
    let history = SavedHistory::default();
    let stalled = history.with_new_holds();
    let pending = stalled.hold_load();
    pending.pause();
    daemon.launch("/fake/old", &history);
    daemon.launch("/fake/stalled", &stalled);
    daemon.launch("/fake/new", &history.with_new_holds());
    let client = daemon.connect().await;
    assert_eq!(add_workspace(&client, "home").await, Response::Done);
    let id = add_server(&client, "Alpha", "/fake/old").await;
    ready_server(&daemon, &id).await;
    let session = create_session_on(&client, &id, "home").await;
    let subscriber_client = daemon.connect().await;
    let mut subscriber = subscribe(&subscriber_client, &session).await;

    assert_eq!(
        update_server(&client, &id, "Alpha", "/fake/stalled").await,
        Response::Done
    );
    timeout(WAIT, pending.wait_started()).await.unwrap();
    let waiting_client = daemon.connect().await;
    let mut waiting = Box::pin(waiting_client.request(Request::Subscribe {
        session: session.clone(),
    }));
    assert!(futures::poll!(&mut waiting).is_pending());
    assert_eq!(
        update_server(&client, &id, "Alpha", "/fake/new").await,
        Response::Done
    );

    assert_eq!(
        timeout(WAIT, waiting).await.unwrap().unwrap(),
        Response::Done
    );
    ready_server(&daemon, &id).await;
    let fresh = subscriber.next_snapshot().await.to_vec();
    let mut watcher = watch(&daemon).await;
    assert_eq!(
        prompt(&client, &session, "new connection").await,
        Response::Done
    );
    assert_eq!(
        watcher.next_turn_end(&session).await,
        idle(StopReason::EndTurn)
    );
    let current = transcript(&daemon, &session).await;
    assert!(current.contains(&user_prompt("new connection")));
    assert!(current.starts_with(&fresh));
    assert_eq!(remove_server(&client, &id).await, Response::Done);
    pending.release();
    tokio::task::yield_now().await;
    assert!(watch(&daemon).await.sessions.is_empty());
}

#[tokio::test]
async fn failed_config_writes_leave_servers_and_sessions_intact() {
    for action in ["add", "update", "remove"] {
        let daemon = TestDaemon::managed();
        let history = SavedHistory::default();
        daemon.launch("/fake/alpha", &history);
        let client = daemon.connect().await;
        assert_eq!(add_workspace(&client, "home").await, Response::Done);
        let id = add_server(&client, "Alpha", "/fake/alpha").await;
        ready_server(&daemon, &id).await;
        let session = create_session_on(&client, &id, "home").await;
        let config_file = daemon.config_file.as_ref().unwrap();
        std::fs::remove_file(config_file).unwrap();
        std::fs::create_dir(config_file).unwrap();
        let response = match action {
            "add" => client
                .request(Request::AddServer {
                    name: "Beta".into(),
                    icon: ServerIcon::Claude,
                    command: "/fake/beta".into(),
                    args: Vec::new(),
                })
                .await
                .unwrap(),
            "update" => update_server(&client, &id, "Changed", "/fake/new").await,
            "remove" => remove_server(&client, &id).await,
            _ => unreachable!(),
        };
        assert!(
            matches!(response, Response::Error { .. }),
            "{action}: {response:?}"
        );
        {
            let control = daemon.control.as_ref().unwrap();
            let configuration = control.configuration.lock().unwrap();
            assert_eq!(configuration.servers.len(), 1, "{action}");
            assert_eq!(configuration.servers[0].name, "Alpha", "{action}");
        }
        assert_eq!(watch(&daemon).await.sessions.len(), 1, "{action}");
        assert_eq!(
            prompt(&client, &session, "still connected").await,
            Response::Done,
            "{action}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn several_servers_initialize_independently_before_serving() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("ur.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let state = Arc::new(Mutex::new(super::state::State::new(Vec::new())));
    let control = Arc::new(super::ServerControl {
        state: state.clone(),
        tasks: Mutex::new(Default::default()),
        configuration: Mutex::new(super::Config {
            servers: Vec::new(),
        }),
        config_file: dir.path().join("config.json"),
        launches: Mutex::new(Default::default()),
    });
    let silent_agents = Arc::new(Mutex::new(Vec::new()));
    control
        .launches
        .lock()
        .unwrap()
        .insert("/fake/stalled".into(), {
            let silent_agents = silent_agents.clone();
            Arc::new(move || {
                let (client, agent) = Channel::duplex();
                silent_agents.lock().unwrap().push(agent);
                client
            })
        });
    let history = SavedHistory::default();
    control.launches.lock().unwrap().insert(
        "/fake/healthy".into(),
        Arc::new(move || {
            let (client, agent) = Channel::duplex();
            tokio::spawn(fake_server(Hold::default(), history.clone()).connect_to(agent));
            client
        }),
    );
    let stalled = control.start(super::ServerConfig {
        id: "stalled".into(),
        name: "Stalled".into(),
        icon: ServerIcon::Claude,
        command: "/fake/stalled".into(),
        args: Vec::new(),
    });
    let healthy = control.start(super::ServerConfig {
        id: "healthy".into(),
        name: "Healthy".into(),
        icon: ServerIcon::Claude,
        command: "/fake/healthy".into(),
        args: Vec::new(),
    });
    let state_file = dir.path().join("state.json");
    let serving_state = state.clone();
    tokio::spawn(async move {
        tokio::join!(stalled.notified(), healthy.notified());
        let terminals = Arc::new(super::terminal::Terminals::new(serving_state.clone()));
        super::server::serve(
            listener,
            terminals,
            serving_state,
            state_file,
            Some(control),
        )
        .await
        .unwrap();
    });
    tokio::task::yield_now().await;
    assert!(state.lock().unwrap().server("healthy").is_ok());
    assert!(tokio::net::UnixStream::connect(&socket).await.is_ok());
    tokio::time::advance(Duration::from_secs(30)).await;
    tokio::task::yield_now().await;
    let client = timeout(WAIT, Client::connect(&socket))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        client.request(Request::Watch).await.unwrap(),
        Response::Done
    );
}
