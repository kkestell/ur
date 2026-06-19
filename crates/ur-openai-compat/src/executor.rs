//! The shared HTTP execution and retry state machine for OpenAI-compatible
//! providers. Each provider implements [`Dialect`] to supply its connection
//! config, request decoration, retry policy, error mapping, and chunk decoder.

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use reqwest::RequestBuilder;
use reqwest::header::RETRY_AFTER;
use serde::Deserialize;
use serde_json::Value;
use ur_core::provider::RawEvent;
use ur_core::{BoxFuture, BoxStream, Error, Result, Stream};

use crate::sse::{CompletionState, DecodeChunk, Frame, SseDecoder, SseItem};

/// The per-provider seam over the shared Chat Completions stream executor.
pub trait Dialect: Send + Sync + 'static {
    /// The connection pool to issue requests on.
    fn http(&self) -> &reqwest::Client;
    /// The bearer API key.
    fn api_key(&self) -> &str;
    /// The base URL the `chat/completions` endpoint is resolved against.
    fn base_url(&self) -> &str;
    /// The per-request timeout.
    fn timeout(&self) -> Duration;
    /// The maximum number of automatic retries.
    fn max_retries(&self) -> u32;
    /// The provider's chunk decoder.
    fn decode_chunk(&self) -> DecodeChunk;
    /// The provider's display name (e.g. `"DeepSeek"`), woven into the decode
    /// errors raised when a stream ends without a `finish_reason` or before
    /// `[DONE]`.
    fn provider_name(&self) -> &'static str;

    /// Decorates the outgoing request (e.g. with attribution headers). Bearer
    /// auth is already applied.
    fn decorate(&self, builder: RequestBuilder) -> RequestBuilder {
        builder
    }

    /// Whether a non-success status should be retried.
    fn is_retryable_status(&self, status: u16) -> bool;

    /// Maps a non-success response to an [`Error`].
    fn status_error(
        &self,
        response: reqwest::Response,
        retry_after: Option<Duration>,
    ) -> impl Future<Output = Error> + Send;
}

/// Streams a Chat Completions request to completion under `config`.
pub fn chat<D: Dialect>(config: Arc<D>, body: Value) -> BoxStream<'static, Result<RawEvent>> {
    let completion = CompletionState::new(config.provider_name());
    Box::pin(ChatStream {
        state: State::Connecting(Box::pin(connect(Arc::clone(&config), body.clone()))),
        config,
        body,
        decoder: SseDecoder::default(),
        completion,
        ready: VecDeque::new(),
        stream_retries: 0,
        emitted_events: false,
    })
}

struct ChatStream<D: Dialect> {
    state: State,
    config: Arc<D>,
    body: Value,
    decoder: SseDecoder,
    completion: CompletionState,
    ready: VecDeque<RawEvent>,
    stream_retries: u32,
    emitted_events: bool,
}

enum State {
    Connecting(BoxFuture<'static, Result<reqwest::Response>>),
    Reading(BoxStream<'static, std::result::Result<Bytes, reqwest::Error>>),
    Done,
}

impl<D: Dialect> ChatStream<D> {
    fn can_retry_stream_error(&self) -> bool {
        !self.emitted_events && self.stream_retries < self.config.max_retries()
    }

    fn retry_after_stream_error(&mut self) {
        self.stream_retries += 1;
        self.decoder = SseDecoder::default();
        self.completion = CompletionState::new(self.config.provider_name());
        self.ready.clear();
        self.state = State::Connecting(Box::pin(connect_after_delay(
            Arc::clone(&self.config),
            self.body.clone(),
            self.stream_retries - 1,
        )));
    }
}

impl<D: Dialect> Stream for ChatStream<D> {
    type Item = Result<RawEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        let decode = this.config.decode_chunk();

        loop {
            if let Some(event) = this.ready.pop_front() {
                this.emitted_events = true;
                return Poll::Ready(Some(Ok(event)));
            }

            match &mut this.state {
                State::Connecting(future) => match future.as_mut().poll(cx) {
                    Poll::Ready(Ok(response)) => {
                        this.state = State::Reading(Box::pin(response.bytes_stream()));
                    }
                    Poll::Ready(Err(error)) => {
                        this.state = State::Done;
                        return Poll::Ready(Some(Err(error)));
                    }
                    Poll::Pending => return Poll::Pending,
                },
                State::Reading(stream) => match stream.as_mut().poll_next(cx) {
                    Poll::Ready(Some(Ok(bytes))) => {
                        if let Err(error) = push_items(
                            &mut this.decoder,
                            &mut this.completion,
                            &mut this.ready,
                            decode,
                            &bytes,
                        ) {
                            this.state = State::Done;
                            return Poll::Ready(Some(Err(error)));
                        }

                        if this.completion.is_done() {
                            this.state = State::Done;
                        }
                    }
                    Poll::Ready(Some(Err(_error))) if this.can_retry_stream_error() => {
                        this.retry_after_stream_error();
                    }
                    Poll::Ready(Some(Err(error))) => {
                        this.state = State::Done;
                        return Poll::Ready(Some(Err(transport(error))));
                    }
                    Poll::Ready(None) => {
                        match finish_items(
                            &mut this.decoder,
                            &mut this.completion,
                            &mut this.ready,
                            decode,
                        ) {
                            Ok(()) if this.completion.is_done() => {
                                this.state = State::Done;
                            }
                            Ok(()) => {
                                this.state = State::Done;
                                return Poll::Ready(Some(Err(unexpected_eof(
                                    this.config.provider_name(),
                                ))));
                            }
                            Err(error) => {
                                this.state = State::Done;
                                return Poll::Ready(Some(Err(error)));
                            }
                        }
                    }
                    Poll::Pending => return Poll::Pending,
                },
                State::Done => return Poll::Ready(None),
            }
        }
    }
}

async fn connect_after_delay<D: Dialect>(
    config: Arc<D>,
    body: Value,
    retry_attempt: u32,
) -> Result<reqwest::Response> {
    sleep_before_retry(retry_attempt, None).await;
    connect(config, body).await
}

fn apply_frame(
    completion: &mut CompletionState,
    ready: &mut VecDeque<RawEvent>,
    decode: DecodeChunk,
    frame: Frame,
) -> Result<()> {
    match frame {
        Frame::Done => ready.extend(completion.apply(SseItem::Done)?),
        Frame::Data(data) => {
            for item in decode(&data)? {
                ready.extend(completion.apply(item)?);
            }
        }
    }
    Ok(())
}

fn push_items(
    decoder: &mut SseDecoder,
    completion: &mut CompletionState,
    ready: &mut VecDeque<RawEvent>,
    decode: DecodeChunk,
    bytes: &[u8],
) -> Result<()> {
    for frame in decoder.push(bytes)? {
        apply_frame(completion, ready, decode, frame)?;
    }
    Ok(())
}

fn finish_items(
    decoder: &mut SseDecoder,
    completion: &mut CompletionState,
    ready: &mut VecDeque<RawEvent>,
    decode: DecodeChunk,
) -> Result<()> {
    for frame in decoder.finish()? {
        apply_frame(completion, ready, decode, frame)?;
    }
    Ok(())
}

async fn connect<D: Dialect>(config: Arc<D>, body: Value) -> Result<reqwest::Response> {
    let url = endpoint_url(config.base_url())?;
    let mut attempt = 0;

    loop {
        let builder = config
            .http()
            .post(url.clone())
            .bearer_auth(config.api_key());
        let result = config
            .decorate(builder)
            .timeout(config.timeout())
            .json(&body)
            .send()
            .await;

        match result {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                let retry_after = retry_after(response.headers());
                let status = response.status().as_u16();
                let retryable = config.is_retryable_status(status);
                let error = config.status_error(response, retry_after).await;
                if retryable && attempt < config.max_retries() {
                    sleep_before_retry(attempt, retry_after).await;
                    attempt += 1;
                    continue;
                }
                return Err(error);
            }
            Err(error) => {
                if retryable_transport_error(&error) && attempt < config.max_retries() {
                    sleep_before_retry(attempt, None).await;
                    attempt += 1;
                    continue;
                }
                return Err(transport(error));
            }
        }
    }
}

fn endpoint_url(base_url: &str) -> Result<reqwest::Url> {
    let mut base = base_url.to_owned();
    if !base.ends_with('/') {
        base.push('/');
    }
    let parsed = reqwest::Url::parse(&base).map_err(|source| Error::Config {
        message: format!("base_url '{base_url}' is not a valid URL: {source}"),
    })?;
    parsed
        .join("chat/completions")
        .map_err(|source| Error::Config {
            message: format!("base_url '{base_url}' cannot form chat endpoint: {source}"),
        })
}

fn retryable_transport_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

async fn sleep_before_retry(attempt: u32, retry_after: Option<Duration>) {
    let backoff = Duration::from_millis(10 * 2_u64.pow(attempt.min(10)));
    tokio::time::sleep(retry_after.unwrap_or(backoff)).await;
}

/// Reads a numeric `Retry-After` header into a duration.
pub fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// A provider error body parsed from the canonical `{ "error": { "message",
/// "code" } }` envelope.
pub struct ErrorBody {
    pub message: String,
    pub code: Option<String>,
}

#[derive(Deserialize)]
struct WireErrorEnvelope {
    error: Option<WireError>,
}

#[derive(Deserialize)]
struct WireError {
    message: Option<String>,
    // `code` is a string for some providers (OpenAI's `insufficient_quota`) and
    // a number for others (OpenRouter's HTTP status); accept either and surface
    // only string codes.
    code: Option<Value>,
}

/// Extracts an [`ErrorBody`] from a non-success response: the JSON
/// `error.message` when present, otherwise the canonical status reason or the
/// raw body text.
pub async fn error_body(response: reqwest::Response) -> ErrorBody {
    let default = response
        .status()
        .canonical_reason()
        .unwrap_or("provider error")
        .to_owned();

    let Ok(text) = response.text().await else {
        return ErrorBody {
            message: default,
            code: None,
        };
    };

    if text.trim().is_empty() {
        return ErrorBody {
            message: default,
            code: None,
        };
    }

    if let Ok(envelope) = serde_json::from_str::<WireErrorEnvelope>(&text)
        && let Some(error) = envelope.error
    {
        let message = error.message.unwrap_or(default);
        let message = message.trim();
        return ErrorBody {
            message: if message.is_empty() {
                "provider error".to_owned()
            } else {
                message.to_owned()
            },
            code: error
                .code
                .and_then(|value| value.as_str().map(str::to_owned)),
        };
    }

    let message = text.trim();
    ErrorBody {
        message: if message.is_empty() {
            default
        } else {
            message.to_owned()
        },
        code: None,
    }
}

/// Wraps a transport error.
pub fn transport(error: reqwest::Error) -> Error {
    Error::Transport(Box::new(error))
}

fn unexpected_eof(provider: &str) -> Error {
    Error::Decode {
        context: format!("reading {provider} SSE stream"),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "SSE stream ended before data: [DONE]",
        )),
    }
}
