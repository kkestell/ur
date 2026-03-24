//! Single-turn agent orchestrator (tracer bullet).
//!
//! Drives a full agent turn by calling provider extensions directly:
//! load session → add user msg → LLM stream → tool dispatch →
//! LLM stream → append session → compact.

use std::io::Write;
use std::path::Path;

use anyhow::{Result, bail};
use wasmtime::Engine;

use crate::config::UserConfig;
use crate::extension_host::{ExtensionInstance, wit_types};
use crate::manifest::{self, ManifestEntry, WorkspaceManifest};
use crate::model;

const DEFAULT_RUN_USER_MESSAGE: &str = "Hello, please greet the world";
const RUN_USER_MESSAGE_ENV_VAR: &str = "UR_RUN_USER_MESSAGE";

/// Extracts concatenated text from a message's parts.
fn extract_text(msg: &wit_types::Message) -> String {
    msg.parts
        .iter()
        .filter_map(|p| match p {
            wit_types::MessagePart::Text(s) => Some(s.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Extracts tool calls from a message's parts.
fn extract_tool_calls(msg: &wit_types::Message) -> Vec<&wit_types::ToolCall> {
    msg.parts
        .iter()
        .filter_map(|p| match p {
            wit_types::MessagePart::ToolCall(tc) => Some(tc),
            _ => None,
        })
        .collect()
}

/// Assembles a `Completion` from streamed chunks, printing deltas as they arrive.
fn stream_completion(
    llm: &mut ExtensionInstance,
    messages: &[wit_types::Message],
    model_id: &str,
    settings: &[wit_types::ConfigSetting],
    tools: &[wit_types::ToolDescriptor],
) -> Result<wit_types::Completion> {
    let mut parts: Vec<wit_types::MessagePart> = Vec::new();
    let mut usage = None;

    llm.complete(messages, model_id, settings, tools, None, |chunk| {
        for dp in &chunk.delta_parts {
            match dp {
                wit_types::MessagePart::Text(delta) => {
                    print!("{delta}");
                    let _ = std::io::stdout().flush();
                    // Accumulate text into the last text part, or start a new one.
                    if let Some(wit_types::MessagePart::Text(existing)) = parts.last_mut() {
                        existing.push_str(delta);
                    } else {
                        parts.push(wit_types::MessagePart::Text(delta.clone()));
                    }
                }
                wit_types::MessagePart::ToolCall(tc) => {
                    parts.push(wit_types::MessagePart::ToolCall(tc.clone()));
                }
                wit_types::MessagePart::ToolResult(tr) => {
                    parts.push(wit_types::MessagePart::ToolResult(tr.clone()));
                }
            }
        }
        if chunk.usage.is_some() {
            usage = chunk.usage;
        }
    })?
    .map_err(|e| anyhow::anyhow!("LLM streaming: {e}"))?;

    let has_text = parts
        .iter()
        .any(|p| matches!(p, wit_types::MessagePart::Text(_)));
    if has_text {
        println!();
    }

    Ok(wit_types::Completion {
        message: wit_types::Message {
            role: "assistant".into(),
            parts,
        },
        usage,
    })
}

fn resolve_run_user_message(env_value: Option<String>) -> String {
    match env_value {
        Some(value) if !value.trim().is_empty() => value,
        _ => DEFAULT_RUN_USER_MESSAGE.into(),
    }
}

fn run_user_message() -> wit_types::Message {
    let content = resolve_run_user_message(std::env::var(RUN_USER_MESSAGE_ENV_VAR).ok());
    wit_types::Message {
        role: "user".into(),
        parts: vec![wit_types::MessagePart::Text(content)],
    }
}

/// Runs a single hardcoded agent turn, printing debug output at each step.
#[expect(
    clippy::too_many_lines,
    reason = "The tracer-bullet flow stays linear so the end-to-end turn is easy to inspect"
)]
pub fn run(engine: &Engine, ur_root: &Path, workspace: &Path) -> Result<()> {
    let manifest = manifest::scan_and_load(engine, ur_root, workspace)?;
    let config = UserConfig::load(ur_root)?;

    // Resolve "default" role to a provider/model pair.
    let providers = model::collect_provider_models(engine, &manifest)?;
    let (provider_id, model_id) = model::resolve_role(&config, "default", &providers)?;

    // Load the LLM extension and get settings from list-settings().
    let init_config = crate::provider::init_config(&provider_id);
    let (mut settings_probe, extension_id) =
        load_llm_provider(engine, &manifest, &provider_id, &init_config)?;
    // Populate dynamic catalog (needed for OpenRouter).
    let _ = settings_probe.list_models();
    let descriptors = settings_probe.list_settings()?;
    drop(settings_probe);

    let settings = config.settings_for(&extension_id, &model_id, &descriptors)?;

    // ── 1. Load session ──────────────────────────────────────────────
    let session_id = "demo";
    println!("[turn] loading session \"{session_id}\"...");
    let mut session = load_slot(engine, &manifest, "session-provider")?;
    session
        .init(&[])?
        .map_err(|e| anyhow::anyhow!("session init: {e}"))?;
    let mut messages: Vec<wit_types::Message> = session
        .load_session(session_id)?
        .map_err(|e| anyhow::anyhow!("load_session: {e}"))?;
    let loaded_message_count = messages.len();
    println!(
        "[turn] session loaded: {} messages ({})",
        messages.len(),
        if messages.is_empty() {
            "fresh session"
        } else {
            "existing"
        }
    );

    // ── 2. Add user message ──────────────────────────────────────────
    let user_msg = run_user_message();
    println!("[turn] adding user message: {:?}", extract_text(&user_msg));
    messages.push(user_msg);

    // ── 3. Load general extensions and collect tools ────────────────
    let mut generals = load_general_extensions(engine, &manifest)?;
    let mut tools: Vec<wit_types::ToolDescriptor> = Vec::new();
    for ext in &mut generals {
        ext.init(&[])?
            .map_err(|e| anyhow::anyhow!("extension init: {e}"))?;
        tools.extend(ext.list_tools()?);
    }
    if !tools.is_empty() {
        println!(
            "[turn] collected {} tool{}",
            tools.len(),
            if tools.len() == 1 { "" } else { "s" }
        );
    }

    // ── 4. First LLM completion (streaming) ──────────────────────────
    println!("[turn] resolving role \"default\" → {provider_id}/{model_id}");
    let init_config = crate::provider::init_config(&provider_id);
    let (mut llm, _) = load_llm_provider(engine, &manifest, &provider_id, &init_config)?;

    println!(
        "[turn] calling LLM streaming ({} message{})...",
        messages.len(),
        if messages.len() == 1 { "" } else { "s" }
    );
    let completion = stream_completion(&mut llm, &messages, &model_id, &settings, &tools)?;

    let tool_calls = extract_tool_calls(&completion.message);
    if tool_calls.is_empty() {
        println!(
            "[turn] LLM returned message: {:?}",
            extract_text(&completion.message)
        );
    } else {
        for tc in &tool_calls {
            println!(
                "[turn] LLM returned tool call: {}({})",
                tc.name, tc.arguments_json
            );
        }
    }

    // Push the assistant message into history.
    messages.push(completion.message.clone());

    // ── 5. Tool dispatch ─────────────────────────────────────────────
    if !tool_calls.is_empty() {
        dispatch_tool_calls(&tool_calls, engine, &manifest, &mut messages)?;

        // ── 6. Second LLM completion (with tool results) ────────────
        println!(
            "[turn] calling LLM streaming ({} messages, includes tool result)...",
            messages.len()
        );
        let completion2 = stream_completion(&mut llm, &messages, &model_id, &settings, &tools)?;
        println!(
            "[turn] LLM returned message: {:?}",
            extract_text(&completion2.message)
        );
        messages.push(completion2.message);
    }

    // ── 7. Append to session ─────────────────────────────────────────
    let session_appends = pending_session_appends(&messages, loaded_message_count);
    println!(
        "[turn] appending {} message{} to session \"{session_id}\"",
        session_appends.len(),
        if session_appends.len() == 1 { "" } else { "s" }
    );
    for message in session_appends {
        session
            .append_session(session_id, message)?
            .map_err(|e| anyhow::anyhow!("append_session: {e}"))?;
    }

    // ── 8. Compact ───────────────────────────────────────────────────
    println!("[turn] compacting {} messages...", messages.len());
    let mut compaction = load_slot(engine, &manifest, "compaction-provider")?;
    compaction
        .init(&[])?
        .map_err(|e| anyhow::anyhow!("compaction init: {e}"))?;
    let compacted = compaction
        .compact(&messages)?
        .map_err(|e| anyhow::anyhow!("compact: {e}"))?;
    println!(
        "[turn] compaction result: {} messages ({})",
        compacted.len(),
        if compacted.len() == messages.len() {
            "unchanged"
        } else {
            "compacted"
        }
    );

    println!("[turn] done");
    Ok(())
}

fn pending_session_appends(
    messages: &[wit_types::Message],
    loaded_message_count: usize,
) -> &[wit_types::Message] {
    if loaded_message_count >= messages.len() {
        return &[];
    }

    &messages[loaded_message_count..]
}

/// Dispatches tool calls to general extensions in parallel, appending results to messages.
///
/// Each tool call runs in its own scoped thread with a fresh extension
/// instance. Results are collected and appended in the original tool
/// call order.
fn dispatch_tool_calls(
    tool_calls: &[&wit_types::ToolCall],
    engine: &Engine,
    manifest: &WorkspaceManifest,
    messages: &mut Vec<wit_types::Message>,
) -> Result<()> {
    if tool_calls.is_empty() {
        return Ok(());
    }

    for tc in tool_calls {
        println!("[turn] dispatching tool {:?} to extensions...", tc.name);
    }

    let results: Vec<Result<wit_types::Message>> = std::thread::scope(|s| {
        let handles: Vec<_> = tool_calls
            .iter()
            .map(|tc| {
                s.spawn(move || {
                    let mut generals = load_general_extensions(engine, manifest)?;
                    for ext in &mut generals {
                        ext.init(&[])?
                            .map_err(|e| anyhow::anyhow!("extension init: {e}"))?;
                    }

                    for ext in &mut generals {
                        if let Ok(result) = ext.call_tool(&tc.name, &tc.arguments_json)? {
                            println!("[turn] tool result: {result:?}");
                            return Ok(wit_types::Message {
                                role: "tool".into(),
                                parts: vec![wit_types::MessagePart::ToolResult(
                                    wit_types::ToolResult {
                                        tool_call_id: tc.id.clone(),
                                        tool_name: tc.name.clone(),
                                        content: result,
                                    },
                                )],
                            });
                        }
                    }
                    bail!("no extension handled tool {:?}", tc.name)
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|h| h.join().expect("tool dispatch thread panicked"))
            .collect()
    });

    for result in results {
        messages.push(result?);
    }
    Ok(())
}

/// Finds the first enabled entry for a slot and loads it.
fn load_slot(
    engine: &Engine,
    manifest: &WorkspaceManifest,
    slot: &str,
) -> Result<ExtensionInstance> {
    let entry = first_enabled(manifest, slot)?;
    let instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path))?;
    Ok(instance)
}

/// Loads the LLM provider extension matching a specific provider ID.
///
/// Returns the instance and its manifest extension ID.
fn load_llm_provider(
    engine: &Engine,
    manifest: &WorkspaceManifest,
    provider_id: &str,
    init_config: &[(String, String)],
) -> Result<(ExtensionInstance, String)> {
    for entry in &manifest.extensions {
        if !entry.enabled || entry.slot.as_deref() != Some("llm-provider") {
            continue;
        }
        let mut instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path))?;
        instance
            .init(init_config)?
            .map_err(|e| anyhow::anyhow!("LLM init: {e}"))?;
        if let Ok(Ok(id)) = instance.provider_id()
            && id == provider_id
        {
            return Ok((instance, entry.id.clone()));
        }
    }
    bail!("no enabled LLM provider with id \"{provider_id}\"")
}

/// Loads all enabled general extensions (for tool dispatch).
fn load_general_extensions(
    engine: &Engine,
    manifest: &WorkspaceManifest,
) -> Result<Vec<ExtensionInstance>> {
    let mut result = Vec::new();
    for entry in &manifest.extensions {
        if !entry.enabled || entry.slot.is_some() {
            continue;
        }
        let instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path))?;
        result.push(instance);
    }
    Ok(result)
}

/// Finds the first enabled manifest entry for a given slot.
fn first_enabled<'a>(manifest: &'a WorkspaceManifest, slot: &str) -> Result<&'a ManifestEntry> {
    manifest
        .extensions
        .iter()
        .find(|e| e.enabled && e.slot.as_deref() == Some(slot))
        .ok_or_else(|| anyhow::anyhow!("no enabled extension for slot \"{slot}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_message(role: &str, text: &str) -> wit_types::Message {
        wit_types::Message {
            role: role.into(),
            parts: vec![wit_types::MessagePart::Text(text.into())],
        }
    }

    fn tool_call_message(tool_call_id: &str, tool_name: &str) -> wit_types::Message {
        wit_types::Message {
            role: "assistant".into(),
            parts: vec![wit_types::MessagePart::ToolCall(wit_types::ToolCall {
                id: tool_call_id.into(),
                name: tool_name.into(),
                arguments_json: "{\"city\":\"Austin\"}".into(),
                provider_metadata_json: String::new(),
            })],
        }
    }

    fn tool_result_message(tool_call_id: &str, tool_name: &str) -> wit_types::Message {
        wit_types::Message {
            role: "tool".into(),
            parts: vec![wit_types::MessagePart::ToolResult(wit_types::ToolResult {
                tool_call_id: tool_call_id.into(),
                tool_name: tool_name.into(),
                content: "{\"temperature_f\":72}".into(),
            })],
        }
    }

    #[test]
    fn pending_session_appends_no_tool_turn_includes_user_and_reply() {
        let messages = vec![
            text_message("user", "Earlier question"),
            text_message("assistant", "Earlier answer"),
            text_message("user", "Hello"),
            text_message("assistant", "Hi there"),
        ];

        let appends = pending_session_appends(&messages, 2);

        assert_eq!(appends.len(), 2);
        assert_eq!(appends[0].role, "user");
        assert_eq!(extract_text(&appends[0]), "Hello");
        assert_eq!(appends[1].role, "assistant");
        assert_eq!(extract_text(&appends[1]), "Hi there");
    }

    #[test]
    fn pending_session_appends_tool_turn_includes_full_turn_delta() {
        let messages = vec![
            text_message("assistant", "Existing context"),
            text_message("user", "Weather?"),
            tool_call_message("call-1", "get_weather"),
            tool_result_message("call-1", "get_weather"),
            text_message("assistant", "It is 72F in Austin."),
        ];

        let appends = pending_session_appends(&messages, 1);

        assert_eq!(appends.len(), 4);
        assert_eq!(appends[0].role, "user");
        assert!(matches!(
            &appends[1].parts[0],
            wit_types::MessagePart::ToolCall(tc)
                if tc.id == "call-1" && tc.name == "get_weather"
        ));
        assert!(matches!(
            &appends[2].parts[0],
            wit_types::MessagePart::ToolResult(tr)
                if tr.tool_call_id == "call-1" && tr.tool_name == "get_weather"
        ));
        assert_eq!(appends[3].role, "assistant");
        assert_eq!(extract_text(&appends[3]), "It is 72F in Austin.");
    }

    #[test]
    fn resolve_run_user_message_uses_default_when_env_is_absent() {
        assert_eq!(
            resolve_run_user_message(None),
            "Hello, please greet the world"
        );
    }

    #[test]
    fn resolve_run_user_message_prefers_env_override() {
        assert_eq!(
            resolve_run_user_message(Some(
                "What is the weather in Paris, and should I wear a coat?".into(),
            )),
            "What is the weather in Paris, and should I wear a coat?"
        );
    }
}
