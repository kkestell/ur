//! DeepSeek [`Provider`] implementation for `ur`.
//!
//! This crate provides [`DeepSeekClient`], its builder, and the compiled-in
//! DeepSeek model catalog. Most applications reach it through the `ur` facade as
//! `ur::deepseek` with the `deepseek` feature enabled.
//!
//! See the repository
//! [DeepSeek provider specification](https://github.com/kkestell/ur/blob/main/docs/DEEPSEEK.md)
//! for the wire mapping, generation-setting semantics, strict mode, and retry
//! behavior this crate implements.
//!
//! # Example
//!
//! ```no_run
//! use futures_util::StreamExt;
//! use ur_core::event::Event;
//! use ur_core::{Agent, Model};
//! use ur_deepseek::DeepSeekClient;
//!
//! # async fn run() -> ur_core::Result<()> {
//! let client = DeepSeekClient::try_from_env()?;
//! let model = Model::new(client, "deepseek-v4-pro");
//! let agent = Agent::new("You are concise.", model);
//! let mut session = agent.session();
//!
//! let mut events = session.send("Say hello in one sentence.");
//! while let Some(event) = events.next().await {
//!     match event? {
//!         Event::TextDelta { delta } => print!("{delta}"),
//!         Event::Done { .. } => break,
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod catalog;
mod client;
mod executor;
mod request;
mod sse;

pub use client::{DeepSeekClient, DeepSeekClientBuilder, DeepSeekHttpClient};

use ur_core::provider::{ModelNotice, ModelSpec, Provider, RawEvent, Request};
use ur_core::{BoxStream, Result};

impl Provider for DeepSeekClient {
    fn chat(&self, request: &Request) -> BoxStream<'static, Result<RawEvent>> {
        let config = self.config();
        match request::encode(request, config.user_id.as_deref(), config.is_beta()) {
            Ok(body) => executor::chat(self.shared_config(), body),
            Err(error) => Box::pin(futures_util::stream::once(async move { Err(error) })),
        }
    }

    fn model_spec(&self, model_id: &str) -> Option<ModelSpec> {
        catalog::model_spec(model_id)
    }

    fn model_notice(&self, model_id: &str) -> Option<ModelNotice> {
        catalog::model_notice(model_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing::span::{Attributes, Record};
    use tracing::{Event as TracingEvent, Id, Level, Metadata, Subscriber};
    use ur_core::Model;

    #[test]
    fn client_is_a_provider() {
        fn assert_provider<P: Provider>() {}

        assert_provider::<DeepSeekClient>();
    }

    #[test]
    fn catalog_lookups_are_silent_and_repeatable() {
        let client = DeepSeekClient::new("key");

        for _ in 0..3 {
            assert_eq!(
                client.model_spec("deepseek-v4-pro"),
                Some(ModelSpec::new(1_000_000, 384_000))
            );
            assert!(matches!(
                client.model_notice("deepseek-chat"),
                Some(ModelNotice::Deprecated { .. })
            ));
            assert_eq!(client.model_spec("unknown"), None);
            assert_eq!(client.model_notice("deepseek-v4-pro"), None);
        }
    }

    struct WarningCounter {
        warnings: Arc<AtomicUsize>,
    }

    impl Subscriber for WarningCounter {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &TracingEvent<'_>) {
            if *event.metadata().level() == Level::WARN {
                self.warnings.fetch_add(1, Ordering::Relaxed);
            }
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    #[test]
    fn legacy_models_warn_once_while_direct_lookups_stay_silent() {
        let warnings = Arc::new(AtomicUsize::new(0));
        let dispatch = tracing::Dispatch::new(WarningCounter {
            warnings: Arc::clone(&warnings),
        });

        tracing::dispatcher::with_default(&dispatch, || {
            let client = DeepSeekClient::new("key");

            for id in ["deepseek-chat", "deepseek-reasoner"] {
                let _ = client.model_spec(id);
                let _ = client.model_notice(id);
            }
            assert_eq!(warnings.load(Ordering::Relaxed), 0);

            let _ = Model::new(client.clone(), "deepseek-chat");
            assert_eq!(warnings.load(Ordering::Relaxed), 1);

            let _ = Model::new(client.clone(), "deepseek-reasoner");
            assert_eq!(warnings.load(Ordering::Relaxed), 2);

            let _ = Model::new(client, "deepseek-v4-pro");
            assert_eq!(warnings.load(Ordering::Relaxed), 2);
        });
    }
}
