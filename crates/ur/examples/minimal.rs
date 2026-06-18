//! The smallest useful program: build a client from the environment, send one
//! message, and stream the text back. Requires `OPENAI_API_KEY`; built but not
//! run as part of the test suite.

use futures_util::StreamExt;

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::openai::OpenAiClient::try_from_env()?;
    let model = ur::Model::new(client, "gpt-5.4-nano");
    let agent = ur::Agent::new("You are a concise assistant.", model);

    let mut session = agent.session();
    let mut events = session.send("In one sentence, what is Rust?");
    while let Some(event) = events.next().await {
        if let ur::Event::TextDelta { delta } = event? {
            print!("{delta}");
        }
    }
    println!();
    Ok(())
}
