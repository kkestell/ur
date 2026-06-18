//! OpenAI [`Provider`] implementation for `ur`.
//!
//! This crate provides [`OpenAiClient`], its builder, and Chat Completions
//! streaming support. Most applications reach it through the `ur` facade as
//! `ur::openai` with the `openai` feature enabled.
//!
//! See the repository
//! [OpenAI provider specification](https://github.com/kkestell/ur/blob/main/docs/providers/openai.md)
//! for the wire mapping, generation-setting semantics, strict mode, and retry
//! behavior this crate implements.

#![forbid(unsafe_code)]

mod client;
mod executor;
mod request;
mod sse;

pub use client::{OpenAiClient, OpenAiClientBuilder, OpenAiHttpClient};

use ur_core::provider::{ModelSpec, Provider, RawEvent, Request};
use ur_core::{BoxStream, Result};

impl Provider for OpenAiClient {
    fn chat(&self, request: &Request) -> BoxStream<'static, Result<RawEvent>> {
        let config = self.config();
        match request::encode(request, config.user.as_deref()) {
            Ok(body) => executor::chat(self.shared_config(), body),
            Err(error) => Box::pin(futures_util::stream::once(async move { Err(error) })),
        }
    }

    fn model_spec(&self, _model_id: &str) -> Option<ModelSpec> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_is_a_provider() {
        fn assert_provider<P: Provider>() {}

        assert_provider::<OpenAiClient>();
    }

    #[test]
    fn catalog_is_empty_for_v1() {
        let client = OpenAiClient::new("key");
        assert_eq!(client.model_spec("gpt-5.5"), None);
        assert_eq!(client.model_notice("gpt-5.5"), None);
    }
}
