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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    #[must_use]
    pub fn as_str(self) -> &'static str {
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
/// Extensions are called in manifest-defined order for this hook point.
/// Each extension sees the (possibly modified) context from the previous
/// extension. If a `before_*` hook returns `{ action = "reject", reason = "..." }`,
/// the chain stops.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn run_hook(
    extensions: &[Arc<LuaExtension>],
    hook: HookPoint,
    context: serde_json::Value,
) -> Result<HookResult> {
    run_hook_ordered(extensions, hook, context, None)
}

/// Runs a hook chain with explicit ordering from the manifest.
///
/// If `ordering` is provided, extensions are called in that order.
/// Extensions not in the ordering are called after all ordered ones.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn run_hook_ordered(
    extensions: &[Arc<LuaExtension>],
    hook: HookPoint,
    mut context: serde_json::Value,
    ordering: Option<&[&str]>,
) -> Result<HookResult> {
    let hook_name = hook.as_str();

    // Build the execution order.
    let ordered_extensions: Vec<&Arc<LuaExtension>> = if let Some(order) = ordering {
        let mut ordered: Vec<&Arc<LuaExtension>> = Vec::new();
        for id in order {
            if let Some(ext) = extensions.iter().find(|e| e.id == *id) {
                ordered.push(ext);
            }
        }
        // Append any extensions not in the ordering.
        for ext in extensions {
            if !ordered.iter().any(|e| e.id == ext.id) {
                ordered.push(ext);
            }
        }
        ordered
    } else {
        extensions.iter().collect()
    };

    for ext in ordered_extensions {
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
                if let Some(obj) = result.as_object()
                    && let Some(ctx_obj) = context.as_object_mut()
                {
                    for (k, v) in obj {
                        if k != "action" {
                            ctx_obj.insert(k.clone(), v.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_api::HostProviders;
    use crate::types::ExtensionCapabilities;
    use std::io::Write;

    /// Creates a temp extension dir with the given init.lua source.
    fn temp_extension(lua_source: &str) -> (tempfile::TempDir, Arc<LuaExtension>) {
        let dir = tempfile::tempdir().unwrap();
        let init_path = dir.path().join("init.lua");
        let mut f = std::fs::File::create(&init_path).unwrap();
        f.write_all(lua_source.as_bytes()).unwrap();
        let ext = LuaExtension::load(
            dir.path(),
            "test-ext",
            "Test Extension",
            &ExtensionCapabilities::default(),
            &serde_json::json!({}),
            &HostProviders::default(),
        )
        .unwrap();
        (dir, Arc::new(ext))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hook_pass_returns_context_unchanged() {
        let (_dir, ext) = temp_extension(
            r#"
            ur.hook("before_completion", function(ctx)
                return { action = "pass" }
            end)
            "#,
        );
        let ctx = serde_json::json!({ "model": "test-model" });
        let result = run_hook(&[ext], HookPoint::BeforeCompletion, ctx).unwrap();
        match result {
            HookResult::Pass(v) => {
                assert_eq!(v["model"], "test-model");
            }
            HookResult::Rejected(_) => panic!("should not reject"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hook_modify_merges_into_context() {
        let (_dir, ext) = temp_extension(
            r#"
            ur.hook("before_completion", function(ctx)
                return { action = "modify", model = "overridden-model" }
            end)
            "#,
        );
        let ctx = serde_json::json!({ "model": "original", "extra": 42 });
        let result = run_hook(&[ext], HookPoint::BeforeCompletion, ctx).unwrap();
        match result {
            HookResult::Pass(v) => {
                assert_eq!(v["model"], "overridden-model");
                assert_eq!(v["extra"], 42); // preserved
            }
            HookResult::Rejected(_) => panic!("should not reject"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hook_reject_stops_chain() {
        let (_dir, ext) = temp_extension(
            r#"
            ur.hook("before_tool", function(ctx)
                return { action = "reject", reason = "forbidden" }
            end)
            "#,
        );
        let ctx = serde_json::json!({ "tool_name": "test" });
        let result = run_hook(&[ext], HookPoint::BeforeTool, ctx).unwrap();
        match result {
            HookResult::Rejected(reason) => {
                assert_eq!(reason, "forbidden");
            }
            HookResult::Pass(_) => panic!("should reject"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn after_hook_cannot_reject() {
        let (_dir, ext) = temp_extension(
            r#"
            ur.hook("after_completion", function(ctx)
                return { action = "reject", reason = "nope" }
            end)
            "#,
        );
        let ctx = serde_json::json!({ "model": "test" });
        let result = run_hook(&[ext], HookPoint::AfterCompletion, ctx).unwrap();
        // Should be pass, not rejected.
        assert!(matches!(result, HookResult::Pass(_)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ordered_dispatch_respects_ordering() {
        let (_dir_a, ext_a) = temp_extension(
            r#"
            ur.hook("before_completion", function(ctx)
                return { action = "modify", order = (ctx.order or "") .. "A" }
            end)
            "#,
        );
        // Need a second temp dir with a different extension id.
        let dir_b = tempfile::tempdir().unwrap();
        let init_path = dir_b.path().join("init.lua");
        std::fs::write(
            &init_path,
            r#"
            ur.hook("before_completion", function(ctx)
                return { action = "modify", order = (ctx.order or "") .. "B" }
            end)
            "#,
        )
        .unwrap();
        let ext_b = Arc::new(
            LuaExtension::load(
                dir_b.path(),
                "ext-b",
                "Ext B",
                &ExtensionCapabilities::default(),
                &serde_json::json!({}),
                &HostProviders::default(),
            )
            .unwrap(),
        );

        let extensions = vec![ext_a, ext_b];

        // Default order: A then B.
        let ctx = serde_json::json!({});
        let result = run_hook_ordered(&extensions, HookPoint::BeforeCompletion, ctx, None).unwrap();
        match &result {
            HookResult::Pass(v) => assert_eq!(v["order"], "AB"),
            HookResult::Rejected(_) => panic!("expected pass"),
        }

        // Reversed order: B then A.
        let ctx = serde_json::json!({});
        let result = run_hook_ordered(
            &extensions,
            HookPoint::BeforeCompletion,
            ctx,
            Some(&["ext-b", "test-ext"]),
        )
        .unwrap();
        match &result {
            HookResult::Pass(v) => assert_eq!(v["order"], "BA"),
            HookResult::Rejected(_) => panic!("expected pass"),
        }
    }
}
