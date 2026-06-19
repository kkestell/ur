//! Thinking mode. DeepSeek honors the `Thinking` toggle (OpenAI's Chat
//! Completions ignores it). With thinking enabled the backend ignores
//! `temperature`/`top_p`, so `ur` omits them; `Thinking::Disabled` turns
//! reasoning off and lets sampling settings through. Requires
//! `DEEPSEEK_API_KEY`; built but not run as part of the test suite.

use futures_util::StreamExt;

use ur::{Model, Thinking};

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::deepseek::DeepSeekClient::try_from_env()?;
    let model = Model::new(client, "deepseek-v4-pro").thinking(Thinking::Enabled);

    let agent = ur::Agent::new("You are a careful reasoner.", model);

    let mut session = agent.session();
    let mut events = session.send(
        "A bat and a ball cost $1.10 in total. The bat costs $1.00 more than \
         the ball. How much does the ball cost?",
    );
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::ReasoningDelta { delta } => eprint!("{delta}"),
            ur::Event::TextDelta { delta } => print!("{delta}"),
            _ => {}
        }
    }
    println!();
    Ok(())
}
