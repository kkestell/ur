use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol::schema::v1::{
    ContentBlock, PermissionOptionId, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionId, SessionNotification,
    StopReason,
};
use agent_client_protocol::{Agent, ConnectionTo, Responder};
use anyhow::{anyhow, bail};
use ur_client::{
    DaemonMessage, Entry, Event, Frame, PendingPermission, Response, SessionSummary, Status,
    Workspace,
};

use super::server::Outbox;

/// The daemon's ACP connection, workspaces, and sessions, behind one std
/// mutex. Methods queue their events on outboxes while the lock is held, which
/// keeps each daemon client's events in order. They never wait and never
/// await.
pub struct State {
    /// The ACP connection, or why there is none.
    server: Result<ConnectionTo<Agent>, String>,
    /// In the order they were added.
    workspaces: Vec<Workspace>,
    /// In the order they were created.
    sessions: Vec<Session>,
    watchers: Vec<Outbox>,
    /// The request ID for the next pending permission request.
    next_request_id: u32,
}

struct Session {
    id: SessionId,
    workspace: String,
    transcript: Vec<Entry>,
    /// The operation guard, held while a prompt runs.
    op: bool,
    subscribers: Vec<Outbox>,
    status: Status,
    unread: bool,
    /// The socket connections that focus this session.
    focus: Vec<Outbox>,
    /// The responder of each request in `Status::NeedsPermission`, by request
    /// ID.
    responders: HashMap<u32, Responder<RequestPermissionResponse>>,
    /// Set by cancellation until the prompt returns.
    cancelling: bool,
}

impl State {
    pub fn new(workspaces: Vec<Workspace>) -> State {
        State {
            server: Err("the server has not started".to_string()),
            workspaces,
            sessions: Vec::new(),
            watchers: Vec::new(),
            next_request_id: 1,
        }
    }

    pub fn set_server(&mut self, server: Result<ConnectionTo<Agent>, String>) {
        self.server = server;
    }

    pub fn connection(&self) -> anyhow::Result<ConnectionTo<Agent>> {
        connection(&self.server)
    }

    /// Queues the watch snapshot and registers the outbox for later changes.
    /// Watching again from the same socket connection replaces the earlier
    /// registration.
    pub fn watch(&mut self, outbox: Outbox) {
        outbox.send(event(Event::WatchSnapshot {
            workspaces: self.workspaces.clone(),
            sessions: self.sessions.iter().map(Session::summary).collect(),
        }));
        self.watchers
            .retain(|other| !other.same_connection(&outbox));
        self.watchers.push(outbox);
    }

    pub fn workspace_path(&self, name: &str) -> anyhow::Result<PathBuf> {
        self.workspaces
            .iter()
            .find(|workspace| workspace.name == name)
            .map(|workspace| workspace.path.clone())
            .ok_or_else(|| anyhow!("no workspace {name}"))
    }

    /// Checks the name, then calls `save` with the new list of workspaces and
    /// adds the workspace only if `save` succeeds.
    pub fn add_workspace(
        &mut self,
        workspace: Workspace,
        save: impl FnOnce(&[Workspace]) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if workspace.name.is_empty() {
            bail!("a workspace needs a name");
        }
        if self
            .workspaces
            .iter()
            .any(|other| other.name == workspace.name)
        {
            bail!("workspace {} already exists", workspace.name);
        }
        let mut workspaces = self.workspaces.clone();
        workspaces.push(workspace.clone());
        save(&workspaces)?;
        self.workspaces = workspaces;
        broadcast(&mut self.watchers, Event::WorkspaceAdded { workspace });
        Ok(())
    }

    /// Calls `save` with the list of workspaces without this one. If it
    /// succeeds, cancels the workspace's running prompts, removes its sessions,
    /// and removes it. The cancelled prompts are not waited for; their results
    /// and later updates find no session.
    pub fn remove_workspace(
        &mut self,
        name: &str,
        save: impl FnOnce(&[Workspace]) -> anyhow::Result<()>,
        send_cancel: impl Fn(&ConnectionTo<Agent>, &SessionId) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<()> {
        if !self
            .workspaces
            .iter()
            .any(|workspace| workspace.name == name)
        {
            bail!("no workspace {name}");
        }
        let workspaces: Vec<_> = self
            .workspaces
            .iter()
            .filter(|workspace| workspace.name != name)
            .cloned()
            .collect();
        save(&workspaces)?;
        self.workspaces = workspaces;
        for session in self.sessions.iter_mut().filter(|s| s.workspace == name) {
            if session.op {
                // The removal is saved, so it goes ahead. Without an ACP
                // connection, there is no prompt left to cancel.
                if let Err(error) = connection(&self.server)
                    .and_then(|connection| Ok(send_cancel(&connection, &session.id)?))
                {
                    eprintln!("ur daemon: cancelling {}: {error:#}", session.id);
                }
                session.answer_cancelled();
            }
            let removed = event(Event::SessionRemoved {
                session: session.id.clone(),
            });
            for outbox in &session.subscribers {
                outbox.send(removed.clone());
            }
        }
        self.sessions.retain(|session| session.workspace != name);
        broadcast(
            &mut self.watchers,
            Event::WorkspaceRemoved {
                name: name.to_string(),
            },
        );
        Ok(())
    }

    /// Adds a session created in `workspace`, unless the workspace was removed
    /// while `session/new` was in flight.
    pub fn add_session(&mut self, id: SessionId, workspace: String) -> anyhow::Result<()> {
        if !self.workspaces.iter().any(|other| other.name == workspace) {
            bail!("workspace {workspace} was removed");
        }
        let session = Session {
            id,
            workspace,
            transcript: Vec::new(),
            op: false,
            subscribers: Vec::new(),
            status: Status::Idle { last_stop: None },
            unread: false,
            focus: Vec::new(),
            responders: HashMap::new(),
            cancelling: false,
        };
        publish(&mut self.watchers, &session);
        self.sessions.push(session);
        Ok(())
    }

    /// Appends an ACP update entry. An update for an unknown session is
    /// dropped.
    pub fn apply_update(&mut self, notification: SessionNotification) {
        let id = notification.session_id;
        let Ok(session) = find(&mut self.sessions, &id) else {
            eprintln!("ur daemon: dropping an update for unknown session {id}");
            return;
        };
        session.append(Entry::Update {
            update: Box::new(notification.update),
        });
    }

    /// Queues the session snapshot and registers the outbox for later entries.
    /// Subscribing again from the same socket connection replaces the earlier
    /// subscription.
    pub fn subscribe(&mut self, id: &SessionId, outbox: Outbox) -> anyhow::Result<()> {
        let session = find(&mut self.sessions, id)?;
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

    /// Makes `sessions` the ones this socket connection focuses, and clears
    /// their unread flags. An unknown session changes nothing.
    pub fn focus(&mut self, outbox: &Outbox, sessions: &[SessionId]) -> anyhow::Result<()> {
        for id in sessions {
            find(&mut self.sessions, id)?;
        }
        for session in &mut self.sessions {
            session.focus.retain(|other| !other.same_connection(outbox));
            if sessions.contains(&session.id) {
                session.focus.push(outbox.clone());
                if session.unread {
                    session.unread = false;
                    publish(&mut self.watchers, session);
                }
            }
        }
        Ok(())
    }

    /// Clears the focus of a socket connection that ended.
    pub fn disconnect(&mut self, outbox: &Outbox) {
        for session in &mut self.sessions {
            session.focus.retain(|other| !other.same_connection(outbox));
        }
    }

    /// Answers busy while the session's operation guard is held. Otherwise
    /// calls `send` to send `session/prompt`, then holds the guard, appends
    /// the user prompt entry, sets `Working`, and clears the unread flag,
    /// since whoever prompts has seen the session. Sending first means a failed
    /// send leaves nothing to undo, and the server's first update waits for
    /// the lock, so it follows the user prompt entry.
    pub fn start_prompt(
        &mut self,
        id: &SessionId,
        content: Vec<ContentBlock>,
        send: impl FnOnce(ConnectionTo<Agent>, Vec<ContentBlock>) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<Response> {
        let connection = self.connection()?;
        let session = find(&mut self.sessions, id)?;
        if session.op {
            return Ok(Response::Busy);
        }
        send(connection, content.clone())?;
        session.op = true;
        session.append(Entry::UserPrompt { content });
        session.status = Status::Working;
        session.unread = false;
        publish(&mut self.watchers, session);
        Ok(Response::Done)
    }

    /// Ends the prompt with its stop reason, or with the message of the error
    /// it returned, which becomes a turn error entry. Remaining pending
    /// permission requests are dropped. A session removed with its workspace
    /// is gone, and nothing happens.
    pub fn finish_prompt(&mut self, id: &SessionId, result: Result<StopReason, String>) {
        let Ok(session) = find(&mut self.sessions, id) else {
            return;
        };
        session.status = match result {
            Ok(stop) => Status::Idle {
                last_stop: Some(stop),
            },
            Err(message) => {
                session.append(Entry::TurnError {
                    message: message.clone(),
                });
                Status::Failed { message }
            }
        };
        // Dropping a responder sends no reply. The turn is over, so the server
        // no longer waits for one.
        session.responders.clear();
        session.op = false;
        session.cancelling = false;
        if session.focus.is_empty() {
            session.unread = true;
        }
        publish(&mut self.watchers, session);
    }

    /// Adds a pending permission request and sets `NeedsPermission`. During
    /// cancellation, answers `Cancelled` at once instead.
    pub fn request_permission(
        &mut self,
        request: RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    ) -> agent_client_protocol::Result<()> {
        let Ok(session) = find(&mut self.sessions, &request.session_id) else {
            return responder
                .respond_with_internal_error(format!("no session {}", request.session_id));
        };
        if session.cancelling {
            return responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ));
        }
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let pending = PendingPermission {
            request_id,
            request,
        };
        match &mut session.status {
            Status::NeedsPermission { requests } => requests.push(pending),
            status => {
                *status = Status::NeedsPermission {
                    requests: vec![pending],
                }
            }
        }
        session.responders.insert(request_id, responder);
        publish(&mut self.watchers, session);
        Ok(())
    }

    /// Answers a pending permission request with one of its options. The
    /// first answer removes the request, so a later one finds it resolved.
    pub fn answer_permission(
        &mut self,
        id: &SessionId,
        request_id: u32,
        option_id: PermissionOptionId,
    ) -> anyhow::Result<()> {
        let session = find(&mut self.sessions, id)?;
        let Status::NeedsPermission { requests } = &mut session.status else {
            bail!("permission request {request_id} is resolved");
        };
        let Some(position) = requests
            .iter()
            .position(|pending| pending.request_id == request_id)
        else {
            bail!("permission request {request_id} is resolved");
        };
        if !requests[position]
            .request
            .options
            .iter()
            .any(|option| option.option_id == option_id)
        {
            bail!("permission request {request_id} has no option {option_id}");
        }
        requests.remove(position);
        if requests.is_empty() {
            session.status = Status::Working;
        }
        let responder = session
            .responders
            .remove(&request_id)
            .expect("every pending permission request has a responder");
        publish(&mut self.watchers, session);
        responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
        ))?;
        Ok(())
    }

    /// Calls `send_cancel`, answers every pending permission request with
    /// `Cancelled`, and sets `Working`. The operation guard stays held until
    /// the prompt returns. Without a running prompt, does nothing.
    pub fn cancel(
        &mut self,
        id: &SessionId,
        send_cancel: impl FnOnce(&ConnectionTo<Agent>, &SessionId) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<()> {
        let session = find(&mut self.sessions, id)?;
        if !session.op {
            return Ok(());
        }
        send_cancel(&connection(&self.server)?, id)?;
        session.answer_cancelled();
        session.cancelling = true;
        if session.status != Status::Working {
            session.status = Status::Working;
            publish(&mut self.watchers, session);
        }
        Ok(())
    }
}

impl Session {
    fn append(&mut self, entry: Entry) {
        let frame = event(Event::Entry {
            session: self.id.clone(),
            entry: entry.clone(),
        });
        self.subscribers.retain(|outbox| outbox.send(frame.clone()));
        self.transcript.push(entry);
    }

    /// Answers every pending permission request with `Cancelled`, which ACP
    /// requires after `session/cancel`.
    fn answer_cancelled(&mut self) {
        for (_, responder) in self.responders.drain() {
            let cancelled = RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled);
            if let Err(error) = responder.respond(cancelled) {
                eprintln!("ur daemon: answering a permission request: {error}");
            }
        }
    }

    fn summary(&self) -> SessionSummary {
        SessionSummary {
            session: self.id.clone(),
            workspace: self.workspace.clone(),
            status: self.status.clone(),
            unread: self.unread,
        }
    }
}

fn connection(server: &Result<ConnectionTo<Agent>, String>) -> anyhow::Result<ConnectionTo<Agent>> {
    server
        .clone()
        .map_err(|reason| anyhow!("no ACP connection: {reason}"))
}

fn find<'a>(sessions: &'a mut [Session], id: &SessionId) -> anyhow::Result<&'a mut Session> {
    sessions
        .iter_mut()
        .find(|session| session.id == *id)
        .ok_or_else(|| anyhow!("no session {id}"))
}

/// Queues `session_changed` for the session to every watcher.
fn publish(watchers: &mut Vec<Outbox>, session: &Session) {
    broadcast(
        watchers,
        Event::SessionChanged {
            summary: session.summary(),
        },
    );
}

fn broadcast(watchers: &mut Vec<Outbox>, event: Event) {
    let frame = Frame::json(&DaemonMessage::Event { event });
    watchers.retain(|outbox| outbox.send(frame.clone()));
}

fn event(event: Event) -> Frame {
    Frame::json(&DaemonMessage::Event { event })
}
