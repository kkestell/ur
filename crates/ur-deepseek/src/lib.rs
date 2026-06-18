//! DeepSeek provider placeholder for `ur`.

#![forbid(unsafe_code)]

use std::pin::Pin;
use std::task::{Context, Poll};

/// Placeholder DeepSeek client handle.
#[derive(Clone, Debug, Default)]
pub struct DeepSeekClient;

impl ur_core::provider::Provider for DeepSeekClient {
    fn chat(
        &self,
        _request: &ur_core::provider::Request,
    ) -> ur_core::BoxStream<'static, ur_core::Result<ur_core::provider::RawEvent>> {
        Box::pin(PlaceholderStream { done: false })
    }

    fn model_spec(&self, _model_id: &str) -> Option<ur_core::provider::ModelSpec> {
        None
    }
}

struct PlaceholderStream {
    done: bool,
}

impl ur_core::Stream for PlaceholderStream {
    type Item = ur_core::Result<ur_core::provider::RawEvent>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }

        self.done = true;
        Poll::Ready(Some(Ok(ur_core::provider::RawEvent::Done {
            finish_reason: ur_core::event::FinishReason::Stop,
            usage: None,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_is_a_provider() {
        fn assert_provider<P: ur_core::provider::Provider>() {}

        assert_provider::<DeepSeekClient>();
    }
}
