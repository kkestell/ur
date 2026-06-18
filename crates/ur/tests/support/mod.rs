//! Shared fake provider for facade integration tests.

use std::collections::VecDeque;
use std::sync::Mutex;

use ur::{BoxStream, Provider, RawEvent, Request, Result};

/// A provider that replays a scripted list of `RawEvent` batches, one batch per
/// `chat` call, and records every request it receives.
#[derive(Default)]
pub struct FakeProvider {
    responses: Mutex<VecDeque<Vec<RawEvent>>>,
    pub requests: Mutex<Vec<Request>>,
}

impl FakeProvider {
    pub fn new(responses: impl IntoIterator<Item = Vec<RawEvent>>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl Provider for FakeProvider {
    fn chat(&self, request: &Request) -> BoxStream<'static, Result<RawEvent>> {
        self.requests.lock().unwrap().push(request.clone());
        let batch = self
            .responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_default();
        Box::pin(futures_util::stream::iter(batch.into_iter().map(Ok)))
    }

    fn model_spec(&self, _model_id: &str) -> Option<ur::ModelSpec> {
        None
    }
}
