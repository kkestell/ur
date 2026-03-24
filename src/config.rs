//! User configuration for role-to-model mappings and extension settings.
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
    #[must_use]
    pub fn resolve_role(&self, role: &str) -> Option<(&str, &str)> {
        let ref_str = self.roles.get(role)?;
        parse_model_ref(ref_str)
    }

    /// Returns provider-specific settings for a model as typed `ConfigSetting` values.
    ///
    /// Filters `descriptors` (from `list-settings()`) to mutable, non-secret
    /// settings prefixed with `<model_id>.`, reads values from config, and
    /// returns `ConfigSetting` entries with short keys (prefix stripped).
    ///
    /// # Errors
    ///
    /// Returns an error if a setting value cannot be converted to the expected type.
    pub fn settings_for(
        &self,
        extension_id: &str,
        model_id: &str,
        descriptors: &[wit_types::SettingDescriptor],
    ) -> Result<Vec<wit_types::ConfigSetting>> {
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
            settings.push(wit_types::ConfigSetting {
                key: short_key.to_owned(),
                value,
            });
        }
        Ok(settings)
    }
}

/// Parses a `"provider/model"` reference into its two parts.
///
/// Splits on the first slash only, so `openrouter/openai/gpt-4o-mini`
/// yields `("openrouter", "openai/gpt-4o-mini")`.
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

/// Validates that a float falls within the schema's bounds.
///
/// # Errors
///
/// Returns an error if `n` is outside `[schema.min, schema.max]`.
pub(crate) fn validate_number(n: f64, schema: &wit_types::SettingNumber, key: &str) -> Result<()> {
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
/// Returns an error if `raw` cannot be parsed or validated for `schema`.
pub fn parse_setting_value(
    raw: &str,
    schema: &wit_types::SettingSchema,
    key: &str,
) -> Result<toml::Value> {
    match schema {
        wit_types::SettingSchema::Integer(int_schema) => {
            let n: i64 = raw
                .parse()
                .map_err(|_err| anyhow::anyhow!("setting '{key}': '{raw}' is not an integer"))?;
            validate_integer(n, int_schema, key)?;
            Ok(toml::Value::Integer(n))
        }
        wit_types::SettingSchema::Enumeration(enum_schema) => {
            validate_enum(raw, enum_schema, key)?;
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
            validate_number(n, num_schema, key)?;
            Ok(toml::Value::Float(n))
        }
        wit_types::SettingSchema::String(_) => Ok(toml::Value::String(raw.to_owned())),
    }
}

/// Converts a TOML value to a typed `SettingValue` according to the schema.
pub(crate) fn convert_toml_value(
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
        wit_types::SettingSchema::Number(num_schema) => {
            #[expect(
                clippy::cast_precision_loss,
                reason = "TOML integers used for float settings lose no practical precision"
            )]
            let n = val
                .as_float()
                .or_else(|| val.as_integer().map(|i| i as f64))
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected number"))?;
            validate_number(n, num_schema, key)?;
            Ok(wit_types::SettingValue::Number(n))
        }
        wit_types::SettingSchema::String(_) => {
            let s = val
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("setting '{key}': expected string"))?;
            Ok(wit_types::SettingValue::String(s.to_owned()))
        }
    }
}

/// Returns the default `SettingValue` for a schema.
pub(crate) fn default_value(schema: &wit_types::SettingSchema) -> wit_types::SettingValue {
    match schema {
        wit_types::SettingSchema::Integer(s) => wit_types::SettingValue::Integer(s.default_val),
        wit_types::SettingSchema::Enumeration(s) => {
            wit_types::SettingValue::Enumeration(s.default_val.clone())
        }
        wit_types::SettingSchema::Boolean(s) => wit_types::SettingValue::Boolean(s.default_val),
        wit_types::SettingSchema::Number(s) => wit_types::SettingValue::Number(s.default_val),
        wit_types::SettingSchema::String(s) => {
            wit_types::SettingValue::String(s.default_val.clone())
        }
    }
}

/// Returns the type name for a setting schema.
#[must_use]
pub fn schema_type_name(schema: &wit_types::SettingSchema) -> &'static str {
    match schema {
        wit_types::SettingSchema::Integer(_) => "integer",
        wit_types::SettingSchema::Enumeration(_) => "enum",
        wit_types::SettingSchema::Boolean(_) => "boolean",
        wit_types::SettingSchema::Number(_) => "number",
        wit_types::SettingSchema::String(_) => "string",
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
        assert_eq!(
            parse_model_ref("openrouter/openai/gpt-4o-mini"),
            Some(("openrouter", "openai/gpt-4o-mini"))
        );
    }

    #[test]
    fn parse_model_ref_openrouter_google_model() {
        assert_eq!(
            parse_model_ref("openrouter/google/gemini-2.5-flash"),
            Some(("openrouter", "google/gemini-2.5-flash"))
        );
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

    // --- config round-trip tests ---

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
    fn config_round_trip_dotted_key_settings() {
        let mut config = UserConfig::default();
        config
            .extensions
            .entry("google".into())
            .or_default()
            .insert(
                "gemini-flash.thinking_level".into(),
                toml::Value::String("high".into()),
            );

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let loaded: UserConfig = toml::from_str(&toml_str).unwrap();

        let val = &loaded.extensions["google"]["gemini-flash.thinking_level"];
        assert_eq!(val, &toml::Value::String("high".into()));
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

    fn int_descriptor(key: &str, min: i64, max: i64, default: i64) -> wit_types::SettingDescriptor {
        wit_types::SettingDescriptor {
            key: key.into(),
            name: key.into(),
            description: String::new(),
            schema: wit_types::SettingSchema::Integer(wit_types::SettingInteger {
                min,
                max,
                default_val: default,
            }),
            secret: false,
            readonly: false,
        }
    }

    #[test]
    fn settings_for_returns_defaults_when_no_overrides() {
        let config = UserConfig::default();
        let descriptors = vec![int_descriptor("model.budget", 0, 10000, 4000)];
        let settings = config
            .settings_for("provider", "model", &descriptors)
            .unwrap();
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
            .extensions
            .entry("provider".into())
            .or_default()
            .insert("model.budget".into(), toml::Value::Integer(8000));

        let descriptors = vec![int_descriptor("model.budget", 0, 10000, 4000)];
        let settings = config
            .settings_for("provider", "model", &descriptors)
            .unwrap();
        assert!(matches!(
            settings[0].value,
            wit_types::SettingValue::Integer(8000)
        ));
    }

    #[test]
    fn settings_for_skips_secret_and_readonly() {
        let config = UserConfig::default();
        let descriptors = vec![
            wit_types::SettingDescriptor {
                key: "api_key".into(),
                name: "API Key".into(),
                description: String::new(),
                schema: wit_types::SettingSchema::String(wit_types::SettingString {
                    default_val: String::new(),
                }),
                secret: true,
                readonly: false,
            },
            wit_types::SettingDescriptor {
                key: "model.context_window_in".into(),
                name: "Context".into(),
                description: String::new(),
                schema: wit_types::SettingSchema::Integer(wit_types::SettingInteger {
                    min: 0,
                    max: 1_000_000,
                    default_val: 1_000_000,
                }),
                secret: false,
                readonly: true,
            },
            int_descriptor("model.budget", 0, 10000, 4000),
        ];
        let settings = config
            .settings_for("provider", "model", &descriptors)
            .unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].key, "budget");
    }

    #[test]
    fn settings_for_rejects_invalid_override() {
        let mut config = UserConfig::default();
        config
            .extensions
            .entry("provider".into())
            .or_default()
            .insert("model.budget".into(), toml::Value::Integer(99999));

        let descriptors = vec![int_descriptor("model.budget", 0, 10000, 4000)];
        config
            .settings_for("provider", "model", &descriptors)
            .unwrap_err();
    }

    // --- validate_number tests ---

    #[test]
    fn validate_number_within_bounds() {
        let schema = wit_types::SettingNumber {
            min: 0.0,
            max: 2.0,
            default_val: 1.0,
        };
        validate_number(1.0, &schema, "k").unwrap();
    }

    #[test]
    fn validate_number_below_min() {
        let schema = wit_types::SettingNumber {
            min: 0.0,
            max: 2.0,
            default_val: 1.0,
        };
        assert!(validate_number(-0.1, &schema, "k").is_err());
    }

    #[test]
    fn validate_number_above_max() {
        let schema = wit_types::SettingNumber {
            min: 0.0,
            max: 2.0,
            default_val: 1.0,
        };
        assert!(validate_number(2.1, &schema, "k").is_err());
    }

    #[test]
    fn validate_number_at_boundaries() {
        let schema = wit_types::SettingNumber {
            min: 0.0,
            max: 2.0,
            default_val: 1.0,
        };
        validate_number(0.0, &schema, "k").unwrap();
        validate_number(2.0, &schema, "k").unwrap();
    }

    // --- number settings_for tests ---

    fn num_descriptor(key: &str, min: f64, max: f64, default: f64) -> wit_types::SettingDescriptor {
        wit_types::SettingDescriptor {
            key: key.into(),
            name: key.into(),
            description: String::new(),
            schema: wit_types::SettingSchema::Number(wit_types::SettingNumber {
                min,
                max,
                default_val: default,
            }),
            secret: false,
            readonly: false,
        }
    }

    #[test]
    fn settings_for_number_returns_default() {
        let config = UserConfig::default();
        let descriptors = vec![num_descriptor("model.temperature", 0.0, 2.0, 1.0)];
        let settings = config
            .settings_for("provider", "model", &descriptors)
            .unwrap();
        assert_eq!(settings[0].key, "temperature");
        assert!(matches!(
            settings[0].value,
            wit_types::SettingValue::Number(v) if (v - 1.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn settings_for_number_override_from_float() {
        let mut config = UserConfig::default();
        config
            .extensions
            .entry("provider".into())
            .or_default()
            .insert("model.temperature".into(), toml::Value::Float(0.7));

        let descriptors = vec![num_descriptor("model.temperature", 0.0, 2.0, 1.0)];
        let settings = config
            .settings_for("provider", "model", &descriptors)
            .unwrap();
        assert!(matches!(
            settings[0].value,
            wit_types::SettingValue::Number(v) if (v - 0.7).abs() < f64::EPSILON
        ));
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

    fn num_schema(min: f64, max: f64) -> wit_types::SettingSchema {
        wit_types::SettingSchema::Number(wit_types::SettingNumber {
            min,
            max,
            default_val: min,
        })
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
    fn parse_setting_value_enum_valid() {
        let v = parse_setting_value("high", &enum_schema(&["low", "high"]), "k").unwrap();
        assert_eq!(v, toml::Value::String("high".into()));
    }

    #[test]
    fn parse_setting_value_enum_invalid() {
        parse_setting_value("nope", &enum_schema(&["low", "high"]), "k").unwrap_err();
    }

    #[test]
    fn parse_setting_value_boolean() {
        let v = parse_setting_value("true", &bool_schema(), "k").unwrap();
        assert_eq!(v, toml::Value::Boolean(true));
    }

    #[test]
    fn parse_setting_value_number_valid() {
        let v = parse_setting_value("0.7", &num_schema(0.0, 2.0), "k").unwrap();
        assert_eq!(v, toml::Value::Float(0.7));
    }
}
