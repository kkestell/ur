//! User configuration for role-to-model mappings and extension settings.
//!
//! Config lives at `{ur_root}/config.toml` and is optional — the system
//! works with zero config via provider-declared defaults.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::types::{
    ConfigSetting, SettingDescriptor, SettingEnum, SettingInteger, SettingNumber, SettingSchema,
    SettingValue,
};

/// Top-level user configuration.
///
/// ```toml
/// [roles]
/// default = "google/gemini-3-flash-preview"
/// fast = "google/gemini-3.1-pro-preview"
///
/// [extensions.google]
/// "gemini-3-flash-preview.thinking_level" = "high"
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct UserConfig {
    /// Role name → "provider/model" mapping.
    #[serde(default)]
    pub roles: BTreeMap<String, String>,

    /// Extension ID → dotted-key settings.
    #[serde(default)]
    pub extensions: BTreeMap<String, BTreeMap<String, toml::Value>>,
}

impl UserConfig {
    /// Loads config from `{ur_root}/config.toml`.
    ///
    /// Returns `Default` if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn load(ur_root: &Path) -> Result<Self> {
        let path = ur_root.join("config.toml");
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                let config: Self = toml::from_str(&contents)?;
                info!(
                    path = %path.display(),
                    roles = config.roles.len(),
                    extensions = config.extensions.len(),
                    "config loaded"
                );
                Ok(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %path.display(), "no config file, using defaults");
                Ok(Self::default())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Saves config to `{ur_root}/config.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn save(&self, ur_root: &Path) -> Result<()> {
        std::fs::create_dir_all(ur_root)?;
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(ur_root.join("config.toml"), contents)?;
        Ok(())
    }

    /// Resolves a role name to `(provider_id, model_id)`.
    #[must_use]
    pub fn resolve_role(&self, role: &str) -> Option<(&str, &str)> {
        let ref_str = self.roles.get(role)?;
        parse_model_ref(ref_str)
    }

    /// Returns provider-specific settings for a model as typed `ConfigSetting` values.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn settings_for(
        &self,
        extension_id: &str,
        model_id: &str,
        descriptors: &[SettingDescriptor],
    ) -> Result<Vec<ConfigSetting>> {
        let overrides = self.extensions.get(extension_id);
        let prefix = format!("{model_id}.");

        let mut settings = Vec::new();
        for desc in descriptors {
            if desc.secret || desc.readonly || !desc.key.starts_with(&prefix) {
                continue;
            }
            let short_key = &desc.key[prefix.len()..];
            let value = match overrides.and_then(|o| o.get(&desc.key)) {
                Some(toml_val) => convert_toml_value(toml_val, &desc.schema, &desc.key)?,
                None => default_value(&desc.schema),
            };
            settings.push(ConfigSetting {
                key: short_key.to_owned(),
                value,
            });
        }
        Ok(settings)
    }
}

/// Parses a `"provider/model"` reference into its two parts.
#[must_use]
pub fn parse_model_ref(s: &str) -> Option<(&str, &str)> {
    let slash = s.find('/')?;
    let provider = &s[..slash];
    let model = &s[slash + 1..];
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider, model))
}

pub(crate) fn validate_integer(n: i64, schema: &SettingInteger, key: &str) -> Result<()> {
    if n < schema.min || n > schema.max {
        bail!(
            "setting '{key}': {n} is outside range {}..{}",
            schema.min,
            schema.max
        );
    }
    Ok(())
}

pub(crate) fn validate_enum(s: &str, schema: &SettingEnum, key: &str) -> Result<()> {
    if !schema.allowed.iter().any(|a| a == s) {
        bail!(
            "setting '{key}': '{s}' is not one of [{}]",
            schema.allowed.join(", ")
        );
    }
    Ok(())
}

pub(crate) fn validate_number(n: f64, schema: &SettingNumber, key: &str) -> Result<()> {
    if n < schema.min || n > schema.max {
        bail!(
            "setting '{key}': {n} is outside range {}..{}",
            schema.min,
            schema.max
        );
    }
    Ok(())
}

/// Parses a CLI string value into a TOML value according to the setting's schema.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn parse_setting_value(raw: &str, schema: &SettingSchema, key: &str) -> Result<toml::Value> {
    match schema {
        SettingSchema::Integer(int_schema) => {
            let n: i64 = raw
                .parse()
                .map_err(|_err| anyhow::anyhow!("setting '{key}': '{raw}' is not an integer"))?;
            validate_integer(n, int_schema, key)?;
            Ok(toml::Value::Integer(n))
        }
        SettingSchema::Enumeration(enum_schema) => {
            validate_enum(raw, enum_schema, key)?;
            Ok(toml::Value::String(raw.to_owned()))
        }
        SettingSchema::Boolean(_) => {
            let b: bool = raw
                .parse()
                .map_err(|_err| anyhow::anyhow!("setting '{key}': '{raw}' is not a boolean"))?;
            Ok(toml::Value::Boolean(b))
        }
        SettingSchema::Number(num_schema) => {
            let n: f64 = raw
                .parse()
                .map_err(|_err| anyhow::anyhow!("setting '{key}': '{raw}' is not a number"))?;
            validate_number(n, num_schema, key)?;
            Ok(toml::Value::Float(n))
        }
        SettingSchema::String(_) => Ok(toml::Value::String(raw.to_owned())),
    }
}

/// Converts a TOML value to a typed `SettingValue` according to the schema.
pub(crate) fn convert_toml_value(
    val: &toml::Value,
    schema: &SettingSchema,
    key: &str,
) -> Result<SettingValue> {
    match schema {
        SettingSchema::Integer(int_schema) => {
            let n = val
                .as_integer()
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected integer"))?;
            validate_integer(n, int_schema, key)?;
            Ok(SettingValue::Integer(n))
        }
        SettingSchema::Enumeration(enum_schema) => {
            let s = val
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected string"))?;
            validate_enum(s, enum_schema, key)?;
            Ok(SettingValue::Enumeration(s.to_owned()))
        }
        SettingSchema::Boolean(_) => {
            let b = val
                .as_bool()
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected boolean"))?;
            Ok(SettingValue::Boolean(b))
        }
        SettingSchema::Number(num_schema) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "TOML integers used for float settings lose no practical precision"
            )]
            let n = val
                .as_float()
                .or_else(|| val.as_integer().map(|i| i as f64))
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected number"))?;
            validate_number(n, num_schema, key)?;
            Ok(SettingValue::Number(n))
        }
        SettingSchema::String(_) => {
            let s = val
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected string"))?;
            Ok(SettingValue::String(s.to_owned()))
        }
    }
}

/// Returns the default `SettingValue` for a schema.
pub(crate) fn default_value(schema: &SettingSchema) -> SettingValue {
    match schema {
        SettingSchema::Integer(s) => SettingValue::Integer(s.default_val),
        SettingSchema::Enumeration(s) => SettingValue::Enumeration(s.default_val.clone()),
        SettingSchema::Boolean(s) => SettingValue::Boolean(s.default_val),
        SettingSchema::Number(s) => SettingValue::Number(s.default_val),
        SettingSchema::String(s) => SettingValue::String(s.default_val.clone()),
    }
}

/// Returns the type name for a setting schema.
#[must_use]
pub fn schema_type_name(schema: &SettingSchema) -> &'static str {
    match schema {
        SettingSchema::Integer(_) => "integer",
        SettingSchema::Enumeration(_) => "enum",
        SettingSchema::Boolean(_) => "boolean",
        SettingSchema::Number(_) => "number",
        SettingSchema::String(_) => "string",
    }
}

#[expect(dead_code, reason = "Will be used by future CLI display code")]
pub(crate) fn format_setting_value(val: &SettingValue) -> String {
    match val {
        SettingValue::Integer(n) => n.to_string(),
        SettingValue::Enumeration(s) | SettingValue::String(s) => s.clone(),
        SettingValue::Boolean(b) => b.to_string(),
        SettingValue::Number(n) => format!("{n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_ref_valid() {
        assert_eq!(
            parse_model_ref("anthropic/claude-sonnet-4-6"),
            Some(("anthropic", "claude-sonnet-4-6"))
        );
    }

    #[test]
    fn parse_model_ref_empty_provider() {
        assert_eq!(parse_model_ref("/model"), None);
    }

    #[test]
    fn parse_model_ref_empty_model() {
        assert_eq!(parse_model_ref("provider/"), None);
    }

    #[test]
    fn parse_model_ref_no_slash() {
        assert_eq!(parse_model_ref("justprovider"), None);
    }

    #[test]
    fn parse_model_ref_multiple_slashes() {
        assert_eq!(
            parse_model_ref("openrouter/openai/gpt-4o-mini"),
            Some(("openrouter", "openai/gpt-4o-mini"))
        );
    }

    #[test]
    fn resolve_role_returns_configured_role() {
        let mut config = UserConfig::default();
        config.roles.insert("fast".into(), "openai/gpt-5".into());
        assert_eq!(config.resolve_role("fast"), Some(("openai", "gpt-5")));
    }

    #[test]
    fn resolve_role_returns_none_for_unconfigured() {
        let config = UserConfig::default();
        assert_eq!(config.resolve_role("missing"), None);
    }

    #[test]
    fn config_round_trip_slash_model_id_in_role() {
        let mut config = UserConfig::default();
        config
            .roles
            .insert("default".into(), "openrouter/openai/gpt-4o-mini".into());

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: UserConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(
            loaded.resolve_role("default"),
            Some(("openrouter", "openai/gpt-4o-mini"))
        );
    }

    #[test]
    fn validate_integer_within_bounds() {
        let schema = SettingInteger {
            min: 0,
            max: 100,
            default_val: 50,
        };
        validate_integer(50, &schema, "k").unwrap();
    }

    #[test]
    fn validate_integer_below_min() {
        let schema = SettingInteger {
            min: 10,
            max: 100,
            default_val: 50,
        };
        assert!(validate_integer(5, &schema, "k").is_err());
    }

    #[test]
    fn validate_enum_allowed_value() {
        let schema = SettingEnum {
            allowed: vec!["a".into(), "b".into()],
            default_val: "a".into(),
        };
        validate_enum("b", &schema, "k").unwrap();
    }

    #[test]
    fn validate_enum_disallowed_value() {
        let schema = SettingEnum {
            allowed: vec!["a".into(), "b".into()],
            default_val: "a".into(),
        };
        assert!(validate_enum("c", &schema, "k").is_err());
    }

    #[test]
    fn parse_setting_value_integer_valid() {
        let schema = SettingSchema::Integer(SettingInteger {
            min: 0,
            max: 100,
            default_val: 0,
        });
        let v = parse_setting_value("50", &schema, "k").unwrap();
        assert_eq!(v, toml::Value::Integer(50));
    }

    #[test]
    fn settings_for_returns_defaults_when_no_overrides() {
        let config = UserConfig::default();
        let descriptors = vec![SettingDescriptor {
            key: "model.budget".into(),
            name: "budget".into(),
            description: String::new(),
            schema: SettingSchema::Integer(SettingInteger {
                min: 0,
                max: 10000,
                default_val: 4000,
            }),
            secret: false,
            readonly: false,
        }];
        let settings = config
            .settings_for("provider", "model", &descriptors)
            .unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].key, "budget");
        assert!(matches!(settings[0].value, SettingValue::Integer(4000)));
    }
}
