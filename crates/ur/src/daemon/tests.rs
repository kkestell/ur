use std::path::PathBuf;
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, SessionId, SessionUpdate, ToolCallStatus,
};
use agent_client_protocol::{Channel, ConnectTo};
use anyhow::anyhow;
use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;
use ur_client::{Client, Entry, Event, Request, Response};
use ur_fake_server::{Hold, fake_server};

const WAIT: Duration = Duration::from_secs(5);

/// The daemon running in this process on a socket in its own directory.
struct TestDaemon {
    socket: PathBuf,
    _dir: TempDir,
}

impl TestDaemon {
    /// Runs the daemon with the fake server over an in-process channel.
    fn start(hold: &Hold) -> TestDaemon {
        let (client, agent) = Channel::duplex();
        tokio::spawn(fake_server(hold.clone()).connect_to(agent));
        TestDaemon::run(Ok(client))
    }

    fn run(server: anyhow::Result<Channel>) -> TestDaemon {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("ur.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        tokio::spawn(super::run(listener, server));
        TestDaemon { socket, _dir: dir }
    }

    async fn connect(&self) -> Client {
        Client::connect(&self.socket).await.unwrap()
    }
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

async fn new_session(client: &Client) -> SessionId {
    let request = Request::NewSession {
        path: PathBuf::from("/some/workspace"),
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

async fn prompt(client: &Client, session: &SessionId, text: &str) -> Response {
    let request = Request::Prompt {
        session: session.clone(),
        content: vec![ContentBlock::from(text)],
    };
    client.request(request).await.unwrap()
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
    let session = new_session(&first).await;
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
    let session = new_session(&client).await;
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
    let session = new_session(&client).await;

    // The update can arrive before or after the subscription starts; either
    // way it belongs in the transcript.
    let mut subscription = subscribe(&client, &session).await;
    let transcript = subscription.wait_for(1).await;
    assert!(is_commands_update(&transcript[0]), "{transcript:?}");
}

#[tokio::test]
async fn permission_requests_are_rejected() {
    let daemon = TestDaemon::start(&Hold::default());
    let client = daemon.connect().await;
    let session = new_session(&client).await;
    let mut subscription = subscribe(&client, &session).await;

    assert_eq!(prompt(&client, &session, "tool").await, Response::Done);

    let transcript = subscription.wait_for(5).await;
    assert!(
        matches!(
            update(&transcript[2]),
            SessionUpdate::ToolCall(call) if call.tool_call_id.to_string() == "tally-1"
        ),
        "{transcript:?}"
    );
    assert!(
        matches!(
            update(&transcript[3]),
            SessionUpdate::ToolCallUpdate(update)
                if update.fields.status == Some(ToolCallStatus::Failed)
        ),
        "{transcript:?}"
    );
    let SessionUpdate::AgentMessageChunk(ContentChunk {
        content: ContentBlock::Text(text),
        ..
    }) = update(&transcript[4])
    else {
        panic!("expected a message: {transcript:?}");
    };
    assert!(
        text.text
            .contains("ur does not answer permission requests yet"),
        "{}",
        text.text
    );
}

#[tokio::test]
async fn session_requests_fail_without_a_server() {
    let daemon = TestDaemon::run(Err(anyhow!("the config file is missing")));
    let client = daemon.connect().await;

    let request = Request::NewSession {
        path: PathBuf::from("/some/workspace"),
    };
    match client.request(request).await.unwrap() {
        Response::Error { message } => {
            assert!(message.contains("the config file is missing"), "{message}");
        }
        other => panic!("unexpected response {other:?}"),
    }
    assert!(matches!(
        client.request(Request::OpenTerminal).await.unwrap(),
        Response::Opened { .. }
    ));
}
