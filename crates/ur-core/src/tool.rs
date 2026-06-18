//! Tool trait and schema records.

use std::hash::{Hash, Hasher};

use crate::{BoxFuture, JsonError, JsonValue, Result};

/// A callable tool exposed to the model.
pub trait Tool: Send + Sync + 'static {
    /// Returns the stable tool name.
    fn name(&self) -> &str;

    /// Returns the tool schema advertised to providers.
    fn schema(&self) -> ToolSchema;

    /// Calls the tool with raw JSON arguments.
    fn call(&self, args: ToolArguments) -> BoxFuture<'static, std::result::Result<String, String>>;
}

impl<T: Tool + ?Sized> Tool for std::sync::Arc<T> {
    fn name(&self) -> &str {
        (**self).name()
    }

    fn schema(&self) -> ToolSchema {
        (**self).schema()
    }

    fn call(&self, args: ToolArguments) -> BoxFuture<'static, std::result::Result<String, String>> {
        (**self).call(args)
    }
}

/// Raw, unparsed tool-call arguments as delivered on the wire.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Deserialize, serde::Serialize),
    serde(transparent)
)]
pub struct ToolArguments(String);

impl ToolArguments {
    /// Creates tool arguments from raw JSON text.
    pub fn new(raw_json: impl Into<String>) -> Self {
        Self(raw_json.into())
    }

    /// Returns raw JSON text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Parses the raw JSON string into a typed value.
    pub fn parse<T: serde::de::DeserializeOwned>(&self) -> Result<T, JsonError> {
        serde_json::from_str(&self.0)
    }

    /// Parses the raw JSON string into a JSON value.
    pub fn to_value(&self) -> Result<JsonValue, JsonError> {
        self.parse()
    }
}

impl From<String> for ToolArguments {
    fn from(raw_json: String) -> Self {
        Self(raw_json)
    }
}

impl From<&str> for ToolArguments {
    fn from(raw_json: &str) -> Self {
        Self(raw_json.to_owned())
    }
}

impl std::fmt::Display for ToolArguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// JSON Schema metadata for a tool.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ToolSchema {
    /// Tool name.
    pub name: String,
    /// Optional human-readable tool description.
    pub description: Option<String>,
    /// JSON Schema for the parameters object.
    pub parameters: JsonValue,
    /// Whether the provider should use strict constrained-schema mode.
    pub strict: bool,
}

impl ToolSchema {
    /// Constructs a non-strict schema with no description.
    pub fn new(name: impl Into<String>, parameters: JsonValue) -> Self {
        Self {
            name: name.into(),
            description: None,
            parameters,
            strict: false,
        }
    }

    /// Sets the schema description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets whether strict mode should be requested for this schema.
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

impl Hash for ToolSchema {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.description.hash(state);
        hash_json_value(&self.parameters, state);
        self.strict.hash(state);
    }
}

fn hash_json_value<H: Hasher>(value: &JsonValue, state: &mut H) {
    std::mem::discriminant(value).hash(state);

    match value {
        JsonValue::Null => {}
        JsonValue::Bool(value) => value.hash(state),
        JsonValue::Number(value) => value.to_string().hash(state),
        JsonValue::String(value) => value.hash(state),
        JsonValue::Array(values) => values
            .iter()
            .for_each(|value| hash_json_value(value, state)),
        JsonValue::Object(values) => {
            let mut entries: Vec<_> = values.iter().collect();
            entries.sort_unstable_by_key(|(key, _)| *key);

            for (key, value) in entries {
                key.hash(state);
                hash_json_value(value, state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn tool_arguments_construct_parse_and_display() {
        let from_new = ToolArguments::new(r#"{"n":1}"#);
        let from_string = ToolArguments::from(String::from(r#"{"n":1}"#));
        let from_str = ToolArguments::from(r#"{"n":1}"#);

        assert_eq!(from_new, from_string);
        assert_eq!(from_new, from_str);
        assert_eq!(from_new.as_str(), r#"{"n":1}"#);
        assert_eq!(from_new.to_string(), r#"{"n":1}"#);

        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Args {
            n: u32,
        }

        assert_eq!(from_new.parse::<Args>().unwrap(), Args { n: 1 });
        assert_eq!(from_new.to_value().unwrap(), serde_json::json!({ "n": 1 }));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn tool_arguments_serde_is_transparent() {
        let arguments = ToolArguments::new(r#"{"n":1}"#);
        let encoded = serde_json::to_value(&arguments).unwrap();
        assert_eq!(encoded, serde_json::json!(r#"{"n":1}"#));

        let decoded: ToolArguments = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.as_str(), r#"{"n":1}"#);
    }

    #[test]
    fn tool_schema_builders_set_optional_fields() {
        let schema = ToolSchema::new("add", serde_json::json!({ "type": "object" }))
            .description("Add numbers")
            .strict(true);

        assert_eq!(schema.name, "add");
        assert_eq!(schema.description.as_deref(), Some("Add numbers"));
        assert_eq!(schema.parameters, serde_json::json!({ "type": "object" }));
        assert!(schema.strict);
    }

    #[test]
    fn equal_tool_schemas_have_equal_hashes() {
        use std::collections::hash_map::DefaultHasher;

        let left = ToolSchema::new(
            "lookup",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string" },
                    "units": { "type": "string" }
                }
            }),
        );
        let right = ToolSchema::new(
            "lookup",
            serde_json::json!({
                "properties": {
                    "units": { "type": "string" },
                    "city": { "type": "string" }
                },
                "type": "object"
            }),
        );

        assert_eq!(left, right);

        let mut left_hasher = DefaultHasher::new();
        let mut right_hasher = DefaultHasher::new();
        left.hash(&mut left_hasher);
        right.hash(&mut right_hasher);

        assert_eq!(left_hasher.finish(), right_hasher.finish());
    }

    #[test]
    fn tool_is_object_safe_behind_arc() {
        struct Echo;

        impl Tool for Echo {
            fn name(&self) -> &str {
                "echo"
            }

            fn schema(&self) -> ToolSchema {
                ToolSchema::new("echo", serde_json::json!({ "type": "object" }))
            }

            fn call(
                &self,
                args: ToolArguments,
            ) -> BoxFuture<'static, std::result::Result<String, String>> {
                Box::pin(async move { Ok(args.to_string()) })
            }
        }

        let tool: Arc<dyn Tool> = Arc::new(Echo);
        let shared = Arc::new(tool);

        assert_eq!(shared.name(), "echo");
        assert_eq!(shared.schema().name, "echo");
    }
}
