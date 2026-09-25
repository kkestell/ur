use std::sync::{Arc, Mutex};

use futures::{SinkExt, StreamExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;
use ur_client::{ClientMessage, DaemonMessage, Frame, FrameCodec, Request, Response};

use super::ops;
use super::state::State;
use super::terminal::Terminals;

const OUTBOX_CAPACITY: usize = 1024;

pub async fn serve(
    listener: UnixListener,
    terminals: Arc<Terminals>,
    state: Arc<Mutex<State>>,
) -> anyhow::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(connection(stream, terminals.clone(), state.clone()));
    }
}

/// The frames waiting to be written to one socket connection.
#[derive(Clone)]
pub struct Outbox {
    sender: mpsc::Sender<Frame>,
    cancel: CancellationToken,
}

impl Outbox {
    /// Queues a frame. Returns false when the socket connection is gone or
    /// being closed, so the caller drops this outbox. A daemon client that
    /// cannot keep up fills its outbox, and that closes its socket connection.
    pub fn send(&self, frame: Frame) -> bool {
        match self.sender.try_send(frame) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.cancel.cancel();
                false
            }
            Err(TrySendError::Closed(_)) => false,
        }
    }

    pub fn same_connection(&self, other: &Outbox) -> bool {
        self.sender.same_channel(&other.sender)
    }
}

async fn connection(stream: UnixStream, terminals: Arc<Terminals>, state: Arc<Mutex<State>>) {
    let (mut sink, mut stream) = Framed::new(stream, FrameCodec).split();
    let (sender, mut receiver) = mpsc::channel(OUTBOX_CAPACITY);
    let cancel = CancellationToken::new();
    let outbox = Outbox {
        sender,
        cancel: cancel.clone(),
    };

    let reader = async {
        while let Some(frame) = stream.next().await {
            let frame = match frame {
                Ok(frame) => frame,
                Err(error) => {
                    eprintln!("ur daemon: closing a socket connection: {error}");
                    return;
                }
            };
            match frame {
                Frame::Json(json) => match serde_json::from_slice::<ClientMessage>(&json) {
                    Ok(ClientMessage { id, request }) => {
                        if let Some(response) = handle(id, request, &terminals, &state, &outbox) {
                            outbox.send(Frame::json(&DaemonMessage::Response { id, response }));
                        }
                    }
                    Err(error) => {
                        eprintln!("ur daemon: closing a socket connection: bad request: {error}");
                        return;
                    }
                },
                Frame::Pty { id, bytes } => {
                    if let Err(error) = terminals.write(id, &bytes) {
                        eprintln!("ur daemon: dropping input: {error:#}");
                    }
                }
            }
        }
    };
    let writer = async {
        while let Some(frame) = receiver.recv().await {
            if sink.send(frame).await.is_err() {
                return;
            }
        }
    };
    tokio::select! {
        () = reader => {}
        () = writer => {}
        () = cancel.cancelled() => {}
    }
}

/// Answers request `id`, or returns `None` when the op answers it later.
fn handle(
    id: u64,
    request: Request,
    terminals: &Arc<Terminals>,
    state: &Arc<Mutex<State>>,
    outbox: &Outbox,
) -> Option<Response> {
    let result = match request {
        Request::OpenTerminal => terminals
            .open()
            .map(|terminal| Response::Opened { terminal }),
        Request::AttachTerminal {
            terminal,
            rows,
            cols,
        } => terminals
            .attach(terminal, rows, cols, outbox.clone())
            .map(|()| Response::Done),
        Request::TerminalResize {
            terminal,
            rows,
            cols,
        } => terminals
            .resize(terminal, rows, cols)
            .map(|()| Response::Done),
        Request::NewSession { path } => match ops::new_session(state, path, id, outbox.clone()) {
            Ok(()) => return None,
            Err(error) => Err(error),
        },
        Request::Subscribe { session } => state
            .lock()
            .unwrap()
            .subscribe(&session, outbox.clone())
            .map(|()| Response::Done),
        Request::Prompt { session, content } => ops::prompt(state, session, content),
    };
    Some(result.unwrap_or_else(|error| Response::Error {
        message: format!("{error:#}"),
    }))
}
