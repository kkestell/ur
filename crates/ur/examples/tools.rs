//! Two ways to give an agent tools, used together. A free `#[ur::tool]` fn is a
//! stateless tool registered with `Agent::tool`. An `#[ur::tools]` impl block turns
//! a type's `#[ur::tool]` methods into a stateful tool set whose calls share owned
//! state (here a counter behind an `Arc`), registered with `Agent::tool_set`. Cloning
//! the state is cheap because the field is an `Arc`, so every tool call sees the same
//! underlying counter. Requires `OPENAI_API_KEY`.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use futures_util::StreamExt;

/// Stateless tool: a plain `async` fn with no captured state.
#[ur::tool(description = "Add two integers.")]
async fn add(a: i64, b: i64) -> i64 {
    a + b
}

/// Shared state for the stateful tool set. `Clone` is cheap because the field is an `Arc`.
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

#[tokio::main]
async fn main() -> ur::Result<()> {
    let counter = Counter {
        count: Arc::new(AtomicI64::new(0)),
    };

    let client = ur::openai::OpenAiClient::try_from_env()?;
    let model = ur::Model::new(client, "gpt-5.5");
    let agent = ur::Agent::new("You are a concise assistant. Use tools when useful.", model)
        .tool(add)
        .tool_set(counter.clone());

    let mut session = agent.session();
    let mut events =
        session.send("Add 41 and 1. Then bump the count by 40, then by 2, and report the total.");
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::ToolResult { output, .. } => match output {
                ur::ToolOutput::Ok(v) => eprintln!("result: {v}"),
                ur::ToolOutput::Err(e) => eprintln!("error: {e}"),
            },
            ur::Event::Done { .. } => break,
            _ => {}
        }
    }
    println!();

    // The tool calls mutated the shared state owned by `counter`.
    eprintln!("final count: {}", counter.count.load(Ordering::SeqCst));
    Ok(())
}
