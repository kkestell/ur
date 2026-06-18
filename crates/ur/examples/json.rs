//! Requesting a JSON-object response. `ResponseFormat::JsonObject` sets the
//! response format; the prompt must still instruct the model to emit JSON.
//! Requires `OPENAI_API_KEY`; built but not run as part of the test suite.

use futures_util::StreamExt;

use ur::{Model, ResponseFormat};

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::openai::OpenAiClient::try_from_env()?;
    let model = Model::new(client, "gpt-5.4-nano").response_format(ResponseFormat::JsonObject);

    let agent = ur::Agent::new("You reply only with a single JSON object.", model);

    let mut session = agent.session();
    let mut events = session.send("Return the capital of France as {\"capital\": \"...\"}.");
    let mut json = String::new();
    while let Some(event) = events.next().await {
        if let ur::Event::TextDelta { delta } = event? {
            json.push_str(&delta);
        }
    }
    println!("{json}");
    Ok(())
}
