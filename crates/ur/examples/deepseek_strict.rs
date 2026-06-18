//! Strict-mode tools. Strict requires the beta host and applies to the whole
//! tool set. The `#[ur::tool]` macro always emits a non-strict schema, so a
//! strict tool is written by hand with `ToolSchema::strict(true)`. Requires
//! `DEEPSEEK_API_KEY`; built but not run as part of the test suite.

use futures_util::StreamExt;

use ur::{Agent, BoxFuture, Model, Tool, ToolArguments, ToolSchema};

struct GetUser;

impl Tool for GetUser {
    fn name(&self) -> &str {
        "get_user"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "get_user",
            serde_json::json!({
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"],
                "additionalProperties": false
            }),
        )
        .description("Look up a user by id.")
        .strict(true)
    }

    fn call(&self, args: ToolArguments) -> BoxFuture<'static, Result<String, String>> {
        let _ = args;
        Box::pin(async move { Ok("{\"name\": \"Ada Lovelace\"}".to_owned()) })
    }
}

#[tokio::main]
async fn main() -> ur::Result<()> {
    // Strict mode requires the beta base URL.
    let client = ur::deepseek::DeepSeekClient::builder().beta(true).build()?;
    let model = Model::new(client, "deepseek-v4-pro");
    let agent = Agent::new("Use tools when useful.", model).tool(GetUser);

    let mut session = agent.session();
    let mut events = session.send("Look up user 42 and tell me their name.");
    while let Some(event) = events.next().await {
        match event? {
            ur::Event::TextDelta { delta } => print!("{delta}"),
            ur::Event::ToolCall {
                name, arguments, ..
            } => eprintln!("\ncall {name}({arguments})"),
            _ => {}
        }
    }
    println!();
    Ok(())
}
