//! Reasoning effort. `ReasoningEffort` controls how hard the model reasons
//! before answering; OpenAI maps Low/Medium/High directly and folds ExtraHigh
//! and Max to High. The reported reasoning-token count reflects the setting.
//! Requires `OPENAI_API_KEY`; built but not run as part of the test suite.

use futures_util::StreamExt;

use ur::{Model, ReasoningEffort};

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::openai::OpenAiClient::try_from_env()?;
    let model = Model::new(client, "gpt-5.4-nano").reasoning_effort(ReasoningEffort::High);

    let agent = ur::Agent::new("You are a careful problem solver.", model);

    let mut session = agent.session();
    let mut events = session.send(
        "A bat and a ball cost $1.10 in total. The bat costs $1.00 more than \
         the ball. How much does the ball cost?",
    );
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::Usage { usage } => {
                if let Some(reasoning) = usage.reasoning_tokens {
                    eprintln!("\nreasoning tokens: {reasoning}");
                }
            }
            _ => {}
        }
    }
    println!();
    Ok(())
}
