//! Model role management: provider queries and role resolution.
//!
//! Providers declare their available models via the WIT `llm-provider`
//! interface. This module collects those declarations and resolves
//! user-configured role mappings.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};

use crate::config::UserConfig;
use crate::extension_host::{self, wit_types};
use crate::manifest;

/// Provider ID → declared models, ordered alphabetically by provider.
pub type ProviderModels = BTreeMap<String, Vec<wit_types::ModelDescriptor>>;

/// Queries all enabled LLM providers for their declared provider ID and models.
pub fn collect_provider_models(
    engine: &wasmtime::Engine,
    manifest: &manifest::WorkspaceManifest,
) -> Result<ProviderModels> {
    let mut result = BTreeMap::new();
    for entry in &manifest.extensions {
        if !entry.enabled || entry.slot.as_deref() != Some("llm-provider") {
            continue;
        }
        let path = Path::new(&entry.wasm_path);
        let opts = extension_host::LoadOptions::for_entry(entry);
        // Probe with empty init to discover provider ID, then re-load
        // with real credentials so list_models() can make authenticated calls.
        let mut probe = extension_host::ExtensionInstance::load(engine, path, &opts)
            .map_err(|e| anyhow::anyhow!("loading {}: {e}", entry.id))?;
        let probe_init = probe
            .init(&[])
            .map_err(|e| anyhow::anyhow!("init {}: {e}", entry.id))?;
        if probe_init.is_err() {
            continue;
        }
        let Ok(Ok(provider_id)) = probe.provider_id() else {
            continue;
        };
        drop(probe);

        let init_config = crate::provider::init_config(&provider_id);
        let mut instance = extension_host::ExtensionInstance::load(engine, path, &opts)
            .map_err(|e| anyhow::anyhow!("loading {}: {e}", entry.id))?;
        let init_result = instance
            .init(&init_config)
            .map_err(|e| anyhow::anyhow!("init {}: {e}", entry.id))?;
        if let Err(e) = init_result {
            tracing::warn!(extension = %entry.id, error = %e, "init failed");
            continue;
        }
        let provider_id = instance
            .provider_id()?
            .map_err(|e| anyhow::anyhow!("{}: provider-id failed: {e}", entry.id))?;
        match instance.list_models()? {
            Ok(models) => {
                result.insert(provider_id, models);
            }
            Err(e) => tracing::warn!(extension = %entry.id, error = %e, "list-models failed"),
        }
    }
    Ok(result)
}

/// Resolves a role to `(provider_id, model_id)`.
///
/// Tries the requested role, falls back to `"default"`, then falls back
/// to the first provider's default model.
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
pub fn find_descriptor<'a>(
    provider_models: &'a ProviderModels,
    provider_id: &str,
    model_id: &str,
) -> Option<&'a wit_types::ModelDescriptor> {
    provider_models
        .get(provider_id)
        .and_then(|models| models.iter().find(|m| m.id == model_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str, is_default: bool) -> wit_types::ModelDescriptor {
        wit_types::ModelDescriptor {
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

    #[test]
    fn find_descriptor_unknown_model_in_known_provider() {
        let pm = sample_providers();
        assert!(find_descriptor(&pm, "anthropic", "nonexistent").is_none());
    }
}
