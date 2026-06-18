//! Model settings shared by providers.

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

/// Desired response format.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum ResponseFormat {
    /// Plain text output.
    #[default]
    Text,
    /// JSON object output.
    JsonObject,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::Hash;

    #[test]
    fn public_setting_traits_are_available() {
        fn assert_traits<T: Clone + Copy + std::fmt::Debug + Eq + Hash + Send + Sync + 'static>() {}

        assert_traits::<Thinking>();
        assert_traits::<ReasoningEffort>();
        assert_traits::<ResponseFormat>();

        assert_eq!(Thinking::default(), Thinking::Default);
        assert_eq!(ResponseFormat::default(), ResponseFormat::Text);
    }
}
