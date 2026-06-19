//! Kitchen sink: the core features of the agent loop in one program — a
//! builder-configured client, reasoning effort, both tool flavors (a stateless
//! `#[ur::tool]` fn registered with `Agent::tool`, and a stateful `#[ur::tools]`
//! set registered with `Agent::tool_set`), a multi-turn `Session` with rollback
//! via a cloned checkpoint, and handling of the full event stream (text,
//! reasoning, tool calls and results, usage). Requires `OPENAI_API_KEY`.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use futures_util::StreamExt;

use ur::{Model, ReasoningEffort};

/// Stateless tool: a plain `async` fn with no captured state.
#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 {
    a + b
}

/// Shared state for the stateful tool set. `Clone` is cheap because the field is an `Arc`,
/// so every tool call sees the same underlying counter.
#[derive(Clone)]
struct Counter {
    count: Arc<AtomicI64>,
}

#[ur::tools]
impl Counter {
    #[ur::tool(description = "Add to the running count and return the new total.")]
    async fn bump(&self, by: i64) -> i64 {
        self.count.fetch_add(by, Ordering::SeqCst) + by
    }

    #[ur::tool(description = "Return the running count.")]
    fn total(&self) -> i64 {
        self.count.load(Ordering::SeqCst)
    }
}

/// Drive one user turn to completion, streaming every event kind to the console.
async fn turn(session: &mut ur::Session<ur::openai::OpenAiClient>, prompt: &str) -> ur::Result<()> {
    println!("\n> {prompt}");
    let mut events = session.send(prompt);
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
                "tokens: in={} (cached {}) out={} reasoning={}",
                usage.prompt_tokens,
                usage.cached_prompt_tokens.unwrap_or(0),
                usage.completion_tokens,
                usage.reasoning_tokens.unwrap_or(0),
            ),
            ur::Event::Done { .. } => break,
            _ => {}
        }
    }
    println!();
    Ok(())
}

#[tokio::main]
async fn main() -> ur::Result<()> {
    // Configure the client through its builder; the API key falls back to `$OPENAI_API_KEY`.
    let client = ur::openai::OpenAiClient::builder()
        .timeout(Duration::from_secs(120))
        .max_retries(5)
        .user("kitchen-sink")
        .build()?;

    // Tune the model: ask it to reason harder before answering.
    let model = Model::new(client, "gpt-5.5").reasoning_effort(ReasoningEffort::High);

    let counter = Counter {
        count: Arc::new(AtomicI64::new(0)),
    };

    // Register both tool flavors on one agent.
    let agent = ur::Agent::new("You are a concise assistant. Use tools when useful.", model)
        .tool(add)
        .tool_set(counter.clone());

    let mut session = agent.session();

    // Multi-turn: the session replays history, so the second turn sees the first.
    turn(&mut session, "Add 41 and 1 with the tool.").await?;
    turn(
        &mut session,
        "Now bump the count by that result, then report the total.",
    )
    .await?;

    // Rollback: a `Session` is `Clone`, so cloning before a turn is a checkpoint.
    let checkpoint = session.clone();
    turn(&mut session, "Bump the count by 1000.").await?;
    eprintln!(
        "history before rollback: {} messages",
        session.history().len()
    );
    session = checkpoint; // discard the throwaway turn
    eprintln!(
        "history after rollback: {} messages",
        session.history().len()
    );

    // The 1000 bump is gone from the conversation, but it still mutated the shared
    // counter — tool side effects are not undone by rolling back the history.
    eprintln!("final count: {}", counter.count.load(Ordering::SeqCst));
    Ok(())
}
