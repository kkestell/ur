use futures_util::StreamExt;
use serde::Serialize;

#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 {
    a + b
}

#[derive(Serialize)]
struct Weather {
    temp_c: f64,
    summary: String,
}

#[ur::tool(description = "Look up the current weather for a city.")]
async fn weather(city: String) -> Result<Weather, std::io::Error> {
    Ok(Weather {
        temp_c: 18.5,
        summary: format!("clear skies over {city}"),
    })
}

#[tokio::main]
async fn main() -> ur::Result<()> {
    // OpenRouter recommends the HTTP-Referer / X-Title headers for app attribution.
    let client = ur::openrouter::OpenRouterClient::builder()
        .referer("https://github.com/kkestell/ur")
        .title("ur example")
        .build()?;
    // OpenRouter model ids are namespaced by upstream provider.
    let model = ur::Model::new(client, "deepseek/deepseek-v4-flash");

    let agent = ur::Agent::new("You are a concise assistant. Use tools when useful.", model)
        .tool(add)
        .tool(weather);

    let mut session = agent.session();
    let mut events = session.send("What is 41 + 1? Use the tool.");
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::ReasoningDelta { .. } => {}
            ur::Event::ToolCall {
                name, arguments, ..
            } => eprintln!("\ncall {name}({arguments})"),
            ur::Event::ToolResult { output, .. } => match output {
                ur::ToolOutput::Ok(v) => eprintln!("result: {v}"),
                ur::ToolOutput::Err(e) => eprintln!("error: {e}"),
            },
            ur::Event::Usage { usage } => eprintln!(
                "tokens: in={} (cached {}) out={}",
                usage.prompt_tokens,
                usage.cached_prompt_tokens.unwrap_or(0),
                usage.completion_tokens,
            ),
            ur::Event::Done { .. } => break,
            _ => {}
        }
    }
    Ok(())
}
