//! Model role management: provider queries and role resolution.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use tracing::info;

use crate::config::UserConfig;
use crate::providers::LlmProvider;
use crate::types::ModelDescriptor;

/// Provider ID → declared models, ordered alphabetically by provider.
pub type ProviderModels = BTreeMap<String, Vec<ModelDescriptor>>;

/// Collects provider models from all registered LLM providers.
pub async fn collect_provider_models(providers: &[&LlmProvider]) -> ProviderModels {
    let mut result = BTreeMap::new();
    for provider in providers {
        let models = provider.list_models().await;
        if !models.is_empty() {
            result.insert(provider.provider_id().to_owned(), models);
        }
    }
    info!(
        providers = result.len(),
        "provider model collection complete"
    );
    result
}

/// Resolves a role to `(provider_id, model_id)`.
///
/// Tries the requested role, falls back to `"default"`, then falls back
/// to the first provider's default model.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn resolve_role(
    config: &UserConfig,
    role: &str,
    provider_models: &ProviderModels,
) -> Result<(String, String)> {
    if let Some((p, m)) = config.resolve_role(role) {
        return Ok((p.to_owned(), m.to_owned()));
    }
    if role != "default"
        && let Some((p, m)) = config.resolve_role("default")
    {
        return Ok((p.to_owned(), m.to_owned()));
    }

    for (provider_id, models) in provider_models {
        if let Some(model) = models.iter().find(|m| m.is_default) {
            return Ok((provider_id.clone(), model.id.clone()));
        }
    }

    bail!("no LLM providers available")
}

/// Finds a model descriptor by provider and model ID.
#[must_use]
pub fn find_descriptor<'a>(
    provider_models: &'a ProviderModels,
    provider_id: &str,
    model_id: &str,
) -> Option<&'a ModelDescriptor> {
    provider_models
        .get(provider_id)
        .and_then(|models| models.iter().find(|m| m.id == model_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str, is_default: bool) -> ModelDescriptor {
        ModelDescriptor {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            is_default,
        }
    }

    fn sample_providers() -> ProviderModels {
        let mut pm = BTreeMap::new();
        pm.insert(
            "anthropic".into(),
            vec![
                descriptor("claude-sonnet", true),
                descriptor("claude-opus", false),
            ],
        );
        pm.insert("openai".into(), vec![descriptor("gpt-5", false)]);
        pm
    }

    #[test]
    fn resolve_role_explicit_mapping() {
        let mut config = UserConfig::default();
        config.roles.insert("fast".into(), "openai/gpt-5".into());
        let pm = sample_providers();
        let (p, m) = resolve_role(&config, "fast", &pm).unwrap();
        assert_eq!(p, "openai");
        assert_eq!(m, "gpt-5");
    }

    #[test]
    fn resolve_role_falls_back_to_default() {
        let mut config = UserConfig::default();
        config
            .roles
            .insert("default".into(), "anthropic/claude-opus".into());
        let pm = sample_providers();
        let (p, m) = resolve_role(&config, "unknown-role", &pm).unwrap();
        assert_eq!(p, "anthropic");
        assert_eq!(m, "claude-opus");
    }

    #[test]
    fn resolve_role_falls_back_to_first_provider_default_model() {
        let config = UserConfig::default();
        let pm = sample_providers();
        let (p, m) = resolve_role(&config, "anything", &pm).unwrap();
        assert_eq!(p, "anthropic");
        assert_eq!(m, "claude-sonnet");
    }

    #[test]
    fn resolve_role_no_providers_errors() {
        let config = UserConfig::default();
        let pm = BTreeMap::new();
        resolve_role(&config, "default", &pm).unwrap_err();
    }

    #[test]
    fn find_descriptor_known_provider_and_model() {
        let pm = sample_providers();
        let d = find_descriptor(&pm, "anthropic", "claude-opus").unwrap();
        assert_eq!(d.id, "claude-opus");
    }

    #[test]
    fn find_descriptor_unknown_provider() {
        let pm = sample_providers();
        assert!(find_descriptor(&pm, "google", "gemini").is_none());
    }
}
