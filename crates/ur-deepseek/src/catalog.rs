//! Compiled-in DeepSeek model catalog and deprecation notices.

use ur_core::provider::{ModelNotice, ModelSpec};

/// Every catalogued DeepSeek model shares this context window and output cap.
const CONTEXT_WINDOW: u32 = 1_000_000;
const MAX_OUTPUT: u32 = 384_000;

/// Model ids served by this provider.
const MODEL_IDS: &[&str] = &[
    "deepseek-v4-flash",
    "deepseek-v4-pro",
    "deepseek-chat",
    "deepseek-reasoner",
];

/// Model ids that still resolve but are scheduled for removal.
const DEPRECATED_MODEL_IDS: &[&str] = &["deepseek-chat", "deepseek-reasoner"];

/// Returns static catalog facts for a known DeepSeek model id.
pub(crate) fn model_spec(model_id: &str) -> Option<ModelSpec> {
    MODEL_IDS
        .contains(&model_id)
        .then(|| ModelSpec::new(CONTEXT_WINDOW, MAX_OUTPUT))
}

/// Returns a deprecation notice for a removed-soon DeepSeek model id.
pub(crate) fn model_notice(model_id: &str) -> Option<ModelNotice> {
    DEPRECATED_MODEL_IDS
        .contains(&model_id)
        .then(|| ModelNotice::Deprecated {
            message: format!("model '{model_id}' is deprecated and will be removed on 2026-07-24"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_ids_share_the_catalog_spec() {
        for id in MODEL_IDS {
            assert_eq!(model_spec(id), Some(ModelSpec::new(1_000_000, 384_000)));
        }
    }

    #[test]
    fn unknown_ids_have_no_spec() {
        assert_eq!(model_spec("gpt-4"), None);
        assert_eq!(model_spec("deepseek-v3"), None);
    }

    #[test]
    fn only_legacy_ids_are_deprecated() {
        for id in DEPRECATED_MODEL_IDS {
            assert!(matches!(
                model_notice(id),
                Some(ModelNotice::Deprecated { .. })
            ));
        }
        assert_eq!(model_notice("deepseek-v4-pro"), None);
        assert_eq!(model_notice("deepseek-v4-flash"), None);
        assert_eq!(model_notice("unknown"), None);
    }
}
