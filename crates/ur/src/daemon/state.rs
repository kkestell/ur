use std::collections::HashMap;
use std::path::PathBuf;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, DeleteSessionRequest, LoadSessionRequest, PermissionOptionId,
    PromptRequest, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionConfigOption, SessionId, SessionInfo, SessionNotification,
    SessionUpdate, StopReason,
};
use agent_client_protocol::{Agent, ConnectionTo, Responder};
use anyhow::{anyhow, bail};
use ur_client::{
    DaemonMessage, Entry, Event, Frame, PendingPermission, Response, SessionSummary, Status,
    TerminalId, TerminalSummary, Workspace,
};

use super::server::Outbox;

/// The daemon's ACP connection, workspaces, sessions, and terminal summaries,
/// behind one std mutex. `Terminals` holds the PTYs and reports to it.
/// Methods queue their events on outboxes while the lock is held, which keeps
/// each daemon client's events in order. They never wait and never await.
pub struct State {
    /// The ACP connection, or why there is none.
    server: Result<Server, String>,
    /// The generation of the current or last ACP connection. Prompt and load
    /// results from an earlier one are ignored.
    generation: u64,
    /// In the order they were added.
    workspaces: Vec<Workspace>,
    /// In the order they were created.
    sessions: Vec<Session>,
    /// In the order they were opened.
    terminals: Vec<TerminalSummary>,
    watchers: Vec<Outbox>,
    /// The request ID for the next pending permission request.
    next_request_id: u32,
}

/// The ACP connection and the capabilities from its `initialize`.
pub struct Server {
    pub connection: ConnectionTo<Agent>,
    pub capabilities: AgentCapabilities,
}

impl Server {
    pub fn can_list(&self) -> bool {
        self.capabilities.session_capabilities.list.is_some()
    }

    fn can_load(&self) -> bool {
        self.capabilities.load_session
    }

    fn can_delete(&self) -> bool {
        self.capabilities.session_capabilities.delete.is_some()
    }
}

struct Session {
    id: SessionId,
    workspace: String,
    transcript: Vec<Entry>,
    /// Whether the transcript came from `session/new` or `session/load` during
    /// the current ACP connection.
    loaded: bool,
    title: Option<String>,
    /// The last activity.
    updated_at: Option<String>,
    config_options: Vec<SessionConfigOption>,
    /// The operation guard.
    op: Option<Op>,
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

/// The session operation that holds the operation guard.
enum Op {
    /// A turn is running.
    Prompt {
        /// The waiting delete: the request ID and outbox of a `delete_session`
        /// that cancelled the turn and sends `session/delete` when it ends.
        delete: Option<(u64, Outbox)>,
    },
    /// `session/load` is in flight.
    Load {
        /// The replayed entries, which become the transcript if the load
        /// succeeds.
        replay: Vec<Entry>,
        /// The request ID and outbox of each `subscribe` that answers when the
        /// load ends.
        waiting: Vec<(u64, Outbox)>,
    },
    /// `session/delete` is in flight.
    Delete,
}

impl State {
    pub fn new(workspaces: Vec<Workspace>) -> State {
        State {
            server: Err("the server has not started".to_string()),
            generation: 0,
            workspaces,
            sessions: Vec::new(),
            terminals: Vec::new(),
            watchers: Vec::new(),
            next_request_id: 1,
        }
    }

    /// Sets the ACP connection, which starts a new generation and sends its
    /// capabilities to every watcher, or why there is none.
    pub fn set_server(&mut self, server: Result<Server, String>) {
        if let Ok(server) = &server {
            self.generation += 1;
            broadcast(
                &mut self.watchers,
                Event::CapabilitiesChanged {
                    capabilities: Box::new(server.capabilities.clone()),
                },
            );
        }
        self.server = server;
    }

    pub fn server(&self) -> anyhow::Result<&Server> {
        server(&self.server)
    }

    pub fn connection(&self) -> anyhow::Result<ConnectionTo<Agent>> {
        Ok(self.server()?.connection.clone())
    }

    pub fn workspaces(&self) -> Vec<Workspace> {
        self.workspaces.clone()
    }

    /// Records that the ACP connection ended. The new server process has
    /// loaded no sessions, so every session becomes unloaded. Every running
    /// turn fails with `reason` and its pending permission requests are
    /// dropped. A running load ends with the unchanged transcript, and a
    /// waiting delete answers an error. Every operation guard is released.
    pub fn server_exited(&mut self, reason: String) {
        for session in &mut self.sessions {
            session.loaded = false;
            match session.op.take() {
                Some(Op::Load { waiting, .. }) => session.end_load(waiting),
                Some(Op::Prompt {
                    delete: Some((id, outbox)),
                }) => {
                    respond(
                        &outbox,
                        id,
                        Response::Error {
                            message: "the server exited before the session was deleted".to_string(),
                        },
                    );
                }
                // A `session/delete` in flight answers from its callback.
                Some(Op::Prompt { delete: None } | Op::Delete) | None => {}
            }
            // A prompt that starts with a load is `Working` during the load.
            if matches!(
                session.status,
                Status::Working | Status::NeedsPermission { .. }
            ) {
                session.end_turn(Err(reason.clone()));
                publish(&mut self.watchers, session);
            }
        }
        self.server = Err(reason);
    }

    /// Queues the watch snapshot and registers the outbox for later changes.
    /// Watching again from the same socket connection replaces the earlier
    /// registration.
    pub fn watch(&mut self, outbox: Outbox) {
        outbox.send(event(Event::WatchSnapshot {
            workspaces: self.workspaces.clone(),
            sessions: self.sessions.iter().map(Session::summary).collect(),
            terminals: self.terminals.clone(),
            capabilities: self
                .server
                .as_ref()
                .ok()
                .map(|server| Box::new(server.capabilities.clone())),
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
    /// succeeds, cancels the workspace's running prompts, removes its sessions
    /// and terminal summaries, and removes it. Returns the removed terminals,
    /// which the caller closes. The cancelled prompts are not waited for; their
    /// results and later updates find no session. A waiting delete answers
    /// `Done`, since its session is gone.
    pub fn remove_workspace(
        &mut self,
        name: &str,
        save: impl FnOnce(&[Workspace]) -> anyhow::Result<()>,
        send_cancel: impl Fn(&ConnectionTo<Agent>, &SessionId) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<Vec<TerminalId>> {
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
            if let Some(Op::Prompt { delete }) = &mut session.op {
                if let Some((id, outbox)) = delete.take() {
                    respond(&outbox, id, Response::Done);
                }
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
        let terminals = self
            .terminals
            .extract_if(.., |terminal| terminal.workspace == name)
            .map(|terminal| terminal.terminal)
            .collect();
        broadcast(
            &mut self.watchers,
            Event::WorkspaceRemoved {
                name: name.to_string(),
            },
        );
        Ok(terminals)
    }

    pub fn add_terminal(&mut self, summary: TerminalSummary) {
        broadcast(
            &mut self.watchers,
            Event::TerminalChanged {
                summary: summary.clone(),
            },
        );
        self.terminals.push(summary);
    }

    /// Does nothing when the terminal title is unchanged, or the terminal was
    /// removed with its workspace.
    pub fn set_terminal_title(&mut self, id: TerminalId, title: String) {
        let Some(summary) = self
            .terminals
            .iter_mut()
            .find(|summary| summary.terminal == id)
        else {
            return;
        };
        if summary.title != title {
            summary.title = title;
            let summary = summary.clone();
            broadcast(&mut self.watchers, Event::TerminalChanged { summary });
        }
    }

    /// Removes the terminal summary if it is still there, and sends
    /// `terminal_exited` to every watcher and to each attached outbox whose
    /// socket connection is not watching, so each gets it once.
    pub fn remove_terminal(&mut self, id: TerminalId, attached: &[Outbox]) {
        self.terminals.retain(|summary| summary.terminal != id);
        let exited = Event::TerminalExited { terminal: id };
        let frame = event(exited.clone());
        for outbox in attached {
            if !self
                .watchers
                .iter()
                .any(|watcher| watcher.same_connection(outbox))
            {
                outbox.send(frame.clone());
            }
        }
        broadcast(&mut self.watchers, exited);
    }

    /// Adds a session created in `workspace` with the config options from
    /// `session/new`, unless the workspace was removed while `session/new` was
    /// in flight.
    pub fn add_session(
        &mut self,
        id: SessionId,
        workspace: String,
        config_options: Vec<SessionConfigOption>,
    ) -> anyhow::Result<()> {
        if !self.workspaces.iter().any(|other| other.name == workspace) {
            bail!("workspace {workspace} was removed");
        }
        let mut session = Session::new(id, workspace);
        session.loaded = true;
        session.config_options = config_options;
        publish(&mut self.watchers, &session);
        self.sessions.push(session);
        Ok(())
    }

    /// Adds each saved session from `session/list` that is not in `State`,
    /// and updates the session title and last activity of the others. Does
    /// nothing if the workspace was removed while the list was in flight.
    pub fn add_saved_sessions(&mut self, workspace: &str, sessions: Vec<SessionInfo>) {
        if !self.workspaces.iter().any(|other| other.name == workspace) {
            return;
        }
        for info in sessions {
            match find(&mut self.sessions, &info.session_id) {
                Ok(session) => {
                    if session.title != info.title || session.updated_at != info.updated_at {
                        session.title = info.title;
                        session.updated_at = info.updated_at;
                        publish(&mut self.watchers, session);
                    }
                }
                Err(_) => {
                    let mut session = Session::new(info.session_id, workspace.to_string());
                    session.title = info.title;
                    session.updated_at = info.updated_at;
                    publish(&mut self.watchers, &session);
                    self.sessions.push(session);
                }
            }
        }
    }

    /// Appends an ACP update entry, or adds it to the replay while a load
    /// runs. A `session_info_update` also changes the session title and last
    /// activity, and a `config_option_update` the config options. An update
    /// for an unknown session is dropped.
    pub fn apply_update(&mut self, notification: SessionNotification) {
        let id = notification.session_id;
        let Ok(session) = find(&mut self.sessions, &id) else {
            eprintln!("ur daemon: dropping an update for unknown session {id}");
            return;
        };
        if let SessionUpdate::SessionInfoUpdate(info) = &notification.update {
            // A null value clears the field, and an absent one leaves it.
            let mut changed = false;
            if let Some(title) = info.title.as_opt_ref() {
                changed |= replace(&mut session.title, title.cloned());
            }
            if let Some(updated_at) = info.updated_at.as_opt_ref() {
                changed |= replace(&mut session.updated_at, updated_at.cloned());
            }
            if changed {
                publish(&mut self.watchers, session);
            }
        }
        if let SessionUpdate::ConfigOptionUpdate(update) = &notification.update {
            session.set_config_options(update.config_options.clone());
        }
        let entry = Entry::Update {
            update: Box::new(notification.update),
        };
        match &mut session.op {
            Some(Op::Load { replay, .. }) => replay.push(entry),
            _ => session.append(entry),
        }
    }

    /// Registers the outbox for the session's later entries, and queues the
    /// session snapshot and returns `done`. Subscribing to an unloaded session
    /// that is not `Failed` calls `load` to send `session/load` when the server
    /// can load it and no other session operation runs. During any load, the
    /// snapshot and the response wait for it to end, and this returns `None`.
    /// Subscribing again from the same socket connection replaces the earlier
    /// subscription.
    pub fn subscribe(
        &mut self,
        id: &SessionId,
        request_id: u64,
        outbox: Outbox,
        load: impl FnOnce(
            &ConnectionTo<Agent>,
            u64,
            LoadSessionRequest,
        ) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<Option<Response>> {
        let generation = self.generation;
        let can_load = self.server.as_ref().is_ok_and(Server::can_load);
        let session = find(&mut self.sessions, id)?;
        let waits = match &mut session.op {
            Some(Op::Load { waiting, .. }) => {
                waiting.push((request_id, outbox.clone()));
                true
            }
            Some(Op::Prompt { .. } | Op::Delete) => false,
            None => {
                if !session.loaded && can_load && !matches!(session.status, Status::Failed { .. }) {
                    let connection = connection(&self.server)?;
                    load(
                        &connection,
                        generation,
                        load_request(&self.workspaces, session),
                    )?;
                    session.op = Some(Op::Load {
                        replay: Vec::new(),
                        waiting: vec![(request_id, outbox.clone())],
                    });
                    true
                } else {
                    false
                }
            }
        };
        if !waits {
            outbox.send(session.snapshot());
        }
        session
            .subscribers
            .retain(|other| !other.same_connection(&outbox));
        session.subscribers.push(outbox);
        Ok((!waits).then_some(Response::Done))
    }

    /// Loads every unloaded session that has subscribers, calling `load` for
    /// each, when the server can load them.
    pub fn reload_subscribed(
        &mut self,
        load: impl Fn(
            &ConnectionTo<Agent>,
            u64,
            LoadSessionRequest,
        ) -> agent_client_protocol::Result<()>,
    ) {
        let Ok(server) = &self.server else {
            return;
        };
        if !server.can_load() {
            return;
        }
        for session in &mut self.sessions {
            if session.loaded || session.op.is_some() || session.subscribers.is_empty() {
                continue;
            }
            let request = load_request(&self.workspaces, session);
            if let Err(error) = load(&server.connection, self.generation, request) {
                eprintln!("ur daemon: loading {}: {error}", session.id);
                continue;
            }
            session.op = Some(Op::Load {
                replay: Vec::new(),
                waiting: Vec::new(),
            });
        }
    }

    /// Ends a load. On success, the replay becomes the transcript, followed by
    /// the old transcript's trailing turn error entry if it had one, which
    /// keeps the error of a turn the server exit interrupted, and the config
    /// options from `session/load` replace the old ones when it has any. On
    /// failure, the
    /// transcript is unchanged, the session stays unloaded, and it becomes
    /// `Failed`. Either way, every subscriber gets a session snapshot and each
    /// waiting `subscribe` is answered. Returns whether the session is loaded.
    /// A result from an earlier generation, or for a session that is gone, is
    /// ignored.
    pub fn finish_load(
        &mut self,
        id: &SessionId,
        generation: u64,
        result: Result<Option<Vec<SessionConfigOption>>, String>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        let Ok(session) = find(&mut self.sessions, id) else {
            return false;
        };
        let (replay, waiting) = match session.op.take() {
            Some(Op::Load { replay, waiting }) => (replay, waiting),
            other => {
                session.op = other;
                return false;
            }
        };
        match result {
            Ok(config_options) => {
                let error = match session.transcript.last() {
                    Some(error @ Entry::TurnError { .. }) => Some(error.clone()),
                    _ => None,
                };
                session.transcript = replay;
                session.transcript.extend(error);
                session.loaded = true;
                if let Some(config_options) = config_options {
                    session.config_options = config_options;
                }
            }
            Err(message) => {
                session.status = Status::Failed {
                    message: format!("session/load failed: {message}"),
                };
                publish(&mut self.watchers, session);
            }
        }
        session.end_load(waiting);
        session.loaded
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

    /// Answers busy while the session's operation guard is held. A loaded
    /// session is prompted as `start_prompt` describes. An unloaded one is
    /// loaded first: `load` sends `session/load`, and the load holds the
    /// guard, sets `Working`, and clears the unread flag. Its callback sends
    /// the prompt when the load succeeds.
    pub fn prompt(
        &mut self,
        id: &SessionId,
        content: Vec<ContentBlock>,
        send: impl FnOnce(&ConnectionTo<Agent>, u64, PromptRequest) -> agent_client_protocol::Result<()>,
        load: impl FnOnce(
            &ConnectionTo<Agent>,
            u64,
            LoadSessionRequest,
        ) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<Response> {
        let generation = self.generation;
        let server = server(&self.server)?;
        let session = find(&mut self.sessions, id)?;
        if session.op.is_some() {
            return Ok(Response::Busy);
        }
        if session.loaded {
            return self.start_prompt(id, content, send);
        }
        if !server.can_load() {
            bail!("the server cannot load session {id}");
        }
        load(
            &server.connection,
            generation,
            load_request(&self.workspaces, session),
        )?;
        session.op = Some(Op::Load {
            replay: Vec::new(),
            waiting: Vec::new(),
        });
        session.status = Status::Working;
        session.unread = false;
        publish(&mut self.watchers, session);
        Ok(Response::Done)
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
        send: impl FnOnce(&ConnectionTo<Agent>, u64, PromptRequest) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<Response> {
        let generation = self.generation;
        let connection = self.connection()?;
        let session = find(&mut self.sessions, id)?;
        if session.op.is_some() {
            return Ok(Response::Busy);
        }
        send(
            &connection,
            generation,
            PromptRequest::new(id.clone(), content.clone()),
        )?;
        session.op = Some(Op::Prompt { delete: None });
        session.append(Entry::UserPrompt { content });
        session.status = Status::Working;
        session.unread = false;
        publish(&mut self.watchers, session);
        Ok(Response::Done)
    }

    /// Ends the prompt with its stop reason, or with the message of the error
    /// it returned, and returns the waiting delete, if any. A result from an
    /// earlier generation, or for a session removed with its workspace, is
    /// ignored.
    pub fn finish_prompt(
        &mut self,
        id: &SessionId,
        generation: u64,
        result: Result<StopReason, String>,
    ) -> Option<(u64, Outbox)> {
        if generation != self.generation {
            return None;
        }
        let Ok(session) = find(&mut self.sessions, id) else {
            return None;
        };
        let delete = match session.op.take() {
            Some(Op::Prompt { delete }) => delete,
            _ => None,
        };
        session.end_turn(result);
        publish(&mut self.watchers, session);
        delete
    }

    /// Answers an error when the server does not advertise `session/delete`,
    /// and busy while a load or delete holds the operation guard. An idle
    /// session calls `send_delete` to send `session/delete` and holds the
    /// guard. A running turn is cancelled as `cancel` does, and the request
    /// becomes the waiting delete, sent when the turn ends. Returns `None`
    /// when the request answers later.
    pub fn delete_session(
        &mut self,
        id: &SessionId,
        request_id: u64,
        outbox: Outbox,
        send_cancel: impl FnOnce(&ConnectionTo<Agent>, &SessionId) -> agent_client_protocol::Result<()>,
        send_delete: impl FnOnce(
            &ConnectionTo<Agent>,
            u64,
            DeleteSessionRequest,
        ) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<Option<Response>> {
        let generation = self.generation;
        let server = server(&self.server)?;
        if !server.can_delete() {
            bail!("the server cannot delete sessions");
        }
        let session = find(&mut self.sessions, id)?;
        match &mut session.op {
            None => {
                send_delete(
                    &server.connection,
                    generation,
                    DeleteSessionRequest::new(id.clone()),
                )?;
                session.op = Some(Op::Delete);
            }
            Some(Op::Prompt { delete: None }) => {
                cancel_turn(&mut self.watchers, session, &server.connection, send_cancel)?;
                session.op = Some(Op::Prompt {
                    delete: Some((request_id, outbox)),
                });
            }
            Some(Op::Load { .. } | Op::Delete | Op::Prompt { delete: Some(_) }) => {
                return Ok(Some(Response::Busy));
            }
        }
        Ok(None)
    }

    /// Sends the waiting delete's `session/delete` through `send_delete` once
    /// its turn has ended, and holds the operation guard.
    pub fn start_delete(
        &mut self,
        id: &SessionId,
        send_delete: impl FnOnce(
            &ConnectionTo<Agent>,
            u64,
            DeleteSessionRequest,
        ) -> agent_client_protocol::Result<()>,
    ) -> anyhow::Result<()> {
        let generation = self.generation;
        let connection = self.connection()?;
        let session = find(&mut self.sessions, id)?;
        send_delete(
            &connection,
            generation,
            DeleteSessionRequest::new(id.clone()),
        )?;
        session.op = Some(Op::Delete);
        Ok(())
    }

    /// Ends a delete. On success, removes the session, sends
    /// `session_removed` to its subscribers, and `session_deleted` to every
    /// watcher. On failure, releases the operation guard and the session
    /// stays. A result from an earlier generation, or for a session that is
    /// gone, is ignored.
    pub fn finish_delete(&mut self, id: &SessionId, generation: u64, result: Result<(), String>) {
        if generation != self.generation {
            return;
        }
        let Some(position) = self.sessions.iter().position(|session| session.id == *id) else {
            return;
        };
        if result.is_err() {
            self.sessions[position].op = None;
            return;
        }
        let session = self.sessions.remove(position);
        let removed = event(Event::SessionRemoved {
            session: session.id.clone(),
        });
        for outbox in &session.subscribers {
            outbox.send(removed.clone());
        }
        broadcast(
            &mut self.watchers,
            Event::SessionDeleted {
                session: session.id,
            },
        );
    }

    /// Sets the config options from a `session/set_config_option` response.
    pub fn set_config_options(
        &mut self,
        id: &SessionId,
        config_options: Vec<SessionConfigOption>,
    ) -> anyhow::Result<()> {
        find(&mut self.sessions, id)?.set_config_options(config_options);
        Ok(())
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
        // A load has no turn to cancel yet.
        if !matches!(session.op, Some(Op::Prompt { .. })) {
            return Ok(());
        }
        cancel_turn(
            &mut self.watchers,
            session,
            &connection(&self.server)?,
            send_cancel,
        )
    }
}

impl Session {
    /// A saved session: `Idle`, not unread, and unloaded.
    fn new(id: SessionId, workspace: String) -> Session {
        Session {
            id,
            workspace,
            transcript: Vec::new(),
            loaded: false,
            title: None,
            updated_at: None,
            config_options: Vec::new(),
            op: None,
            subscribers: Vec::new(),
            status: Status::Idle { last_stop: None },
            unread: false,
            focus: Vec::new(),
            responders: HashMap::new(),
            cancelling: false,
        }
    }

    /// Ends the turn with its stop reason, or with an error message, which
    /// becomes a turn error entry and `Failed`. Remaining pending permission
    /// requests are dropped. The caller releases the operation guard and
    /// publishes the change.
    fn end_turn(&mut self, result: Result<StopReason, String>) {
        self.status = match result {
            Ok(stop) => Status::Idle {
                last_stop: Some(stop),
            },
            Err(message) => {
                self.append(Entry::TurnError {
                    message: message.clone(),
                });
                Status::Failed { message }
            }
        };
        // Dropping a responder sends no reply. The turn is over, so the server
        // no longer waits for one.
        self.responders.clear();
        self.cancelling = false;
        if self.focus.is_empty() {
            self.unread = true;
        }
    }

    /// Sends every subscriber a session snapshot, then answers each waiting
    /// `subscribe`.
    fn end_load(&mut self, waiting: Vec<(u64, Outbox)>) {
        let snapshot = self.snapshot();
        self.subscribers
            .retain(|outbox| outbox.send(snapshot.clone()));
        for (id, outbox) in waiting {
            respond(&outbox, id, Response::Done);
        }
    }

    fn snapshot(&self) -> Frame {
        event(Event::SessionSnapshot {
            session: self.id.clone(),
            transcript: self.transcript.clone(),
            config_options: self.config_options.clone(),
        })
    }

    /// Sets the config options and sends them to every subscriber. During a
    /// load nothing is sent, since the snapshot at its end carries them.
    fn set_config_options(&mut self, config_options: Vec<SessionConfigOption>) {
        self.config_options = config_options;
        if matches!(self.op, Some(Op::Load { .. })) {
            return;
        }
        let frame = event(Event::ConfigOptionsChanged {
            session: self.id.clone(),
            config_options: self.config_options.clone(),
        });
        self.subscribers.retain(|outbox| outbox.send(frame.clone()));
    }

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
            title: self.title.clone(),
            updated_at: self.updated_at.clone(),
        }
    }
}

/// Calls `send_cancel`, answers every pending permission request with
/// `Cancelled`, and sets `Working`. The operation guard stays held until the
/// prompt returns.
fn cancel_turn(
    watchers: &mut Vec<Outbox>,
    session: &mut Session,
    connection: &ConnectionTo<Agent>,
    send_cancel: impl FnOnce(&ConnectionTo<Agent>, &SessionId) -> agent_client_protocol::Result<()>,
) -> anyhow::Result<()> {
    send_cancel(connection, &session.id)?;
    session.answer_cancelled();
    session.cancelling = true;
    if session.status != Status::Working {
        session.status = Status::Working;
        publish(watchers, session);
    }
    Ok(())
}

fn server(server: &Result<Server, String>) -> anyhow::Result<&Server> {
    server
        .as_ref()
        .map_err(|reason| anyhow!("no ACP connection: {reason}"))
}

fn connection(result: &Result<Server, String>) -> anyhow::Result<ConnectionTo<Agent>> {
    Ok(server(result)?.connection.clone())
}

/// `session/load` for the session, with its workspace path as `cwd`.
fn load_request(workspaces: &[Workspace], session: &Session) -> LoadSessionRequest {
    let workspace = workspaces
        .iter()
        .find(|workspace| workspace.name == session.workspace)
        .expect("every session's workspace exists");
    LoadSessionRequest::new(session.id.clone(), workspace.path.clone())
}

/// Sets `field` and returns whether it changed.
fn replace(field: &mut Option<String>, value: Option<String>) -> bool {
    let changed = *field != value;
    *field = value;
    changed
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

/// Queues the response to request `id`.
pub fn respond(outbox: &Outbox, id: u64, response: Response) {
    outbox.send(Frame::json(&DaemonMessage::Response { id, response }));
}
