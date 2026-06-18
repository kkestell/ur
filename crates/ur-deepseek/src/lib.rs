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
        Box::pin(EmptyStream)
    }

    fn model_spec(&self, _model_id: &str) -> Option<ur_core::provider::ModelSpec> {
        None
    }
}

struct EmptyStream;

impl ur_core::Stream for EmptyStream {
    type Item = ur_core::Result<ur_core::provider::RawEvent>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
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
