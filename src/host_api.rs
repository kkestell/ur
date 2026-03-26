//! The `ur` module exposed to Lua extensions.
//!
//! Builds the host API table that gets injected as `ur` global in each
//! extension's sandboxed VM.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use mlua::prelude::*;
use tracing::info;

use crate::lua_host::{RegisteredHook, RegisteredTool};
use crate::providers::{LlmProvider, SessionProvider};
use crate::types::{ExtensionCapabilities, ToolDescriptor};

/// Optional provider references injected into the `ur` module.
///
/// These are `None` during early load (before providers are ready) and
/// populated once the workspace has finished initialization.
#[expect(
    missing_debug_implementations,
    reason = "Contains dyn trait objects that are not Debug"
)]
#[derive(Default)]
pub struct HostProviders {
    pub llm_providers: Vec<Arc<LlmProvider>>,
    pub session_provider: Option<Arc<dyn SessionProvider>>,
}

/// Builds the `ur` module table for a Lua extension.
///
/// # Errors
///
/// Returns an error if the operation fails.
///
/// # Panics
///
/// Panics if a mutex is poisoned.
pub fn build_ur_module(
    lua: &Lua,
    capabilities: &ExtensionCapabilities,
    config: &serde_json::Value,
    tools: &Arc<Mutex<Vec<RegisteredTool>>>,
    hooks: &Arc<Mutex<Vec<RegisteredHook>>>,
    providers: &HostProviders,
) -> Result<LuaTable> {
    let ur = lua.create_table()?;

    // ur.log(msg) — always available
    let log_fn = lua.create_function(|_, msg: String| {
        info!(target: "lua_extension", "{msg}");
        Ok(())
    })?;
    ur.set("log", log_fn)?;

    // ur.config — populated from user config
    let config_table: LuaValue = lua.to_value(config)?;
    ur.set("config", config_table)?;

    // ur.tool(name, spec) — register a tool
    let tools_clone = Arc::clone(tools);
    let tool_fn = lua.create_function(move |lua, (name, spec): (String, LuaTable)| {
        let description: String = spec.get("description").unwrap_or_default();
        let parameters: LuaValue = spec.get::<LuaValue>("parameters").unwrap_or(LuaValue::Nil);
        let handler: LuaFunction = spec
            .get("handler")
            .map_err(|_err| LuaError::runtime("tool spec must include a 'handler' function"))?;

        // Convert parameters to JSON schema string.
        let params_json = if parameters.is_nil() {
            r#"{"type":"object","properties":{}}"#.to_owned()
        } else {
            let json: serde_json::Value = lua.from_value(parameters)?;
            serde_json::to_string(&json).map_err(LuaError::external)?
        };

        let handler_key = lua.create_registry_value(handler)?;

        let descriptor = ToolDescriptor {
            name: name.clone(),
            description,
            parameters_json_schema: params_json,
        };

        tools_clone.lock().unwrap().push(RegisteredTool {
            descriptor,
            handler_key,
        });

        Ok(())
    })?;
    ur.set("tool", tool_fn)?;

    // ur.hook(name, fn) — register a lifecycle hook handler
    let hooks_clone = Arc::clone(hooks);
    let hook_fn = lua.create_function(move |lua, (name, handler): (String, LuaFunction)| {
        let valid_hooks = [
            "before_completion",
            "after_completion",
            "before_tool",
            "after_tool",
            "before_session_load",
            "after_session_load",
            "before_session_append",
            "before_compaction",
            "after_compaction",
        ];

        if !valid_hooks.contains(&name.as_str()) {
            return Err(LuaError::runtime(format!(
                "unknown hook '{name}'. Valid hooks: {}",
                valid_hooks.join(", ")
            )));
        }

        let handler_key = lua.create_registry_value(handler)?;

        hooks_clone.lock().unwrap().push(RegisteredHook {
            hook_name: name,
            handler_key,
        });

        Ok(())
    })?;
    ur.set("hook", hook_fn)?;

    // ur.complete(messages, opts) — always available, calls native LLM (no hooks).
    if !providers.llm_providers.is_empty() {
        let complete_fn = build_complete_fn(lua, &providers.llm_providers)?;
        ur.set("complete", complete_fn)?;
    }

    // ur.session — read-only session access (always available if provider exists).
    if let Some(session_provider) = &providers.session_provider {
        let session = build_session_module(lua, session_provider)?;
        ur.set("session", session)?;
    }

    // Capability-gated APIs.
    if capabilities.network {
        let http = build_http_module(lua)?;
        ur.set("http", http)?;
    }

    if capabilities.fs_read || capabilities.fs_write {
        let fs = build_fs_module(lua, capabilities.fs_read, capabilities.fs_write)?;
        ur.set("fs", fs)?;
    }

    Ok(ur)
}

/// Builds the `ur.complete()` function for raw LLM completion (no hooks).
fn build_complete_fn(lua: &Lua, llm_providers: &[Arc<LlmProvider>]) -> Result<LuaFunction> {
    let llm_providers = llm_providers.to_vec();
    let complete_fn = lua.create_async_function(
        move |lua, (messages_val, opts): (LuaValue, Option<LuaTable>)| {
            let llm_providers = llm_providers.clone();
            async move {
                let messages_json: serde_json::Value = lua.from_value(messages_val)?;
                let messages: Vec<crate::types::Message> =
                    serde_json::from_value(messages_json).map_err(LuaError::external)?;

                let model_id: String = opts
                    .as_ref()
                    .and_then(|o| o.get::<String>("model").ok())
                    .unwrap_or_default();
                let provider_id: String = opts
                    .as_ref()
                    .and_then(|o| o.get::<String>("provider").ok())
                    .unwrap_or_default();
                let tool_choice: Option<crate::types::ToolChoice> = opts
                    .as_ref()
                    .and_then(|o| o.get::<String>("tool_choice").ok())
                    .and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok());

                // Find matching provider, or use the first one.
                let llm = if provider_id.is_empty() {
                    llm_providers
                        .first()
                        .ok_or_else(|| LuaError::runtime("no LLM providers available"))?
                } else {
                    llm_providers
                        .iter()
                        .find(|p| p.provider_id() == provider_id)
                        .ok_or_else(|| {
                            LuaError::runtime(format!("provider '{provider_id}' not found"))
                        })?
                };

                let effective_model = if model_id.is_empty() {
                    llm.list_models()
                        .await
                        .first()
                        .map(|m| m.id.clone())
                        .unwrap_or_default()
                } else {
                    model_id
                };

                let mut result_parts = Vec::new();
                llm.complete(
                    &messages,
                    &effective_model,
                    &[],
                    &[],
                    tool_choice.as_ref(),
                    &mut |chunk| {
                        for dp in &chunk.delta_parts {
                            if let crate::types::MessagePart::Text(tp) = dp {
                                result_parts.push(tp.text.clone());
                            }
                        }
                    },
                )
                .await
                .map_err(LuaError::external)?;

                let text: String = result_parts.concat();
                lua.create_string(&text)
            }
        },
    )?;
    Ok(complete_fn)
}

/// Builds the `ur.session` sub-module for read-only session access.
fn build_session_module(
    lua: &Lua,
    session_provider: &Arc<dyn SessionProvider>,
) -> Result<LuaTable> {
    let session = lua.create_table()?;

    let sp = Arc::clone(session_provider);
    let load_fn = lua.create_function(move |lua, session_id: String| {
        let events = sp.load_session(&session_id).map_err(LuaError::external)?;
        let json = serde_json::to_value(&events).map_err(LuaError::external)?;
        lua.to_value(&json)
    })?;
    session.set("load", load_fn)?;

    let sp = Arc::clone(session_provider);
    let list_fn = lua.create_function(move |lua, ()| {
        let sessions = sp.list_sessions().map_err(LuaError::external)?;
        let json = serde_json::to_value(&sessions).map_err(LuaError::external)?;
        lua.to_value(&json)
    })?;
    session.set("list", list_fn)?;

    Ok(session)
}

/// Builds the `ur.http` sub-module (gated on `network` capability).
fn build_http_module(lua: &Lua) -> Result<LuaTable> {
    let http = lua.create_table()?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(anyhow::Error::from)?;

    let get_client = client.clone();
    let get_fn =
        lua.create_async_function(move |lua, (url, opts): (String, Option<LuaTable>)| {
            let client = get_client.clone();
            async move {
                let mut builder = client.get(&url);

                if let Some(opts) = &opts
                    && let Ok(headers) = opts.get::<LuaTable>("headers")
                {
                    for pair in headers.pairs::<String, String>() {
                        let (key, value) = pair?;
                        builder = builder.header(key, value);
                    }
                }

                let response = builder.send().await.map_err(LuaError::external)?;
                let status = response.status().as_u16();
                let body = response.text().await.map_err(LuaError::external)?;

                let result = lua.create_table()?;
                result.set("status", status)?;
                result.set("body", body)?;
                Ok(result)
            }
        })?;
    http.set("get", get_fn)?;

    let post_client = client;
    let post_fn = lua.create_async_function(
        move |lua, (url, body, opts): (String, String, Option<LuaTable>)| {
            let client = post_client.clone();
            async move {
                let mut builder = client.post(&url).body(body);

                if let Some(opts) = &opts
                    && let Ok(headers) = opts.get::<LuaTable>("headers")
                {
                    for pair in headers.pairs::<String, String>() {
                        let (key, value) = pair?;
                        builder = builder.header(key, value);
                    }
                }

                let response = builder.send().await.map_err(LuaError::external)?;
                let status = response.status().as_u16();
                let body = response.text().await.map_err(LuaError::external)?;

                let result = lua.create_table()?;
                result.set("status", status)?;
                result.set("body", body)?;
                Ok(result)
            }
        },
    )?;
    http.set("post", post_fn)?;

    Ok(http)
}

/// Builds the `ur.fs` sub-module (gated on `fs-read`/`fs-write`).
fn build_fs_module(lua: &Lua, can_read: bool, can_write: bool) -> Result<LuaTable> {
    let fs = lua.create_table()?;

    if can_read {
        let read_fn = lua.create_function(|_, path: String| {
            std::fs::read_to_string(&path).map_err(LuaError::external)
        })?;
        fs.set("read", read_fn)?;

        let list_fn = lua.create_function(|lua, path: String| {
            let entries: Vec<String> = std::fs::read_dir(&path)
                .map_err(LuaError::external)?
                .filter_map(std::result::Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            lua.to_value(&entries)
        })?;
        fs.set("list", list_fn)?;
    }

    if can_write {
        let write_fn = lua.create_function(|_, (path, content): (String, String)| {
            std::fs::write(&path, &content).map_err(LuaError::external)
        })?;
        fs.set("write", write_fn)?;
    }

    Ok(fs)
}
