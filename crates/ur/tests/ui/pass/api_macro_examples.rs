//! The `#[ur::tool]` examples from the API documentation must compile through
//! the facade and register with `agent.tool(...)`.

use futures_util::stream;
use serde::Serialize;
use ur::{BoxStream, Provider, RawEvent, Request, Result};

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[derive(Serialize)]
struct Weather {
    temp_c: f64,
    summary: String,
}

#[ur::tool(description = "Look up the current weather for a city.")]
async fn weather(city: String) -> std::result::Result<Weather, std::io::Error> {
    Ok(Weather {
        temp_c: 18.5,
        summary: format!("clear skies over {city}"),
    })
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
    let model = ur::Model::new(NullProvider, "model-id");
    let _agent = ur::Agent::new("You are a concise assistant.", model)
        .tool(add)
        .tool(weather);
}
