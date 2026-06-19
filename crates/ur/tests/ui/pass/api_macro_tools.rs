//! The `#[ur::tools]` stateful-tool example from the API documentation must
//! compile through the facade and register with `agent.tool_set(...)`.

use std::sync::Arc;

use futures_util::stream;
use ur::{BoxStream, Provider, RawEvent, Request, Result};

#[derive(Clone)]
struct Tools {
    greeting: Arc<String>,
}

#[ur::tools]
impl Tools {
    #[ur::tool(description = "Greet a user by name.", name = "say_hello")]
    async fn greet(&self, name: String) -> String {
        format!("{}, {name}", self.greeting)
    }

    #[ur::tool(description = "Return the configured greeting.")]
    fn greeting(&self) -> String {
        self.greeting.to_string()
    }

    #[cfg(test)]
    #[ur::tool]
    fn gated(&self) -> i64 {
        1
    }

    // Unmarked: a normal inherent method, not exposed as a tool.
    fn _len(&self) -> usize {
        self.greeting.len()
    }
}

struct NullProvider;

impl Provider for NullProvider {
    fn chat(&self, _request: &Request) -> BoxStream<'static, Result<RawEvent>> {
        Box::pin(stream::empty())
    }

    fn model_spec(&self, _model_id: &str) -> Option<ur::ModelSpec> {
        None
    }
}

fn main() {
    let tools = Tools {
        greeting: Arc::new("Hello".to_owned()),
    };
    let model = ur::Model::new(NullProvider, "model-id");
    let _agent = ur::Agent::new("You are a concise assistant.", model).tool_set(tools);
}
