//! Lifecycle hook dispatch across Lua extensions.
//!
//! Hooks are called in extension order (system > user > workspace).
//! `before_*` hooks can modify context or reject; `after_*` hooks
//! can modify results but not reject.

use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, warn};

use crate::lua_host::LuaExtension;

/// The 9 lifecycle hook points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPoint {
    BeforeCompletion,
    AfterCompletion,
    BeforeTool,
    AfterTool,
    BeforeSessionLoad,
    AfterSessionLoad,
    BeforeSessionAppend,
    BeforeCompaction,
    AfterCompaction,
}

impl HookPoint {
    fn as_str(self) -> &'static str {
        match self {
            Self::BeforeCompletion => "before_completion",
            Self::AfterCompletion => "after_completion",
            Self::BeforeTool => "before_tool",
            Self::AfterTool => "after_tool",
            Self::BeforeSessionLoad => "before_session_load",
            Self::AfterSessionLoad => "after_session_load",
            Self::BeforeSessionAppend => "before_session_append",
            Self::BeforeCompaction => "before_compaction",
            Self::AfterCompaction => "after_compaction",
        }
    }

    fn can_reject(self) -> bool {
        matches!(
            self,
            Self::BeforeCompletion
                | Self::BeforeTool
                | Self::BeforeSessionLoad
                | Self::BeforeSessionAppend
                | Self::BeforeCompaction
        )
    }
}

/// Result of running a hook chain.
#[derive(Debug)]
pub enum HookResult {
    /// All extensions passed; context may be modified.
    Pass(serde_json::Value),
    /// A `before_*` hook rejected the operation.
    Rejected(String),
}

/// Runs a hook chain across all extensions that registered for it.
///
/// Extensions are called in order. Each extension sees the (possibly
/// modified) context from the previous extension. If a `before_*` hook
/// returns `{ action = "reject", reason = "..." }`, the chain stops.
pub fn run_hook(
    extensions: &[Arc<LuaExtension>],
    hook: HookPoint,
    mut context: serde_json::Value,
) -> Result<HookResult> {
    let hook_name = hook.as_str();

    for ext in extensions {
        if !ext.has_hook(hook_name) {
            continue;
        }

        debug!(extension = %ext.id, hook = hook_name, "calling hook");

        let result = match ext.call_hook(hook_name, &context) {
            Ok(v) => v,
            Err(e) => {
                warn!(
                    extension = %ext.id,
                    hook = hook_name,
                    error = %e,
                    "hook handler failed"
                );
                continue;
            }
        };

        // Parse the hook result table.
        let action = result
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("pass");

        match action {
            "pass" => {
                // No changes, continue chain.
            }
            "modify" => {
                // Merge modifications into context.
                if let Some(obj) = result.as_object() {
                    if let Some(ctx_obj) = context.as_object_mut() {
                        for (k, v) in obj {
                            if k != "action" {
                                ctx_obj.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
            }
            "reject" if hook.can_reject() => {
                let reason = result
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("rejected by extension")
                    .to_owned();
                debug!(
                    extension = %ext.id,
                    hook = hook_name,
                    reason = %reason,
                    "hook rejected"
                );
                return Ok(HookResult::Rejected(reason));
            }
            "reject" => {
                warn!(
                    extension = %ext.id,
                    hook = hook_name,
                    "after-hooks cannot reject, ignoring"
                );
            }
            other => {
                warn!(
                    extension = %ext.id,
                    hook = hook_name,
                    action = other,
                    "unknown hook action, treating as pass"
                );
            }
        }
    }

    Ok(HookResult::Pass(context))
}
