use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};
use tokio_util::codec::Framed;

use crate::frame::{Frame, FrameCodec};
use crate::protocol::{ClientMessage, DaemonMessage, Event, Request, Response, TerminalId};

/// A daemon client: a cloneable handle over one socket connection. Dropping
/// every handle closes the connection.
#[derive(Clone)]
pub struct Client {
    outgoing: mpsc::UnboundedSender<Frame>,
    routes: Arc<Mutex<Routes>>,
    next_id: Arc<AtomicU64>,
}

/// Where the reader task delivers what the daemon sends.
#[derive(Default)]
struct Routes {
    pending: HashMap<u64, oneshot::Sender<Response>>,
    pty: HashMap<TerminalId, mpsc::UnboundedSender<Bytes>>,
    closed: bool,
}

impl Client {
    pub async fn connect(path: &Path) -> io::Result<Client> {
        let stream = UnixStream::connect(path).await?;
        let (mut sink, mut stream) = Framed::new(stream, FrameCodec).split();
        let (outgoing, mut receiver) = mpsc::unbounded_channel();
        let routes = Arc::new(Mutex::new(Routes::default()));

        tokio::spawn(async move {
            while let Some(frame) = receiver.recv().await {
                if sink.send(frame).await.is_err() {
                    break;
                }
            }
            // Shutting down the write half tells the daemon this daemon client
            // is gone, which ends the socket connection.
            let _ = sink.close().await;
        });

        let reader_routes = routes.clone();
        tokio::spawn(async move {
            while let Some(Ok(frame)) = stream.next().await {
                reader_routes.lock().unwrap().deliver(frame);
            }
            // Dropping the senders fails pending requests and closes every
            // `pty` receiver.
            let mut routes = reader_routes.lock().unwrap();
            *routes = Routes {
                closed: true,
                ..Routes::default()
            };
        });

        Ok(Client {
            outgoing,
            routes,
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    pub async fn request(&self, request: Request) -> io::Result<Response> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        {
            let mut routes = self.routes.lock().unwrap();
            if routes.closed {
                return Err(closed());
            }
            routes.pending.insert(id, sender);
        }
        self.send(Frame::json(&ClientMessage { id, request }))?;
        receiver.await.map_err(|_| closed())
    }

    /// Receives the terminal's output until it exits or the socket connection
    /// ends. Call this before `attach_terminal` so the screen snapshot is not
    /// missed. A later call for the same terminal closes the earlier receiver.
    pub fn pty(&self, id: TerminalId) -> mpsc::UnboundedReceiver<Bytes> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut routes = self.routes.lock().unwrap();
        if !routes.closed {
            routes.pty.insert(id, sender);
        }
        receiver
    }

    pub fn pty_input(&self, id: TerminalId, bytes: Bytes) -> io::Result<()> {
        self.send(Frame::Pty { id, bytes })
    }

    fn send(&self, frame: Frame) -> io::Result<()> {
        self.outgoing.send(frame).map_err(|_| closed())
    }
}

impl Routes {
    fn deliver(&mut self, frame: Frame) {
        match frame {
            Frame::Json(json) => match serde_json::from_slice(&json) {
                Ok(DaemonMessage::Response { id, response }) => {
                    if let Some(sender) = self.pending.remove(&id) {
                        let _ = sender.send(response);
                    }
                }
                Ok(DaemonMessage::Event {
                    event: Event::TerminalExited { terminal },
                }) => {
                    self.pty.remove(&terminal);
                }
                Err(error) => eprintln!("ur-client: bad message from the daemon: {error}"),
            },
            Frame::Pty { id, bytes } => {
                if let Some(sender) = self.pty.get(&id)
                    && sender.send(bytes).is_err()
                {
                    self.pty.remove(&id);
                }
            }
        }
    }
}

fn closed() -> io::Error {
    io::Error::new(
        io::ErrorKind::ConnectionAborted,
        "the connection to the daemon closed",
    )
}
