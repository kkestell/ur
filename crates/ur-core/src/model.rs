//! Model settings shared by providers.

use std::hash::{Hash, Hasher};

use crate::tool::hash_json_value;
use crate::{JsonSchema, JsonValue};

/// Controls model thinking behavior when a provider supports it.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum Thinking {
    /// Let the provider choose its default thinking behavior.
    #[default]
    Default,
    /// Request thinking when supported.
    Enabled,
    /// Disable thinking when supported.
    Disabled,
}

/// Requested reasoning effort.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ReasoningEffort {
    /// Low reasoning effort.
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
    /// Extra-high reasoning effort.
    ExtraHigh,
    /// Maximum reasoning effort.
    Max,
}

/// A JSON Schema the model must conform its output to.
///
/// Constructed by name from a Rust type with [`for_type`](Self::for_type), or
/// from a hand-built schema with [`new`](Self::new). With [`strict`](Self::strict)
/// left at its default of `true`, providers constrain decoding to the schema; a
/// provider that cannot enforce a given schema rejects the request before it is
/// sent.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct JsonSchemaFormat {
    /// Schema name advertised to the provider.
    pub name: String,
    /// Optional human-readable schema description.
    pub description: Option<String>,
    /// The JSON Schema for the response.
    pub schema: JsonValue,
    /// Whether the provider should constrain output to the schema.
    pub strict: bool,
}

impl JsonSchemaFormat {
    /// Constructs a strict schema format from a hand-built JSON Schema.
    pub fn new(name: impl Into<String>, schema: JsonValue) -> Self {
        Self {
            name: name.into(),
            description: None,
            schema,
            strict: true,
        }
    }

    /// Constructs a strict schema format by deriving the schema from a type.
    pub fn for_type<T: JsonSchema>(name: impl Into<String>) -> Self {
        let schema = schemars::SchemaGenerator::default()
            .into_root_schema_for::<T>()
            .to_value();
        Self::new(name, schema)
    }

    /// Sets the schema description.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets whether the provider should constrain output to the schema.
    pub fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

impl Hash for JsonSchemaFormat {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.description.hash(state);
        hash_json_value(&self.schema, state);
        self.strict.hash(state);
    }
}

/// Desired response format.
#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ResponseFormat {
    /// Plain text output.
    #[default]
    Text,
    /// JSON object output, without a schema (legacy JSON mode).
    JsonObject,
    /// JSON output constrained to a schema.
    JsonSchema(JsonSchemaFormat),
}

impl ResponseFormat {
    /// Constructs a [`JsonSchema`](Self::JsonSchema) format from a hand-built schema.
    pub fn json_schema(name: impl Into<String>, schema: JsonValue) -> Self {
        Self::JsonSchema(JsonSchemaFormat::new(name, schema))
    }

    /// Constructs a [`JsonSchema`](Self::JsonSchema) format by deriving the schema from a type.
    pub fn json_schema_for<T: JsonSchema>(name: impl Into<String>) -> Self {
        Self::JsonSchema(JsonSchemaFormat::for_type::<T>(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash;

    #[test]
    fn public_setting_traits_are_available() {
        fn assert_copy<T: Clone + Copy + std::fmt::Debug + Eq + Hash + Send + Sync + 'static>() {}
        fn assert_owned<T: Clone + std::fmt::Debug + Eq + Hash + Send + Sync + 'static>() {}

        assert_copy::<Thinking>();
        assert_copy::<ReasoningEffort>();
        // `ResponseFormat` owns a schema, so it is not `Copy`.
        assert_owned::<ResponseFormat>();
        assert_owned::<JsonSchemaFormat>();

        assert_eq!(Thinking::default(), Thinking::Default);
        assert_eq!(ResponseFormat::default(), ResponseFormat::Text);
    }

    #[test]
    fn json_schema_format_defaults_to_strict() {
        let format = JsonSchemaFormat::new("capital", serde_json::json!({ "type": "object" }));
        assert!(format.strict);
        assert_eq!(format.description, None);

        let loose = format.clone().strict(false).description("a capital");
        assert!(!loose.strict);
        assert_eq!(loose.description.as_deref(), Some("a capital"));
    }

    #[test]
    fn for_type_derives_an_object_schema() {
        #[derive(JsonSchema)]
        #[allow(dead_code)]
        struct Capital {
            city: String,
            country: String,
        }

        let format = JsonSchemaFormat::for_type::<Capital>("capital");
        assert_eq!(format.name, "capital");
        assert!(format.strict);
        assert_eq!(format.schema["type"], serde_json::json!("object"));
        assert!(format.schema["properties"].get("city").is_some());
    }

    #[test]
    fn equal_schema_formats_hash_equally() {
        let left = ResponseFormat::json_schema("p", serde_json::json!({ "a": 1, "b": 2 }));
        let right = ResponseFormat::json_schema("p", serde_json::json!({ "b": 2, "a": 1 }));
        assert_eq!(left, right);

        let mut left_hasher = DefaultHasher::new();
        let mut right_hasher = DefaultHasher::new();
        left.hash(&mut left_hasher);
        right.hash(&mut right_hasher);
        assert_eq!(left_hasher.finish(), right_hasher.finish());
    }
}
