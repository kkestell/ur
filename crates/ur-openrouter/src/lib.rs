//! OpenRouter [`Provider`] implementation for `ur`.
//!
//! This crate provides [`OpenRouterClient`], its builder, and Chat Completions
//! streaming support for [OpenRouter](https://openrouter.ai), an OpenAI-compatible
//! aggregator fronting many model providers. Most applications reach it through
//! the `ur` facade as `ur::openrouter` with the `openrouter` feature enabled.
//!
//! See the repository
//! [OpenRouter provider specification](https://github.com/kkestell/ur/blob/main/docs/providers/openrouter.md)
//! for the wire mapping, generation-setting semantics, attribution headers,
//! provider routing, and retry behavior this crate implements.

#![forbid(unsafe_code)]

mod client;
mod executor;
mod request;
mod sse;

pub use client::{
    OpenRouterClient, OpenRouterClientBuilder, OpenRouterHttpClient, ProviderRouting,
};

use ur_core::provider::{ModelSpec, Provider, RawEvent, Request};
use ur_core::{BoxStream, Result};

impl Provider for OpenRouterClient {
    fn chat(&self, request: &Request) -> BoxStream<'static, Result<RawEvent>> {
        let config = self.config();
        match request::encode(
            request,
            config.user.as_deref(),
            config.provider_routing.as_ref(),
        ) {
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

        assert_provider::<OpenRouterClient>();
    }

    #[test]
    fn catalog_is_empty() {
        let client = OpenRouterClient::new("key");
        assert_eq!(client.model_spec("openai/gpt-5.5"), None);
        assert_eq!(client.model_notice("openai/gpt-5.5"), None);
    }
}
