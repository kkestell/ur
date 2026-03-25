//! The `ur` module exposed to Lua extensions.
//!
//! Builds the host API table that gets injected as `ur` global in each
//! extension's sandboxed VM.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use mlua::prelude::*;
use tracing::info;

use crate::lua_host::{RegisteredHook, RegisteredTool};
use crate::types::{ExtensionCapabilities, ToolDescriptor};

/// Builds the `ur` module table for a Lua extension.
pub fn build_ur_module(
    lua: &Lua,
    capabilities: &ExtensionCapabilities,
    config: &serde_json::Value,
    tools: &Arc<Mutex<Vec<RegisteredTool>>>,
    hooks: &Arc<Mutex<Vec<RegisteredHook>>>,
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
    let tools_clone = tools.clone();
    let tool_fn = lua.create_function(move |lua, (name, spec): (String, LuaTable)| {
        let description: String = spec.get("description").unwrap_or_default();
        let parameters: LuaValue = spec.get::<LuaValue>("parameters").unwrap_or(LuaValue::Nil);
        let handler: LuaFunction = spec
            .get("handler")
            .map_err(|_| LuaError::runtime("tool spec must include a 'handler' function"))?;

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
    let hooks_clone = hooks.clone();
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

/// Builds the `ur.http` sub-module (gated on `network` capability).
fn build_http_module(lua: &Lua) -> Result<LuaTable> {
    let http = lua.create_table()?;

    let get_fn =
        lua.create_async_function(|lua, (url, opts): (String, Option<LuaTable>)| async move {
            let client = reqwest::Client::new();
            let mut builder = client.get(&url);

            if let Some(opts) = &opts {
                if let Ok(headers) = opts.get::<LuaTable>("headers") {
                    for pair in headers.pairs::<String, String>() {
                        let (key, value) = pair?;
                        builder = builder.header(key, value);
                    }
                }
            }

            let response = builder.send().await.map_err(LuaError::external)?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(LuaError::external)?;

            let result = lua.create_table()?;
            result.set("status", status)?;
            result.set("body", body)?;
            Ok(result)
        })?;
    http.set("get", get_fn)?;

    let post_fn = lua.create_async_function(
        |lua, (url, body, opts): (String, String, Option<LuaTable>)| async move {
            let client = reqwest::Client::new();
            let mut builder = client.post(&url).body(body);

            if let Some(opts) = &opts {
                if let Ok(headers) = opts.get::<LuaTable>("headers") {
                    for pair in headers.pairs::<String, String>() {
                        let (key, value) = pair?;
                        builder = builder.header(key, value);
                    }
                }
            }

            let response = builder.send().await.map_err(LuaError::external)?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(LuaError::external)?;

            let result = lua.create_table()?;
            result.set("status", status)?;
            result.set("body", body)?;
            Ok(result)
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
                .filter_map(|e| e.ok())
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
