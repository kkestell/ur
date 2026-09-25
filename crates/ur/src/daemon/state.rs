use std::collections::HashMap;

use agent_client_protocol::schema::v1::{ContentBlock, SessionId, SessionNotification};
use agent_client_protocol::{Agent, ConnectionTo};
use anyhow::anyhow;
use ur_client::{DaemonMessage, Entry, Event, Frame, Response};

use super::server::Outbox;

/// The daemon's ACP connection and sessions, behind one std mutex. Methods
/// queue their events on outboxes while the lock is held, which keeps each
/// subscriber's entries in transcript order. They never wait and never await.
pub struct State {
    /// The ACP connection, or why there is none.
    server: Result<ConnectionTo<Agent>, String>,
    sessions: HashMap<SessionId, Session>,
}

#[derive(Default)]
struct Session {
    transcript: Vec<Entry>,
    /// The operation guard, held while a prompt runs.
    op: bool,
    subscribers: Vec<Outbox>,
}

impl Default for State {
    fn default() -> State {
        State {
            server: Err("the server has not started".to_string()),
            sessions: HashMap::new(),
        }
    }
}

impl State {
    pub fn set_server(&mut self, server: Result<ConnectionTo<Agent>, String>) {
        self.server = server;
    }

    pub fn connection(&self) -> anyhow::Result<ConnectionTo<Agent>> {
        self.server
            .clone()
            .map_err(|reason| anyhow!("no ACP connection: {reason}"))
    }

    pub fn add_session(&mut self, id: SessionId) {
        self.sessions.insert(id, Session::default());
    }

    /// Appends an ACP update entry. An update for an unknown session is
    /// dropped.
    pub fn apply_update(&mut self, notification: SessionNotification) {
        let id = notification.session_id;
        let Some(session) = self.sessions.get_mut(&id) else {
            eprintln!("ur daemon: dropping an update for unknown session {id}");
            return;
        };
        session.append(
            &id,
            Entry::Update {
                update: Box::new(notification.update),
            },
        );
    }

    /// Queues the session snapshot and registers the outbox for later entries.
    /// Subscribing again from the same socket connection replaces the earlier
    /// subscription.
    pub fn subscribe(&mut self, id: &SessionId, outbox: Outbox) -> anyhow::Result<()> {
        let session = self.session(id)?;
        outbox.send(event(Event::SessionSnapshot {
            session: id.clone(),
            transcript: session.transcript.clone(),
        }));
        session
            .subscribers
            .retain(|other| !other.same_connection(&outbox));
        session.subscribers.push(outbox);
        Ok(())
    }

    /// Answers busy while the session's operation guard is held. Otherwise
    /// calls `send` to send `session/prompt`, then holds the guard and appends
    /// the user prompt entry. Sending first means a failed send leaves nothing
    /// to undo, and the server's first update waits for the lock, so it
    /// follows the user prompt entry.
    pub fn start_prompt(
        &mut self,
        id: &SessionId,
        content: Vec<ContentBlock>,
        send: impl FnOnce(ConnectionTo<Agent>, Vec<ContentBlock>) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<Response> {
        let connection = self.connection()?;
        let session = self.session(id)?;
        if session.op {
            return Ok(Response::Busy);
        }
        send(connection, content.clone())?;
        session.op = true;
        session.append(id, Entry::UserPrompt { content });
        Ok(Response::Done)
    }

    pub fn finish_prompt(&mut self, id: &SessionId) {
        self.sessions
            .get_mut(id)
            .expect("sessions are never removed")
            .op = false;
    }

    fn session(&mut self, id: &SessionId) -> anyhow::Result<&mut Session> {
        self.sessions
            .get_mut(id)
            .ok_or_else(|| anyhow!("no session {id}"))
    }
}

impl Session {
    fn append(&mut self, id: &SessionId, entry: Entry) {
        let frame = event(Event::Entry {
            session: id.clone(),
            entry: entry.clone(),
        });
        self.subscribers.retain(|outbox| outbox.send(frame.clone()));
        self.transcript.push(entry);
    }
}

fn event(event: Event) -> Frame {
    Frame::json(&DaemonMessage::Event { event })
}
