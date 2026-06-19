//! Test scaffolding shared by the provider crates' unit tests, gated behind the
//! `test-support` feature so it is compiled only for those test builds.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use ur_core::Error;
use ur_core::provider::RawEvent;

use crate::sse::{CompletionState, DecodeChunk, Frame, SseItem};

/// Folds a batch of framed SSE events through `decode` and a [`CompletionState`],
/// appending the resulting [`RawEvent`]s to `events`.
pub fn drive(
    decode: DecodeChunk,
    state: &mut CompletionState,
    events: &mut Vec<RawEvent>,
    frames: Vec<Frame>,
) -> Result<(), Error> {
    for frame in frames {
        match frame {
            Frame::Done => events.extend(state.apply(SseItem::Done)?),
            Frame::Data(data) => {
                for item in decode(&data)? {
                    events.extend(state.apply(item)?);
                }
            }
        }
    }
    Ok(())
}

/// Spawns a minimal HTTP server that cuts the response body off mid-event on the
/// first connection and serves `success_body` in full on the second, exercising
/// the executor's mid-stream retry path. Returns the base URL and a counter of
/// observed requests.
pub async fn flaky_body_server(success_body: String) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&requests);

    tokio::spawn(async move {
        for attempt in 0..2 {
            let (mut socket, _) = listener.accept().await.unwrap();
            observed.fetch_add(1, Ordering::Relaxed);
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            loop {
                let n = socket.read(&mut buffer).await.unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            if attempt == 0 {
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: 100\r\n\r\ndata: ",
                    )
                    .await
                    .unwrap();
            } else {
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{}",
                    success_body.len(),
                    success_body
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        }
    });

    (format!("http://{address}"), requests)
}
