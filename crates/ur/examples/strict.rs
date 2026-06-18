//! Strict-mode tools. A strict `ToolSchema` constrains the model's tool-call
//! arguments to the schema. The `#[ur::tool]` macro always emits a non-strict
//! schema, so a strict tool is written by hand with `ToolSchema::strict(true)`.
//! OpenAI accepts strict and non-strict tools in the same request and needs no
//! special host. Requires `OPENAI_API_KEY`; built but not run as part of the
//! test suite.

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
    let client = ur::openai::OpenAiClient::try_from_env()?;
    let model = Model::new(client, "gpt-5.4-nano");
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
