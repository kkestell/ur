//! Structured outputs over OpenRouter. A `json_schema` response format
//! constrains the model's reply to a schema derived from a Rust type, so the
//! accumulated text parses back into that type. `ResponseFormat::json_schema_for::<T>`
//! derives the schema with `schemars`; `JsonSchemaFormat` is available for a
//! hand-built schema or to opt out of strict mode. Pick a model whose upstream
//! provider supports schema-constrained outputs. Requires `OPENROUTER_API_KEY`;
//! built but not run as part of the test suite.

use futures_util::StreamExt;

use ur::{Model, ResponseFormat};

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Capital {
    city: String,
    country: String,
}

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::openrouter::OpenRouterClient::try_from_env()?;
    let model = Model::new(client, "deepseek/deepseek-v4-flash")
        .response_format(ResponseFormat::json_schema_for::<Capital>("capital"));

    let agent = ur::Agent::new("You answer with the requested structured data.", model);

    let mut session = agent.session();
    let mut events = session.send("What is the capital of France?");
    let mut json = String::new();
    while let Some(event) = events.next().await {
        if let ur::Event::TextDelta { delta } = event? {
            json.push_str(&delta);
        }
    }

    let capital: Capital = serde_json::from_str(&json).map_err(|error| ur::Error::Decode {
        context: "parsing the structured response".to_owned(),
        source: Box::new(error),
    })?;
    println!("{} is the capital of {}.", capital.city, capital.country);
    Ok(())
}
