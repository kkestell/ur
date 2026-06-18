//! Thinking and reasoning effort. With thinking enabled the backend ignores
//! `temperature`/`top_p`, so `ur` omits them; `reasoning_effort` is aliased
//! (Low/Medium -> High, ExtraHigh -> Max). Requires `DEEPSEEK_API_KEY`; built
//! but not run as part of the test suite.

use futures_util::StreamExt;

use ur::{Model, ReasoningEffort, Thinking};

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::deepseek::DeepSeekClient::try_from_env()?;
    let model = Model::new(client, "deepseek-v4-pro")
        .thinking(Thinking::Enabled)
        .reasoning_effort(ReasoningEffort::Max);

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
