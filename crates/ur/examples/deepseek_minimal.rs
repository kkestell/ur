//! The smallest useful DeepSeek program: build a client from the environment,
//! send one message, and stream the text back. Requires `DEEPSEEK_API_KEY`;
//! built but not run as part of the test suite.

use futures_util::StreamExt;

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::deepseek::DeepSeekClient::try_from_env()?;
    let model = ur::Model::new(client, "deepseek-v4-pro");
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
