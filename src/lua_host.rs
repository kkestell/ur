//! Lua extension runtime: sandboxed Luau VMs with capability-gated host APIs.
//!
//! Each extension gets its own isolated `mlua::Lua` instance with:
//! - Sandbox mode (restricted stdlib)
//! - Memory limits
//! - Interrupt-based execution timeouts
//! - Capability-gated host APIs via the `ur` module

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use mlua::prelude::*;
use tracing::info;

use crate::host_api::{self, HostProviders};
use crate::types::{ExtensionCapabilities, ToolDescriptor};

/// Default memory limit per VM: 64MB.
const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// Interrupt check budget: check every N instructions.
const INTERRUPT_BUDGET: u32 = 100_000;

/// A loaded Lua extension with its VM and registered handlers.
#[expect(
    missing_debug_implementations,
    reason = "Lua VM and registry keys do not implement Debug"
)]
pub struct LuaExtension {
    pub id: String,
    pub name: String,
    pub capabilities: ExtensionCapabilities,
    pub dir_path: PathBuf,
    lua: Lua,
    /// Tools registered by this extension via `ur.tool()`.
    tools: Arc<Mutex<Vec<RegisteredTool>>>,
    /// Hook handlers registered by this extension via `ur.hook()`.
    hooks: Arc<Mutex<Vec<RegisteredHook>>>,
}

/// A tool registered by a Lua extension.
#[expect(
    missing_debug_implementations,
    reason = "LuaRegistryKey does not implement Debug"
)]
pub struct RegisteredTool {
    pub descriptor: ToolDescriptor,
    /// Reference to the Lua handler function.
    pub handler_key: LuaRegistryKey,
}

/// A hook registered by a Lua extension.
#[expect(
    missing_debug_implementations,
    reason = "LuaRegistryKey does not implement Debug"
)]
pub struct RegisteredHook {
    pub hook_name: String,
    /// Reference to the Lua handler function.
    pub handler_key: LuaRegistryKey,
}

impl LuaExtension {
    /// Loads an extension from its directory.
    ///
    /// Reads `extension.toml`, creates a sandboxed VM, injects the `ur`
    /// module, and executes `init.lua`.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    pub fn load(
        dir_path: &Path,
        id: &str,
        name: &str,
        capabilities: &ExtensionCapabilities,
        config: &serde_json::Value,
        providers: &HostProviders,
    ) -> Result<Self> {
        let init_path = dir_path.join("init.lua");
        anyhow::ensure!(
            init_path.is_file(),
            "init.lua not found in {}",
            dir_path.display()
        );

        let lua = Lua::new();
        lua.sandbox(true)
            .map_err(|e| anyhow::anyhow!("failed to enable Lua sandbox: {e}"))?;
        lua.set_memory_limit(DEFAULT_MEMORY_LIMIT)
            .map_err(|e| anyhow::anyhow!("failed to set memory limit: {e}"))?;

        // Set up interrupt for execution timeouts.
        let interrupt_counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&interrupt_counter);
        lua.set_interrupt(move |_| {
            let n = counter_clone.fetch_add(1, Ordering::Relaxed);
            if n > INTERRUPT_BUDGET {
                Err(LuaError::runtime("extension execution timeout"))
            } else {
                Ok(LuaVmState::Continue)
            }
        });

        let tools: Arc<Mutex<Vec<RegisteredTool>>> = Arc::new(Mutex::new(Vec::new()));
        let hooks: Arc<Mutex<Vec<RegisteredHook>>> = Arc::new(Mutex::new(Vec::new()));

        // Build and inject the `ur` module.
        let ur_module =
            host_api::build_ur_module(&lua, capabilities, config, &tools, &hooks, providers)?;
        lua.globals()
            .set("ur", ur_module)
            .map_err(|e| anyhow::anyhow!("failed to inject ur module: {e}"))?;

        // Register custom `require` so `local ur = require("ur")` works.
        // In sandbox mode the built-in require is disabled, so we provide
        // a minimal one that only resolves "ur".
        let require_fn = lua.create_function(|lua, name: String| {
            if name == "ur" {
                lua.globals().get::<LuaValue>("ur")
            } else {
                Err(LuaError::runtime(format!(
                    "module '{name}' not found (only 'ur' is available)"
                )))
            }
        })?;
        lua.globals()
            .set("require", require_fn)
            .map_err(|e| anyhow::anyhow!("failed to inject require: {e}"))?;

        // Execute init.lua.
        let init_source = std::fs::read_to_string(&init_path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", init_path.display()))?;

        // Reset interrupt counter before executing init.
        interrupt_counter.store(0, Ordering::Relaxed);

        lua.load(&init_source)
            .set_name(format!("{id}/init.lua"))
            .exec()
            .map_err(|e| anyhow::anyhow!("executing {}: {e}", init_path.display()))?;

        let tool_count = tools.lock().unwrap().len();
        let hook_count = hooks.lock().unwrap().len();
        info!(
            id,
            tools = tool_count,
            hooks = hook_count,
            "loaded Lua extension"
        );

        Ok(Self {
            id: id.to_owned(),
            name: name.to_owned(),
            capabilities: capabilities.clone(),
            dir_path: dir_path.to_owned(),
            lua,
            tools,
            hooks,
        })
    }

    /// Returns the tool descriptors registered by this extension.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    #[must_use]
    pub fn tool_descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.descriptor.clone())
            .collect()
    }

    /// Returns the hook names registered by this extension.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    #[must_use]
    pub fn hook_names(&self) -> Vec<String> {
        self.hooks
            .lock()
            .unwrap()
            .iter()
            .map(|h| h.hook_name.clone())
            .collect()
    }

    /// Calls a tool handler registered by this extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    pub fn call_tool(&self, name: &str, arguments_json: &str) -> Result<String> {
        let handler_key = {
            let tools = self.tools.lock().unwrap();
            let tool = tools
                .iter()
                .find(|t| t.descriptor.name == name)
                .ok_or_else(|| anyhow::anyhow!("tool not found: {name}"))?;
            self.lua.registry_value::<LuaFunction>(&tool.handler_key)?
        };

        let args: LuaValue = self
            .lua
            .load(arguments_json)
            .eval()
            .or_else(|_| {
                // Try parsing as JSON and converting to Lua.
                let json: serde_json::Value = serde_json::from_str(arguments_json)?;
                self.lua.to_value(&json).map_err(anyhow::Error::from)
            })
            .unwrap_or(LuaValue::Nil);

        // Use call_async with block_on to allow Lua coroutines (async functions) to execute.
        let result: LuaValue = tokio::runtime::Handle::current()
            .block_on(handler_key.call_async(args))
            .map_err(|e| anyhow::anyhow!("calling tool handler {name}: {e}"))?;

        // Convert result to string.
        match result {
            LuaValue::String(s) => Ok(s.to_str()?.to_owned()),
            LuaValue::Nil => Ok(String::new()),
            other => {
                let json: serde_json::Value = self.lua.from_value(other)?;
                Ok(serde_json::to_string(&json)?)
            }
        }
    }

    /// Calls a hook handler and returns the result as a JSON value.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    pub fn call_hook(
        &self,
        hook_name: &str,
        context: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let handler_key = {
            let hooks = self.hooks.lock().unwrap();
            let hook = hooks
                .iter()
                .find(|h| h.hook_name == hook_name)
                .ok_or_else(|| anyhow::anyhow!("hook not registered: {hook_name}"))?;
            self.lua.registry_value::<LuaFunction>(&hook.handler_key)?
        };

        let ctx_lua: LuaValue = self.lua.to_value(context)?;
        // Use call_async with block_on to allow Lua coroutines (async functions) to execute.
        let result: LuaValue = tokio::runtime::Handle::current()
            .block_on(handler_key.call_async(ctx_lua))
            .map_err(|e| anyhow::anyhow!("calling hook handler {hook_name}: {e}"))?;

        let result_json: serde_json::Value = self.lua.from_value(result)?;
        Ok(result_json)
    }

    /// Returns whether this extension has a handler for the given hook.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    #[must_use]
    pub fn has_hook(&self, hook_name: &str) -> bool {
        self.hooks
            .lock()
            .unwrap()
            .iter()
            .any(|h| h.hook_name == hook_name)
    }
}
