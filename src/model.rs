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
        wit_types::SettingSchema::Number(num_schema) => {
            let n: f64 = raw
                .parse()
                .map_err(|_err| anyhow::anyhow!("setting '{key}': '{raw}' is not a number"))?;
            config::validate_number(n, num_schema, key)?;
            Ok(toml::Value::Float(n))
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
        // Probe with empty init to discover provider ID, then re-load
        // with real credentials so list_models() can make authenticated calls.
        let mut probe = extension_host::ExtensionInstance::load(engine, path)
            .map_err(|e| anyhow::anyhow!("loading {}: {e}", entry.id))?;
        let probe_init = probe
            .init(&[])
            .map_err(|e| anyhow::anyhow!("init {}: {e}", entry.id))?;
        if probe_init.is_err() {
            continue;
        }
        let Ok(Ok(provider_id)) = probe.provider_id() else {
            continue;
        };
        drop(probe);

        let init_config = crate::provider::init_config(&provider_id);
        let mut instance = extension_host::ExtensionInstance::load(engine, path)
            .map_err(|e| anyhow::anyhow!("loading {}: {e}", entry.id))?;
        let init_result = instance
            .init(&init_config)
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
/// to the first provider's default model.
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

    for (provider_id, models) in provider_models {
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
            wit_types::SettingSchema::Number(s) => {
                format!("number  {}..{}  (default: {})", s.min, s.max, s.default_val)
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

pub fn cmd_info(provider_models: &ProviderModels, model_ref: &str, property: &str) -> Result<()> {
    let (provider_id, model_id) = config::parse_model_ref(model_ref).ok_or_else(|| {
        anyhow::anyhow!("invalid model reference '{model_ref}' (expected provider/model)")
    })?;

    let descriptor = find_descriptor(provider_models, provider_id, model_id)
        .ok_or_else(|| anyhow::anyhow!("model '{model_ref}' not found in any enabled provider"))?;

    println!("{}", format_property(descriptor, property)?);
    Ok(())
}

fn format_property(descriptor: &wit_types::ModelDescriptor, property: &str) -> Result<String> {
    match property {
        "context_window_in" => Ok(descriptor.context_window_in.to_string()),
        "context_window_out" => Ok(descriptor.context_window_out.to_string()),
        "knowledge_cutoff" => Ok(descriptor.knowledge_cutoff.clone()),
        "cost_in" => Ok(format!("{:.2}", f64::from(descriptor.cost_in) / 1000.0)),
        "cost_out" => Ok(format!("{:.2}", f64::from(descriptor.cost_out) / 1000.0)),
        _ => bail!("unknown property '{property}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str, is_default: bool) -> wit_types::ModelDescriptor {
        wit_types::ModelDescriptor {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            is_default,
            settings: vec![],
            context_window_in: 1_000_000,
            context_window_out: 8_192,
            knowledge_cutoff: "2025-01".into(),
            cost_in: 0,
            cost_out: 0,
        }
    }

    fn sample_providers() -> ProviderModels {
        let mut pm = BTreeMap::new();
        pm.insert(
            "anthropic".into(),
            vec![
                descriptor("claude-sonnet", true),
                descriptor("claude-opus", false),
            ],
        );
        pm.insert("openai".into(), vec![descriptor("gpt-5", false)]);
        pm
    }

    // --- resolve_role tests ---

    #[test]
    fn resolve_role_explicit_mapping() {
        let mut config = UserConfig::default();
        config.roles.insert("fast".into(), "openai/gpt-5".into());
        let pm = sample_providers();
        let (p, m) = resolve_role(&config, "fast", &pm).unwrap();
        assert_eq!(p, "openai");
        assert_eq!(m, "gpt-5");
    }

    #[test]
    fn resolve_role_falls_back_to_default() {
        let mut config = UserConfig::default();
        config
            .roles
            .insert("default".into(), "anthropic/claude-opus".into());
        let pm = sample_providers();
        let (p, m) = resolve_role(&config, "unknown-role", &pm).unwrap();
        assert_eq!(p, "anthropic");
        assert_eq!(m, "claude-opus");
    }

    #[test]
    fn resolve_role_falls_back_to_first_provider_default_model() {
        let config = UserConfig::default();
        let pm = sample_providers();
        let (p, m) = resolve_role(&config, "anything", &pm).unwrap();
        assert_eq!(p, "anthropic");
        assert_eq!(m, "claude-sonnet");
    }

    #[test]
    fn resolve_role_no_providers_errors() {
        let config = UserConfig::default();
        let pm = BTreeMap::new();
        resolve_role(&config, "default", &pm).unwrap_err();
    }

    // --- find_descriptor tests ---

    #[test]
    fn find_descriptor_known_provider_and_model() {
        let pm = sample_providers();
        let d = find_descriptor(&pm, "anthropic", "claude-opus").unwrap();
        assert_eq!(d.id, "claude-opus");
    }

    #[test]
    fn find_descriptor_unknown_provider() {
        let pm = sample_providers();
        assert!(find_descriptor(&pm, "google", "gemini").is_none());
    }

    #[test]
    fn find_descriptor_unknown_model_in_known_provider() {
        let pm = sample_providers();
        assert!(find_descriptor(&pm, "anthropic", "nonexistent").is_none());
    }

    // --- parse_setting_value tests ---

    fn int_schema(min: i64, max: i64) -> wit_types::SettingSchema {
        wit_types::SettingSchema::Integer(wit_types::SettingInteger {
            min,
            max,
            default_val: min,
        })
    }

    fn enum_schema(allowed: &[&str]) -> wit_types::SettingSchema {
        wit_types::SettingSchema::Enumeration(wit_types::SettingEnum {
            allowed: allowed
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            default_val: allowed[0].to_string(),
        })
    }

    fn bool_schema() -> wit_types::SettingSchema {
        wit_types::SettingSchema::Boolean(wit_types::SettingBoolean { default_val: false })
    }

    #[test]
    fn parse_setting_value_integer_valid() {
        let v = parse_setting_value("50", &int_schema(0, 100), "k").unwrap();
        assert_eq!(v, toml::Value::Integer(50));
    }

    #[test]
    fn parse_setting_value_integer_out_of_bounds() {
        parse_setting_value("200", &int_schema(0, 100), "k").unwrap_err();
    }

    #[test]
    fn parse_setting_value_integer_non_numeric() {
        parse_setting_value("abc", &int_schema(0, 100), "k").unwrap_err();
    }

    #[test]
    fn parse_setting_value_enum_valid() {
        let v = parse_setting_value("high", &enum_schema(&["low", "high"]), "k").unwrap();
        assert_eq!(v, toml::Value::String("high".into()));
    }

    #[test]
    fn parse_setting_value_enum_invalid() {
        parse_setting_value("nope", &enum_schema(&["low", "high"]), "k").unwrap_err();
    }

    #[test]
    fn parse_setting_value_boolean_true() {
        let v = parse_setting_value("true", &bool_schema(), "k").unwrap();
        assert_eq!(v, toml::Value::Boolean(true));
    }

    #[test]
    fn parse_setting_value_boolean_false() {
        let v = parse_setting_value("false", &bool_schema(), "k").unwrap();
        assert_eq!(v, toml::Value::Boolean(false));
    }

    #[test]
    fn parse_setting_value_boolean_invalid() {
        parse_setting_value("yes", &bool_schema(), "k").unwrap_err();
    }

    // --- parse_setting_value number tests ---

    fn num_schema(min: f64, max: f64) -> wit_types::SettingSchema {
        wit_types::SettingSchema::Number(wit_types::SettingNumber {
            min,
            max,
            default_val: min,
        })
    }

    #[test]
    fn parse_setting_value_number_valid() {
        let v = parse_setting_value("0.7", &num_schema(0.0, 2.0), "k").unwrap();
        assert_eq!(v, toml::Value::Float(0.7));
    }

    #[test]
    fn parse_setting_value_number_integer_string() {
        let v = parse_setting_value("1", &num_schema(0.0, 2.0), "k").unwrap();
        assert_eq!(v, toml::Value::Float(1.0));
    }

    #[test]
    fn parse_setting_value_number_out_of_bounds() {
        parse_setting_value("5.0", &num_schema(0.0, 2.0), "k").unwrap_err();
    }

    #[test]
    fn parse_setting_value_number_non_numeric() {
        parse_setting_value("abc", &num_schema(0.0, 2.0), "k").unwrap_err();
    }

    // --- format_property tests ---

    fn descriptor_with_metadata() -> wit_types::ModelDescriptor {
        wit_types::ModelDescriptor {
            id: "flash".into(),
            name: "Flash".into(),
            description: String::new(),
            is_default: true,
            settings: vec![],
            context_window_in: 1_048_576,
            context_window_out: 65_536,
            knowledge_cutoff: "2025-01".into(),
            cost_in: 500,
            cost_out: 3000,
        }
    }

    #[test]
    fn format_property_context_window_in() {
        let d = descriptor_with_metadata();
        assert_eq!(format_property(&d, "context_window_in").unwrap(), "1048576");
    }

    #[test]
    fn format_property_context_window_out() {
        let d = descriptor_with_metadata();
        assert_eq!(format_property(&d, "context_window_out").unwrap(), "65536");
    }

    #[test]
    fn format_property_knowledge_cutoff() {
        let d = descriptor_with_metadata();
        assert_eq!(format_property(&d, "knowledge_cutoff").unwrap(), "2025-01");
    }

    #[test]
    fn format_property_cost_in_formats_as_dollars() {
        let d = descriptor_with_metadata();
        assert_eq!(format_property(&d, "cost_in").unwrap(), "0.50");
    }

    #[test]
    fn format_property_cost_out_formats_as_dollars() {
        let d = descriptor_with_metadata();
        assert_eq!(format_property(&d, "cost_out").unwrap(), "3.00");
    }

    #[test]
    fn format_property_unknown_errors() {
        let d = descriptor_with_metadata();
        format_property(&d, "nonexistent").unwrap_err();
    }

    // --- cmd_info tests ---

    fn providers_with_metadata() -> ProviderModels {
        let mut pm = BTreeMap::new();
        pm.insert("google".into(), vec![descriptor_with_metadata()]);
        pm
    }

    #[test]
    fn cmd_info_invalid_model_ref() {
        let pm = providers_with_metadata();
        cmd_info(&pm, "no-slash", "cost_in").unwrap_err();
    }

    #[test]
    fn cmd_info_unknown_model() {
        let pm = providers_with_metadata();
        cmd_info(&pm, "google/nonexistent", "cost_in").unwrap_err();
    }

    #[test]
    fn cmd_info_unknown_property() {
        let pm = providers_with_metadata();
        cmd_info(&pm, "google/flash", "nonexistent").unwrap_err();
    }

    #[test]
    fn cmd_info_valid() {
        let pm = providers_with_metadata();
        cmd_info(&pm, "google/flash", "cost_in").unwrap();
    }
}
