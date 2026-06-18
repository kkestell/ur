//! HTTP execution, retry policy, and provider error mapping.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use reqwest::header::RETRY_AFTER;
use serde::Deserialize;
use serde_json::Value;
use ur_core::provider::RawEvent;
use ur_core::{BoxFuture, BoxStream, Error, Result, Stream};

use crate::client::Config;
use crate::sse::{CompletionState, SseDecoder};

pub(crate) fn chat(config: Arc<Config>, body: Value) -> BoxStream<'static, Result<RawEvent>> {
    Box::pin(ChatStream {
        state: State::Connecting(Box::pin(connect(Arc::clone(&config), body.clone()))),
        config,
        body,
        decoder: SseDecoder::default(),
        completion: CompletionState::default(),
        ready: VecDeque::new(),
        stream_retries: 0,
        emitted_events: false,
    })
}

struct ChatStream {
    state: State,
    config: Arc<Config>,
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

impl ChatStream {
    fn can_retry_stream_error(&self) -> bool {
        !self.emitted_events && self.stream_retries < self.config.max_retries
    }

    fn retry_after_stream_error(&mut self) {
        self.stream_retries += 1;
        self.decoder = SseDecoder::default();
        self.completion = CompletionState::default();
        self.ready.clear();
        self.state = State::Connecting(Box::pin(connect_after_delay(
            Arc::clone(&self.config),
            self.body.clone(),
            self.stream_retries - 1,
        )));
    }
}

impl Stream for ChatStream {
    type Item = Result<RawEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

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
                        match finish_items(&mut this.decoder, &mut this.completion, &mut this.ready)
                        {
                            Ok(()) if this.completion.is_done() => {
                                this.state = State::Done;
                            }
                            Ok(()) => {
                                this.state = State::Done;
                                return Poll::Ready(Some(Err(unexpected_eof())));
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

async fn connect_after_delay(
    config: Arc<Config>,
    body: Value,
    retry_attempt: u32,
) -> Result<reqwest::Response> {
    sleep_before_retry(retry_attempt, None).await;
    connect(config, body).await
}

fn push_items(
    decoder: &mut SseDecoder,
    completion: &mut CompletionState,
    ready: &mut VecDeque<RawEvent>,
    bytes: &[u8],
) -> Result<()> {
    for item in decoder.push(bytes)? {
        ready.extend(completion.apply(item)?);
    }
    Ok(())
}

fn finish_items(
    decoder: &mut SseDecoder,
    completion: &mut CompletionState,
    ready: &mut VecDeque<RawEvent>,
) -> Result<()> {
    for item in decoder.finish()? {
        ready.extend(completion.apply(item)?);
    }
    Ok(())
}

async fn connect(config: Arc<Config>, body: Value) -> Result<reqwest::Response> {
    let url = endpoint_url(&config)?;
    let mut attempt = 0;

    loop {
        let mut builder = config
            .http
            .client
            .post(url.clone())
            .bearer_auth(&config.api_key);
        // OpenRouter attributes requests to an app via these optional headers.
        if let Some(referer) = &config.referer {
            builder = builder.header("HTTP-Referer", referer);
        }
        if let Some(title) = &config.title {
            builder = builder.header("X-Title", title);
        }
        let result = builder.timeout(config.timeout).json(&body).send().await;

        match result {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                let retry_after = retry_after(response.headers());
                let status = response.status().as_u16();
                let retryable = retryable_status(status);
                let error = status_error(response, retry_after).await;
                if retryable && attempt < config.max_retries {
                    sleep_before_retry(attempt, retry_after).await;
                    attempt += 1;
                    continue;
                }
                return Err(error);
            }
            Err(error) => {
                if retryable_transport_error(&error) && attempt < config.max_retries {
                    sleep_before_retry(attempt, None).await;
                    attempt += 1;
                    continue;
                }
                return Err(transport(error));
            }
        }
    }
}

fn endpoint_url(config: &Config) -> Result<reqwest::Url> {
    let mut base = config.base_url.clone();
    if !base.ends_with('/') {
        base.push('/');
    }
    let base = reqwest::Url::parse(&base).map_err(|source| Error::Config {
        message: format!(
            "base_url '{}' is not a valid URL: {source}",
            config.base_url
        ),
    })?;
    base.join("chat/completions")
        .map_err(|source| Error::Config {
            message: format!(
                "base_url '{}' cannot form chat endpoint: {source}",
                config.base_url
            ),
        })
}

fn retryable_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 429 | 500 | 502 | 503 | 504)
}

fn retryable_transport_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect()
}

async fn sleep_before_retry(attempt: u32, retry_after: Option<Duration>) {
    let backoff = Duration::from_millis(10 * 2_u64.pow(attempt.min(10)));
    tokio::time::sleep(retry_after.unwrap_or(backoff)).await;
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

// OpenRouter maps `402` to depleted credits and `403` to moderation/guardrail
// blocks; the rejection reason rides along in the error message.
async fn status_error(response: reqwest::Response, retry_after: Option<Duration>) -> Error {
    let status = response.status().as_u16();
    let body = error_body(response).await;
    let message = body.message;

    match status {
        400 => Error::BadRequest { message },
        401 => Error::Auth,
        402 => Error::InsufficientFunds,
        403 => Error::Auth,
        404 | 422 => Error::InvalidParams { message },
        429 => Error::RateLimited { retry_after },
        _ => Error::Server { status, message },
    }
}

#[derive(Default)]
struct ProviderErrorBody {
    message: String,
}

#[derive(Deserialize)]
struct WireErrorEnvelope {
    error: Option<WireError>,
}

#[derive(Deserialize)]
struct WireError {
    message: Option<String>,
}

async fn error_body(response: reqwest::Response) -> ProviderErrorBody {
    let default = response
        .status()
        .canonical_reason()
        .unwrap_or("provider error")
        .to_owned();

    let Ok(text) = response.text().await else {
        return ProviderErrorBody { message: default };
    };

    if text.trim().is_empty() {
        return ProviderErrorBody { message: default };
    }

    if let Ok(envelope) = serde_json::from_str::<WireErrorEnvelope>(&text)
        && let Some(error) = envelope.error
    {
        let message = error.message.unwrap_or(default);
        let message = message.trim();
        return ProviderErrorBody {
            message: if message.is_empty() {
                "provider error".to_owned()
            } else {
                message.to_owned()
            },
        };
    }

    let message = text.trim();
    ProviderErrorBody {
        message: if message.is_empty() {
            default
        } else {
            message.to_owned()
        },
    }
}

fn transport(error: reqwest::Error) -> Error {
    Error::Transport(Box::new(error))
}

fn unexpected_eof() -> Error {
    Error::Decode {
        context: "reading OpenRouter SSE stream".to_owned(),
        source: Box::new(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "SSE stream ended before data: [DONE]",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt as _;
    use serde_json::json;
    use ur_core::provider::Provider;
    use ur_core::provider::{Message, Request, Settings};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request() -> Request {
        serde_json::from_value(json!({
            "model": "openai/gpt-5.5",
            "messages": serde_json::to_value(vec![Message::user("hello")]).unwrap(),
            "tools": [],
            "settings": serde_json::to_value(Settings::default()).unwrap(),
        }))
        .unwrap()
    }

    async fn client(server: &MockServer) -> crate::OpenRouterClient {
        crate::OpenRouterClient::builder()
            .api_key("test-key")
            .base_url(server.uri())
            .max_retries(0)
            .build()
            .unwrap()
    }

    fn sse(data: &str) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(data)
    }

    fn chunk(value: serde_json::Value) -> String {
        format!("data: {value}\n\n")
    }

    fn stop_stream() -> String {
        format!(
            "{}data: [DONE]\n\n",
            chunk(json!({
                "choices": [{ "delta": {}, "finish_reason": "stop" }],
                "usage": null
            }))
        )
    }

    async fn collect(client: &crate::OpenRouterClient) -> Vec<Result<RawEvent>> {
        client.chat(&request()).collect().await
    }

    #[tokio::test]
    async fn sends_headers_path_and_body_and_streams_events() {
        let server = MockServer::start().await;
        let expected_body = json!({
            "model": "openai/gpt-5.5",
            "messages": [{ "role": "user", "content": "hello" }],
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        let body = format!(
            "{}{}data: [DONE]\n\n",
            chunk(json!({
                "choices": [{
                    "delta": { "content": "hello" },
                    "finish_reason": null
                }],
                "usage": null
            })),
            chunk(json!({
                "choices": [{
                    "delta": {},
                    "finish_reason": "stop"
                }],
                "usage": null
            })),
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("authorization", "Bearer test-key"))
            .and(body_json(expected_body))
            .respond_with(sse(&body))
            .expect(1)
            .mount(&server)
            .await;

        let client = client(&server).await;
        let events = collect(&client).await;

        assert_eq!(
            events.into_iter().collect::<Result<Vec<_>>>().unwrap(),
            vec![
                RawEvent::TextDelta("hello".to_owned()),
                RawEvent::Done {
                    finish_reason: ur_core::event::FinishReason::Stop,
                    usage: None,
                },
            ]
        );
    }

    #[tokio::test]
    async fn attribution_headers_are_sent_when_configured() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(header("http-referer", "https://example.com"))
            .and(header("x-title", "Example App"))
            .respond_with(sse(&stop_stream()))
            .expect(1)
            .mount(&server)
            .await;

        let client = crate::OpenRouterClient::builder()
            .api_key("test-key")
            .base_url(server.uri())
            .referer("https://example.com")
            .title("Example App")
            .max_retries(0)
            .build()
            .unwrap();

        let events = collect(&client).await;
        assert_eq!(
            events.into_iter().collect::<Result<Vec<_>>>().unwrap(),
            vec![RawEvent::Done {
                finish_reason: ur_core::event::FinishReason::Stop,
                usage: None,
            }]
        );
    }

    #[tokio::test]
    async fn provider_routing_is_included_in_the_body() {
        use crate::ProviderRouting;

        let server = MockServer::start().await;
        let expected_body = json!({
            "model": "openai/gpt-5.5",
            "messages": [{ "role": "user", "content": "hello" }],
            "stream": true,
            "stream_options": { "include_usage": true },
            "provider": { "order": ["openai"], "allow_fallbacks": false },
        });
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_json(expected_body))
            .respond_with(sse(&stop_stream()))
            .expect(1)
            .mount(&server)
            .await;

        let client = crate::OpenRouterClient::builder()
            .api_key("test-key")
            .base_url(server.uri())
            .provider_routing(ProviderRouting {
                order: vec!["openai".to_owned()],
                allow_fallbacks: Some(false),
                ..Default::default()
            })
            .max_retries(0)
            .build()
            .unwrap();

        let events = collect(&client).await;
        assert!(
            events
                .into_iter()
                .collect::<Result<Vec<_>>>()
                .unwrap()
                .contains(&RawEvent::Done {
                    finish_reason: ur_core::event::FinishReason::Stop,
                    usage: None,
                })
        );
    }

    #[tokio::test]
    async fn retries_retryable_statuses_then_succeeds() {
        for status in [408, 429, 500, 502, 503, 504] {
            let server = MockServer::start().await;
            let mut failure = ResponseTemplate::new(status).set_body_json(json!({
                "error": { "code": status, "message": "temporarily down" }
            }));
            if status == 429 {
                failure = failure.insert_header("retry-after", "0");
            }

            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(failure)
                .up_to_n_times(1)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(sse(&stop_stream()))
                .expect(1)
                .mount(&server)
                .await;

            let client = crate::OpenRouterClient::builder()
                .api_key("test-key")
                .base_url(server.uri())
                .max_retries(1)
                .build()
                .unwrap();

            let events = collect(&client).await;
            assert_eq!(
                events.into_iter().collect::<Result<Vec<_>>>().unwrap(),
                vec![RawEvent::Done {
                    finish_reason: ur_core::event::FinishReason::Stop,
                    usage: None,
                }],
                "status {status} should be retried"
            );
        }
    }

    #[tokio::test]
    async fn retryable_server_statuses_map_after_retries_are_exhausted() {
        for status in [408, 500, 502, 503, 504] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                    "error": { "code": status, "message": "still down" }
                })))
                .expect(1)
                .mount(&server)
                .await;

            let client = client(&server).await;
            match collect(&client).await.pop().unwrap() {
                Err(Error::Server {
                    status: actual,
                    message,
                }) => {
                    assert_eq!(actual, status);
                    assert_eq!(message, "still down");
                }
                other => panic!("expected server error for {status}, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn retry_after_is_preserved_on_final_rate_limit() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("retry-after", "7")
                    .set_body_json(json!({ "error": { "message": "slow down" } })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = client(&server).await;
        match collect(&client).await.pop().unwrap() {
            Err(Error::RateLimited { retry_after }) => {
                assert_eq!(retry_after, Some(Duration::from_secs(7)));
            }
            other => panic!("expected rate limit error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_retryable_statuses_map_immediately() {
        let cases = [
            (400, "bad request", "bad input"),
            (401, "auth", "ignored"),
            (402, "funds", "ignored"),
            (403, "auth", "moderation"),
            (404, "params", "missing"),
            (422, "params", "bad param"),
        ];

        for (status, expected, message) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                    "error": { "code": status, "message": message }
                })))
                .expect(1)
                .mount(&server)
                .await;

            let client = client(&server).await;
            match collect(&client).await.pop().unwrap() {
                Err(Error::BadRequest { message }) => {
                    assert_eq!(expected, "bad request");
                    assert_eq!(message, "bad input");
                }
                Err(Error::Auth) => assert_eq!(expected, "auth"),
                Err(Error::InsufficientFunds) => assert_eq!(expected, "funds"),
                Err(Error::InvalidParams { message }) => {
                    assert_eq!(expected, "params");
                    assert!(matches!(message.as_str(), "missing" | "bad param"));
                }
                other => panic!("unexpected error mapping for {status}: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn malformed_stream_yields_decode_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(sse("data: {not json}\n\n"))
            .mount(&server)
            .await;

        let client = client(&server).await;
        match collect(&client).await.pop().unwrap() {
            Err(Error::Decode { context, .. }) => {
                assert_eq!(context, "decoding OpenRouter SSE chunk");
            }
            other => panic!("expected decode error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn eof_before_done_yields_decode_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(sse(&chunk(json!({
                "choices": [{ "delta": {}, "finish_reason": "stop" }],
                "usage": null
            }))))
            .mount(&server)
            .await;

        let client = client(&server).await;
        match collect(&client).await.pop().unwrap() {
            Err(Error::Decode { context, source }) => {
                assert_eq!(context, "reading OpenRouter SSE stream");
                assert!(source.to_string().contains("[DONE]"));
            }
            other => panic!("expected decode error, got {other:?}"),
        }
    }
}
