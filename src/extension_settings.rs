//! Extension config subcommand: list, get, set settings.

use std::path::Path;

use anyhow::{Result, bail};
use wasmtime::Engine;

use crate::config::{self, UserConfig};
use crate::extension_host::{ExtensionInstance, wit_types};
use crate::keyring;
use crate::manifest::{self, WorkspaceManifest};
use crate::provider;

/// Loads and initializes an extension by manifest ID, returning the instance
/// and the list of setting descriptors from `list-settings()`.
fn load_extension(
    engine: &Engine,
    manifest: &WorkspaceManifest,
    id: &str,
) -> Result<(ExtensionInstance, Vec<wit_types::SettingDescriptor>)> {
    let entry = manifest::find_entry(manifest, id)?;
    let mut instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path))?;

    // For LLM providers, resolve API key via provider ID.
    if entry.slot.as_deref() == Some("llm-provider")
        && instance.init(&[])?.is_ok()
        && let Ok(Ok(provider_id)) = instance.provider_id()
    {
        // Re-load with credentials.
        drop(instance);
        let config = provider::init_config(&provider_id);
        instance = ExtensionInstance::load(engine, Path::new(&entry.wasm_path))?;
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
fn resolve_provider_id(instance: &mut ExtensionInstance) -> Option<String> {
    instance
        .provider_id()
        .ok()
        .and_then(std::result::Result::ok)
}

/// `ur extension <id> config list [pattern]`
pub fn cmd_config_list(
    engine: &Engine,
    ur_root: &Path,
    manifest: &WorkspaceManifest,
    id: &str,
    pattern: Option<&str>,
) -> Result<()> {
    let (_, descriptors) = load_extension(engine, manifest, id)?;
    let config = UserConfig::load(ur_root)?;
    let overrides = config.extensions.get(id);

    println!("{:<40}{:<10}VALUE", "KEY", "TYPE");

    for desc in &descriptors {
        if let Some(pat) = pattern
            && !glob_match(pat, &desc.key)
        {
            continue;
        }

        let type_name = config::schema_type_name(&desc.schema);

        let value_display = if desc.secret {
            let has_secret = keyring::get_api_key(id)
                .ok()
                .flatten()
                .is_some_and(|v| !v.is_empty());
            if has_secret {
                "****".to_owned()
            } else {
                "(not set)".to_owned()
            }
        } else if desc.readonly {
            let val = config::default_value(&desc.schema);
            format!("{} (readonly)", format_setting_value(&val))
        } else if let Some(toml_val) = overrides.and_then(|o| o.get(&desc.key)) {
            match config::convert_toml_value(toml_val, &desc.schema, &desc.key) {
                Ok(val) => format_setting_value(&val),
                Err(_) => format!("{toml_val}"),
            }
        } else {
            let val = config::default_value(&desc.schema);
            format_setting_value(&val)
        };

        println!("{:<40}{:<10}{}", desc.key, type_name, value_display);
    }

    Ok(())
}

/// `ur extension <id> config get <key>`
pub fn cmd_config_get(
    engine: &Engine,
    ur_root: &Path,
    manifest: &WorkspaceManifest,
    id: &str,
    key: &str,
) -> Result<()> {
    let (_, descriptors) = load_extension(engine, manifest, id)?;

    let desc = descriptors
        .iter()
        .find(|d| d.key == key)
        .ok_or_else(|| anyhow::anyhow!("unknown setting '{key}' for extension '{id}'"))?;

    if desc.secret {
        let stored = keyring::get_api_key(id).ok().flatten();
        if stored.is_some() {
            println!("****");
        } else {
            println!("(not set)");
        }
        return Ok(());
    }

    if desc.readonly {
        let val = config::default_value(&desc.schema);
        println!("{}", format_setting_value(&val));
        return Ok(());
    }

    let config = UserConfig::load(ur_root)?;
    let overrides = config.extensions.get(id);

    let value = match overrides.and_then(|o| o.get(&desc.key)) {
        Some(toml_val) => config::convert_toml_value(toml_val, &desc.schema, &desc.key)?,
        None => config::default_value(&desc.schema),
    };
    println!("{}", format_setting_value(&value));
    Ok(())
}

/// `ur extension <id> config set <key> [value]`
pub fn cmd_config_set(
    engine: &Engine,
    ur_root: &Path,
    manifest: &WorkspaceManifest,
    id: &str,
    key: &str,
    value: Option<&str>,
) -> Result<()> {
    let (mut instance, descriptors) = load_extension(engine, manifest, id)?;

    let desc = descriptors
        .iter()
        .find(|d| d.key == key)
        .ok_or_else(|| anyhow::anyhow!("unknown setting '{key}' for extension '{id}'"))?;

    if desc.readonly {
        bail!("setting '{key}' is read-only");
    }

    if desc.secret {
        let secret_value = if let Some(v) = value {
            v.to_owned()
        } else {
            eprint!("{}: ", desc.name);
            rpassword::read_password()?
        };
        let secret_value = secret_value.trim();
        anyhow::ensure!(!secret_value.is_empty(), "value cannot be empty");

        // Store under provider ID for LLM extensions, extension ID otherwise.
        let keyring_id = resolve_provider_id(&mut instance).unwrap_or_else(|| id.to_owned());
        keyring::set_api_key(&keyring_id, secret_value)?;
        println!("{key} stored securely.");
        return Ok(());
    }

    let raw = value.ok_or_else(|| anyhow::anyhow!("value required for non-secret setting"))?;
    let toml_value = config::parse_setting_value(raw, &desc.schema, key)?;

    let mut config = UserConfig::load(ur_root)?;
    config
        .extensions
        .entry(id.to_owned())
        .or_default()
        .insert(key.to_owned(), toml_value);
    config.save(ur_root)?;

    println!("{id}: {key} = {raw}");
    Ok(())
}

/// Simple glob matching: supports `*` wildcard segments.
fn glob_match(pattern: &str, key: &str) -> bool {
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

fn format_setting_value(val: &wit_types::SettingValue) -> String {
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
