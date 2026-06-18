//! Configuring `DeepSeekClient` through its builder: API-key fallback to the
//! environment, a tighter timeout, a larger retry budget, and a request
//! isolation `user_id`. (The beta host needed for strict tools is shown in
//! `deepseek_strict`.) Requires `DEEPSEEK_API_KEY`; built but not run as part
//! of the test suite.

use std::time::Duration;

use futures_util::StreamExt;

#[tokio::main]
async fn main() -> ur::Result<()> {
    // `api_key` is left unset, so the key falls back to `$DEEPSEEK_API_KEY`.
    let client = ur::deepseek::DeepSeekClient::builder()
        .timeout(Duration::from_secs(120))
        .max_retries(5)
        .user_id("tenant-42")
        .build()?;

    let model = ur::Model::new(client, "deepseek-v4-pro");
    let agent = ur::Agent::new("You are a concise assistant.", model);

    let mut session = agent.session();
    let mut events = session.send("Say hello.");
    while let Some(event) = events.next().await {
        if let ur::Event::TextDelta { delta } = event? {
            print!("{delta}");
        }
    }
    println!();
    Ok(())
}
