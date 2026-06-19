//! HTTP execution wired to the shared executor: DeepSeek's retry policy and
//! error mapping.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use ur_core::provider::RawEvent;
use ur_core::{BoxStream, Error, Result};
use ur_openai_compat::executor::{Dialect, error_body};
use ur_openai_compat::sse::DecodeChunk;

use crate::client::Config;

pub(crate) fn chat(config: Arc<Config>, body: Value) -> BoxStream<'static, Result<RawEvent>> {
    ur_openai_compat::executor::chat(config, body)
}

impl Dialect for Config {
    fn http(&self) -> &reqwest::Client {
        &self.http.client
    }

    fn api_key(&self) -> &str {
        &self.api_key
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn max_retries(&self) -> u32 {
        self.max_retries
    }

    fn decode_chunk(&self) -> DecodeChunk {
        crate::sse::decode_chunk
    }

    fn provider_name(&self) -> &'static str {
        "DeepSeek"
    }

    fn is_retryable_status(&self, status: u16) -> bool {
        matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
    }

    async fn status_error(
        &self,
        response: reqwest::Response,
        retry_after: Option<Duration>,
    ) -> Error {
        let status = response.status().as_u16();
        let message = error_body(response).await.message;

        match status {
            400 => Error::BadRequest { message },
            401 => Error::Auth,
            402 => Error::InsufficientFunds,
            422 => Error::InvalidParams { message },
            429 => Error::RateLimited { retry_after },
            _ => Error::Server { status, message },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt as _;
    use serde_json::json;
    use std::sync::atomic::Ordering;
    use ur_core::provider::Provider;
    use ur_core::provider::{Message, Request, Settings};
    use ur_openai_compat::test_support::flaky_body_server;
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
}
