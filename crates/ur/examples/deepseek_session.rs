//! A multi-turn conversation. `Session` retains history (including each
//! assistant turn's `reasoning_content`) and replays it on every turn, so the
//! reasoning-content 400 described in the provider docs cannot occur. Requires
//! `DEEPSEEK_API_KEY`; built but not run as part of the test suite.

use futures_util::StreamExt;

async fn turn(
    session: &mut ur::Session<ur::deepseek::DeepSeekClient>,
    prompt: &str,
) -> ur::Result<()> {
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
    let client = ur::deepseek::DeepSeekClient::try_from_env()?;
    let model = ur::Model::new(client, "deepseek-v4-pro");
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
