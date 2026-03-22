//! User configuration for role-to-model mappings and provider settings.
//!
//! Config lives at `{ur_root}/config.toml` and is optional — the system
//! works with zero config via provider-declared defaults.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::extension_host::wit_types;

/// Top-level user configuration.
///
/// ```toml
/// [roles]
/// default = "anthropic/claude-sonnet-4-6"
/// fast = "openai/gpt-5.4"
///
/// [providers.anthropic.claude-sonnet-4-6]
/// thinking_budget = 8000
/// ```
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct UserConfig {
    /// Role name → "provider/model" mapping.
    #[serde(default)]
    pub roles: BTreeMap<String, String>,

    /// Provider → model → setting key → value.
    #[serde(default)]
    pub providers: BTreeMap<String, BTreeMap<String, BTreeMap<String, toml::Value>>>,
}

impl UserConfig {
    /// Loads config from `{ur_root}/config.toml`.
    ///
    /// Returns `Default` if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read or parsed.
    pub fn load(ur_root: &Path) -> Result<Self> {
        let path = ur_root.join("config.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Saves config to `{ur_root}/config.toml`.
    ///
    /// Creates the directory and file if they don't exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written.
    pub fn save(&self, ur_root: &Path) -> Result<()> {
        std::fs::create_dir_all(ur_root)?;
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(ur_root.join("config.toml"), contents)?;
        Ok(())
    }

    /// Resolves a role name to `(provider_id, model_id)`.
    ///
    /// Returns `None` if the role is not configured.
    pub fn resolve_role(&self, role: &str) -> Option<(&str, &str)> {
        let ref_str = self.roles.get(role)?;
        parse_model_ref(ref_str)
    }

    /// Returns provider-specific settings for a model as typed `ConfigSetting` values.
    ///
    /// Settings are validated against the provided model descriptor's schema.
    /// Unknown keys are ignored; missing keys use defaults from the schema.
    pub fn settings_for(
        &self,
        provider: &str,
        model: &str,
        descriptor: &wit_types::ModelDescriptor,
    ) -> Result<Vec<wit_types::ConfigSetting>> {
        let overrides = self
            .providers
            .get(provider)
            .and_then(|models| models.get(model));

        let mut settings = Vec::new();
        for desc in &descriptor.settings {
            let value = match overrides.and_then(|o| o.get(&desc.key)) {
                Some(toml_val) => convert_toml_value(toml_val, &desc.schema, &desc.key)?,
                None => default_value(&desc.schema),
            };
            settings.push(wit_types::ConfigSetting {
                key: desc.key.clone(),
                value,
            });
        }
        Ok(settings)
    }
}

/// Parses a `"provider/model"` reference into its two parts.
pub fn parse_model_ref(s: &str) -> Option<(&str, &str)> {
    let slash = s.find('/')?;
    let provider = &s[..slash];
    let model = &s[slash + 1..];
    if provider.is_empty() || model.is_empty() || model.contains('/') {
        return None;
    }
    Some((provider, model))
}

/// Validates that an integer falls within the schema's bounds.
///
/// # Errors
///
/// Returns an error if `n` is outside `[schema.min, schema.max]`.
pub(crate) fn validate_integer(
    n: i64,
    schema: &wit_types::SettingInteger,
    key: &str,
) -> Result<()> {
    if n < schema.min || n > schema.max {
        bail!(
            "setting '{key}': {n} is outside range {}..{}",
            schema.min,
            schema.max
        );
    }
    Ok(())
}

/// Validates that a string is one of the schema's allowed values.
///
/// # Errors
///
/// Returns an error if `s` is not in `schema.allowed`.
pub(crate) fn validate_enum(s: &str, schema: &wit_types::SettingEnum, key: &str) -> Result<()> {
    if !schema.allowed.iter().any(|a| a == s) {
        bail!(
            "setting '{key}': '{s}' is not one of [{}]",
            schema.allowed.join(", ")
        );
    }
    Ok(())
}

/// Converts a TOML value to a typed `SettingValue` according to the schema.
fn convert_toml_value(
    val: &toml::Value,
    schema: &wit_types::SettingSchema,
    key: &str,
) -> Result<wit_types::SettingValue> {
    match schema {
        wit_types::SettingSchema::Integer(int_schema) => {
            let n = val
                .as_integer()
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected integer"))?;
            validate_integer(n, int_schema, key)?;
            Ok(wit_types::SettingValue::Integer(n))
        }
        wit_types::SettingSchema::Enumeration(enum_schema) => {
            let s = val
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected string"))?;
            validate_enum(s, enum_schema, key)?;
            Ok(wit_types::SettingValue::Enumeration(s.to_owned()))
        }
        wit_types::SettingSchema::Boolean(_) => {
            let b = val
                .as_bool()
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected boolean"))?;
            Ok(wit_types::SettingValue::Boolean(b))
        }
    }
}

/// Returns the default `SettingValue` for a schema.
fn default_value(schema: &wit_types::SettingSchema) -> wit_types::SettingValue {
    match schema {
        wit_types::SettingSchema::Integer(s) => wit_types::SettingValue::Integer(s.default_val),
        wit_types::SettingSchema::Enumeration(s) => {
            wit_types::SettingValue::Enumeration(s.default_val.clone())
        }
        wit_types::SettingSchema::Boolean(s) => wit_types::SettingValue::Boolean(s.default_val),
    }
}
