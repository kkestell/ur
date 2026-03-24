//! Extension config subcommand: list, get, set settings.

use std::path::Path;

use anyhow::Result;
use wasmtime::Engine;

use crate::extension_host::{ExtensionInstance, LoadOptions, wit_types};
use crate::manifest::{self, WorkspaceManifest};
use crate::provider;

/// Loads and initializes an extension by manifest ID, returning the instance
/// and the list of setting descriptors from `list-settings()`.
pub(crate) fn load_extension(
    engine: &Engine,
    manifest: &WorkspaceManifest,
    id: &str,
) -> Result<(ExtensionInstance, Vec<wit_types::SettingDescriptor>)> {
    let entry = manifest::find_entry(manifest, id)?;
    let opts = LoadOptions::for_entry(entry);
    let mut instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path), &opts)?;

    // For LLM providers, resolve API key via provider ID.
    if entry.slot.as_deref() == Some("llm-provider")
        && instance.init(&[])?.is_ok()
        && let Ok(Ok(provider_id)) = instance.provider_id()
    {
        // Re-load with credentials.
        drop(instance);
        let config = provider::init_config(&provider_id);
        instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path), &opts)?;
        instance
            .init(&config)?
            .map_err(|e| anyhow::anyhow!("init {id}: {e}"))?;

        // Call list_models to populate dynamic catalogs.
        let _ = instance.list_models();

        let descriptors = instance.list_settings()?;
        return Ok((instance, descriptors));
    }

    instance
        .init(&[])?
        .map_err(|e| anyhow::anyhow!("init {id}: {e}"))?;

    let descriptors = instance.list_settings()?;
    Ok((instance, descriptors))
}

/// Resolves the provider ID for an extension (LLM providers only).
pub(crate) fn resolve_provider_id(instance: &mut ExtensionInstance) -> Option<String> {
    instance
        .provider_id()
        .ok()
        .and_then(std::result::Result::ok)
}

/// Simple glob matching: supports `*` wildcard segments.
pub(crate) fn glob_match(pattern: &str, key: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return key.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return key.starts_with(prefix);
    }
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        return key.starts_with(prefix) && key.ends_with(suffix);
    }
    pattern == key
}

pub(crate) fn format_setting_value(val: &wit_types::SettingValue) -> String {
    match val {
        wit_types::SettingValue::Integer(n) => n.to_string(),
        wit_types::SettingValue::Enumeration(s) | wit_types::SettingValue::String(s) => s.clone(),
        wit_types::SettingValue::Boolean(b) => b.to_string(),
        wit_types::SettingValue::Number(n) => format!("{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_exact() {
        assert!(glob_match("api_key", "api_key"));
        assert!(!glob_match("api_key", "other"));
    }

    #[test]
    fn glob_match_star_only() {
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn glob_match_suffix_star() {
        assert!(glob_match("gemini-flash.*", "gemini-flash.thinking_level"));
        assert!(!glob_match("gemini-flash.*", "gemini-pro.thinking_level"));
    }

    #[test]
    fn glob_match_prefix_star() {
        assert!(glob_match(
            "*.thinking_level",
            "gemini-flash.thinking_level"
        ));
        assert!(!glob_match("*.thinking_level", "gemini-flash.cost_in"));
    }
}
