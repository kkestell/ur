//! Model settings shared by providers.

/// Controls model thinking behavior when a provider supports it.
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

/// Desired response format.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ResponseFormat {
    /// Plain text output.
    #[default]
    Text,
    /// JSON object output.
    JsonObject,
}
