//! Tool trait and schema records.

/// Placeholder tool trait.
pub trait Tool: Send + Sync + 'static {}

impl<T> Tool for std::sync::Arc<T> where T: Tool + ?Sized {}

/// Placeholder raw tool arguments.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ToolArguments(String);

impl ToolArguments {
    /// Creates placeholder tool arguments from raw JSON text.
    pub fn new(json: impl Into<String>) -> Self {
        Self(json.into())
    }

    /// Returns raw JSON text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Placeholder tool schema.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct ToolSchema {
    name: String,
}

impl ToolSchema {
    /// Creates a placeholder tool schema.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Returns the tool name.
    pub fn name(&self) -> &str {
        &self.name
    }
}
