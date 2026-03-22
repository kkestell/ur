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

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_model_ref tests ---

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
        assert_eq!(parse_model_ref("a/b/c"), None);
    }

    // --- resolve_role tests ---

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

    // --- validate_integer tests ---

    #[test]
    fn validate_integer_within_bounds() {
        let schema = wit_types::SettingInteger {
            min: 0,
            max: 100,
            default_val: 50,
        };
        validate_integer(50, &schema, "k").unwrap();
    }

    #[test]
    fn validate_integer_below_min() {
        let schema = wit_types::SettingInteger {
            min: 10,
            max: 100,
            default_val: 50,
        };
        assert!(validate_integer(5, &schema, "k").is_err());
    }

    #[test]
    fn validate_integer_above_max() {
        let schema = wit_types::SettingInteger {
            min: 0,
            max: 100,
            default_val: 50,
        };
        assert!(validate_integer(101, &schema, "k").is_err());
    }

    #[test]
    fn validate_integer_at_boundaries() {
        let schema = wit_types::SettingInteger {
            min: 0,
            max: 100,
            default_val: 50,
        };
        validate_integer(0, &schema, "k").unwrap();
        validate_integer(100, &schema, "k").unwrap();
    }

    // --- validate_enum tests ---

    #[test]
    fn validate_enum_allowed_value() {
        let schema = wit_types::SettingEnum {
            allowed: vec!["a".into(), "b".into()],
            default_val: "a".into(),
        };
        validate_enum("b", &schema, "k").unwrap();
    }

    #[test]
    fn validate_enum_disallowed_value() {
        let schema = wit_types::SettingEnum {
            allowed: vec!["a".into(), "b".into()],
            default_val: "a".into(),
        };
        assert!(validate_enum("c", &schema, "k").is_err());
    }

    // --- settings_for tests ---

    fn make_descriptor(settings: Vec<wit_types::SettingDescriptor>) -> wit_types::ModelDescriptor {
        wit_types::ModelDescriptor {
            id: "test-model".into(),
            name: "Test Model".into(),
            description: String::new(),
            is_default: false,
            settings,
        }
    }

    fn int_setting(key: &str, min: i64, max: i64, default: i64) -> wit_types::SettingDescriptor {
        wit_types::SettingDescriptor {
            key: key.into(),
            name: key.into(),
            description: String::new(),
            schema: wit_types::SettingSchema::Integer(wit_types::SettingInteger {
                min,
                max,
                default_val: default,
            }),
        }
    }

    #[test]
    fn settings_for_returns_defaults_when_no_overrides() {
        let config = UserConfig::default();
        let desc = make_descriptor(vec![int_setting("budget", 0, 10000, 4000)]);
        let settings = config.settings_for("anthropic", "claude", &desc).unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].key, "budget");
        assert!(matches!(
            settings[0].value,
            wit_types::SettingValue::Integer(4000)
        ));
    }

    #[test]
    fn settings_for_returns_overridden_values() {
        let mut config = UserConfig::default();
        config
            .providers
            .entry("anthropic".into())
            .or_default()
            .entry("claude".into())
            .or_default()
            .insert("budget".into(), toml::Value::Integer(8000));

        let desc = make_descriptor(vec![int_setting("budget", 0, 10000, 4000)]);
        let settings = config.settings_for("anthropic", "claude", &desc).unwrap();
        assert!(matches!(
            settings[0].value,
            wit_types::SettingValue::Integer(8000)
        ));
    }

    #[test]
    fn settings_for_rejects_invalid_override() {
        let mut config = UserConfig::default();
        config
            .providers
            .entry("anthropic".into())
            .or_default()
            .entry("claude".into())
            .or_default()
            .insert("budget".into(), toml::Value::Integer(99999));

        let desc = make_descriptor(vec![int_setting("budget", 0, 10000, 4000)]);
        config
            .settings_for("anthropic", "claude", &desc)
            .unwrap_err();
    }
}
