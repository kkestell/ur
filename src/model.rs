//! Model role management: provider queries, role resolution, CLI commands.
//!
//! Providers declare their available models and typed setting schemas via the
//! WIT `llm-provider` interface. This module collects those declarations,
//! resolves user-configured role mappings, and implements the `ur model` CLI.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};

use crate::config::{self, UserConfig};
use crate::extension_host::{self, wit_types};
use crate::manifest;

/// Parses a CLI string value into a TOML value according to the setting's schema.
fn parse_setting_value(
    raw: &str,
    schema: &wit_types::SettingSchema,
    key: &str,
) -> Result<toml::Value> {
    match schema {
        wit_types::SettingSchema::Integer(int_schema) => {
            let n: i64 = raw
                .parse()
                .map_err(|_err| anyhow::anyhow!("setting '{key}': '{raw}' is not an integer"))?;
            config::validate_integer(n, int_schema, key)?;
            Ok(toml::Value::Integer(n))
        }
        wit_types::SettingSchema::Enumeration(enum_schema) => {
            config::validate_enum(raw, enum_schema, key)?;
            Ok(toml::Value::String(raw.to_owned()))
        }
        wit_types::SettingSchema::Boolean(_) => {
            let b: bool = raw
                .parse()
                .map_err(|_err| anyhow::anyhow!("setting '{key}': '{raw}' is not a boolean"))?;
            Ok(toml::Value::Boolean(b))
        }
    }
}

/// Provider ID → declared models, ordered alphabetically by provider.
pub type ProviderModels = BTreeMap<String, Vec<wit_types::ModelDescriptor>>;

/// Queries all enabled LLM providers for their declared provider ID and models.
pub fn collect_provider_models(
    engine: &wasmtime::Engine,
    manifest: &manifest::WorkspaceManifest,
) -> Result<ProviderModels> {
    let mut result = BTreeMap::new();
    for entry in &manifest.extensions {
        if !entry.enabled || entry.slot.as_deref() != Some("llm-provider") {
            continue;
        }
        let path = Path::new(&entry.wasm_path);
        let mut instance =
            extension_host::ExtensionInstance::load(engine, path, Some("llm-provider"))
                .map_err(|e| anyhow::anyhow!("loading {}: {e}", entry.id))?;
        let init_result = instance
            .init(&[])
            .map_err(|e| anyhow::anyhow!("init {}: {e}", entry.id))?;
        if let Err(e) = init_result {
            eprintln!("warning: {}: init failed: {e}", entry.id);
            continue;
        }
        let provider_id = instance
            .provider_id()?
            .map_err(|e| anyhow::anyhow!("{}: provider-id failed: {e}", entry.id))?;
        match instance.list_models()? {
            Ok(models) => {
                result.insert(provider_id, models);
            }
            Err(e) => eprintln!("warning: {}: list-models failed: {e}", entry.id),
        }
    }
    Ok(result)
}

/// Resolves a role to `(provider_id, model_id)`.
///
/// Tries the requested role, falls back to `"default"`, then falls back
/// to the first provider's default model (zero-config).
pub fn resolve_role(
    config: &UserConfig,
    role: &str,
    provider_models: &ProviderModels,
) -> Result<(String, String)> {
    if let Some((p, m)) = config.resolve_role(role) {
        return Ok((p.to_owned(), m.to_owned()));
    }
    if role != "default"
        && let Some((p, m)) = config.resolve_role("default")
    {
        return Ok((p.to_owned(), m.to_owned()));
    }

    for (provider_id, models) in provider_models.iter() {
        if let Some(model) = models.iter().find(|m| m.is_default) {
            return Ok((provider_id.clone(), model.id.clone()));
        }
    }

    bail!("no LLM providers available")
}

/// Finds a model descriptor by provider and model ID.
pub fn find_descriptor<'a>(
    provider_models: &'a ProviderModels,
    provider_id: &str,
    model_id: &str,
) -> Option<&'a wit_types::ModelDescriptor> {
    provider_models
        .get(provider_id)
        .and_then(|models| models.iter().find(|m| m.id == model_id))
}

// --- CLI command handlers ---

pub fn cmd_list(config: &UserConfig, provider_models: &ProviderModels) -> Result<()> {
    println!("{:<12}MODEL", "ROLE");
    if !config.roles.contains_key("default") {
        let (p, model_id) = resolve_role(config, "default", provider_models)?;
        println!("{:<12}{p}/{model_id}", "default");
    }
    for (role, model_ref) in &config.roles {
        println!("{role:<12}{model_ref}");
    }
    Ok(())
}

pub fn cmd_get(config: &UserConfig, provider_models: &ProviderModels, role: &str) -> Result<()> {
    let (provider_id, model_id) = resolve_role(config, role, provider_models)?;
    println!("{role} -> {provider_id}/{model_id}");
    Ok(())
}

pub fn cmd_set(
    ur_root: &Path,
    config: &mut UserConfig,
    provider_models: &ProviderModels,
    role: &str,
    model_ref: &str,
) -> Result<()> {
    let (provider_id, model_id) = config::parse_model_ref(model_ref).ok_or_else(|| {
        anyhow::anyhow!("invalid model reference '{model_ref}' (expected provider/model)")
    })?;

    find_descriptor(provider_models, provider_id, model_id)
        .ok_or_else(|| anyhow::anyhow!("model '{model_ref}' not found in any enabled provider"))?;

    config.roles.insert(role.to_owned(), model_ref.to_owned());
    config.save(ur_root)?;

    println!("{role} -> {provider_id}/{model_id}");
    Ok(())
}

pub fn cmd_config(config: &UserConfig, provider_models: &ProviderModels, role: &str) -> Result<()> {
    let (provider_id, model_id) = resolve_role(config, role, provider_models)?;

    let descriptor = find_descriptor(provider_models, &provider_id, &model_id)
        .ok_or_else(|| anyhow::anyhow!("model '{provider_id}/{model_id}' descriptor not found"))?;

    println!("Settings for {provider_id}/{model_id}:");
    println!();

    if descriptor.settings.is_empty() {
        println!("  (no configurable settings)");
        return Ok(());
    }

    for setting in &descriptor.settings {
        let type_info = match &setting.schema {
            wit_types::SettingSchema::Integer(s) => {
                format!(
                    "integer  {}..{}  (default: {})",
                    s.min, s.max, s.default_val
                )
            }
            wit_types::SettingSchema::Enumeration(s) => {
                format!(
                    "enum  [{}]  (default: {})",
                    s.allowed.join(", "),
                    s.default_val
                )
            }
            wit_types::SettingSchema::Boolean(s) => {
                format!("boolean  (default: {})", s.default_val)
            }
        };
        println!("  {:<20}{type_info}", setting.key);
    }

    Ok(())
}

pub fn cmd_setting(
    ur_root: &Path,
    config: &mut UserConfig,
    provider_models: &ProviderModels,
    role: &str,
    key: &str,
    value: &str,
) -> Result<()> {
    let (provider_id, model_id) = resolve_role(config, role, provider_models)?;

    let descriptor = find_descriptor(provider_models, &provider_id, &model_id)
        .ok_or_else(|| anyhow::anyhow!("model '{provider_id}/{model_id}' descriptor not found"))?;

    let setting_desc = descriptor
        .settings
        .iter()
        .find(|s| s.key == key)
        .ok_or_else(|| anyhow::anyhow!("unknown setting '{key}' for {provider_id}/{model_id}"))?;

    let toml_value = parse_setting_value(value, &setting_desc.schema, key)?;

    config
        .providers
        .entry(provider_id.clone())
        .or_default()
        .entry(model_id.clone())
        .or_default()
        .insert(key.to_owned(), toml_value);
    config.save(ur_root)?;

    println!("{provider_id}/{model_id}: {key} = {value}");
    Ok(())
}
