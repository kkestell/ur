//! HTTP execution, retry policy, and provider error mapping.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::TryStreamExt;
use reqwest::header::RETRY_AFTER;
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
    Reading(BoxStream<'static, std::result::Result<Vec<u8>, reqwest::Error>>),
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
                        this.state = State::Reading(Box::pin(
                            response.bytes_stream().map_ok(|bytes| bytes.to_vec()),
                        ));
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
        let result = config
            .http
            .client
            .post(url.clone())
            .bearer_auth(&config.api_key)
            .timeout(config.timeout)
            .json(&body)
            .send()
            .await;

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
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
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

async fn status_error(response: reqwest::Response, retry_after: Option<Duration>) -> Error {
    let status = response.status().as_u16();
    let message = error_message(response, status).await;

    match status {
        400 => Error::BadRequest { message },
        401 => Error::Auth,
        402 => Error::InsufficientFunds,
        422 => Error::InvalidParams { message },
        429 => Error::RateLimited { retry_after },
        _ => Error::Server { status, message },
    }
}

async fn error_message(response: reqwest::Response, status: u16) -> String {
    let default = response
        .status()
        .canonical_reason()
        .unwrap_or("provider error")
        .to_owned();

    let Ok(body) = response.text().await else {
        return default;
    };

    if body.trim().is_empty() {
        return default;
    }

    serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or(body)
        .trim()
        .to_owned()
        .or_else_default(status)
}

trait DefaultIfEmpty {
    fn or_else_default(self, status: u16) -> String;
}

impl DefaultIfEmpty for String {
    fn or_else_default(self, status: u16) -> String {
        if self.is_empty() {
            reqwest::StatusCode::from_u16(status)
                .ok()
                .and_then(|status| status.canonical_reason().map(str::to_owned))
                .unwrap_or_else(|| "provider error".to_owned())
        } else {
            self
        }
    }
}

fn transport(error: reqwest::Error) -> Error {
    Error::Transport(Box::new(error))
}

fn unexpected_eof() -> Error {
    Error::Decode {
        context: "reading DeepSeek SSE stream".to_owned(),
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use ur_core::provider::Provider;
    use ur_core::provider::{Message, Request, Settings};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request() -> Request {
        serde_json::from_value(json!({
            "model": "deepseek-v4-pro",
            "messages": serde_json::to_value(vec![Message::user("hello")]).unwrap(),
            "tools": [],
            "settings": serde_json::to_value(Settings::default()).unwrap(),
        }))
        .unwrap()
    }

    async fn client(server: &MockServer) -> crate::DeepSeekClient {
        crate::DeepSeekClient::builder()
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

    async fn collect(client: &crate::DeepSeekClient) -> Vec<Result<RawEvent>> {
        client.chat(&request()).collect().await
    }

    #[tokio::test]
    async fn sends_headers_path_and_body_and_streams_events() {
        let server = MockServer::start().await;
        let expected_body = json!({
            "model": "deepseek-v4-pro",
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
    async fn retries_retryable_statuses_then_succeeds() {
        for status in [408, 429, 500, 502, 503, 504] {
            let server = MockServer::start().await;
            let mut failure = ResponseTemplate::new(status).set_body_json(json!({
                "error": { "message": "temporarily down" }
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
                .respond_with(sse(&format!(
                    "{}data: [DONE]\n\n",
                    chunk(json!({
                        "choices": [{ "delta": {}, "finish_reason": "stop" }],
                        "usage": null
                    }))
                )))
                .expect(1)
                .mount(&server)
                .await;

            let client = crate::DeepSeekClient::builder()
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
                    "error": { "message": "still down" }
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
    async fn retries_body_transport_failure_before_any_event() {
        let valid_body = format!(
            "{}data: [DONE]\n\n",
            chunk(json!({
                "choices": [{ "delta": {}, "finish_reason": "stop" }],
                "usage": null
            }))
        );
        let (base_url, requests) = flaky_body_server(valid_body).await;
        let client = crate::DeepSeekClient::builder()
            .api_key("test-key")
            .base_url(base_url)
            .max_retries(1)
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
        assert_eq!(requests.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn unmapped_status_maps_to_non_retryable_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(418).set_body_string("short and stout"))
            .expect(1)
            .mount(&server)
            .await;

        let client = crate::DeepSeekClient::builder()
            .api_key("test-key")
            .base_url(server.uri())
            .max_retries(2)
            .build()
            .unwrap();

        match collect(&client).await.pop().unwrap() {
            Err(Error::Server { status, message }) => {
                assert_eq!(status, 418);
                assert_eq!(message, "short and stout");
            }
            other => panic!("expected server error, got {other:?}"),
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
            (422, "params", "bad param"),
        ];

        for (status, expected, message) in cases {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                    "error": { "message": message }
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
                    assert_eq!(message, "bad param");
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
                assert_eq!(context, "decoding DeepSeek SSE chunk");
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
                assert_eq!(context, "reading DeepSeek SSE stream");
                assert!(source.to_string().contains("[DONE]"));
            }
            other => panic!("expected decode error, got {other:?}"),
        }
    }

    async fn flaky_body_server(success_body: String) -> (String, Arc<AtomicUsize>) {
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
}
