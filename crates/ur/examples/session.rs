//! A multi-turn conversation. `Session` retains the full message history and
//! replays it on every turn, so the model keeps earlier context. Requires
//! `OPENAI_API_KEY`; built but not run as part of the test suite.

use futures_util::StreamExt;

async fn turn(session: &mut ur::Session<ur::openai::OpenAiClient>, prompt: &str) -> ur::Result<()> {
    println!("> {prompt}");
    let mut events = session.send(prompt);
    while let Some(event) = events.next().await {
        if let ur::Event::TextDelta { delta } = event? {
            print!("{delta}");
        }
    }
    println!();
    Ok(())
}

#[tokio::main]
async fn main() -> ur::Result<()> {
    let client = ur::openai::OpenAiClient::try_from_env()?;
    let model = ur::Model::new(client, "gpt-5.4-nano");
    let agent = ur::Agent::new("You are a concise assistant.", model);

    let mut session = agent.session();
    turn(
        &mut session,
        "Pick a number between 1 and 10 and remember it.",
    )
    .await?;
    turn(&mut session, "Is the number you picked even?").await?;
    Ok(())
}
