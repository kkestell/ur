//! Shared provider helpers for API key resolution.

/// Resolves the API key for a provider: env var > keyring > None.
pub fn resolve_api_key(provider_id: &str) -> Option<String> {
    let env_key = format!("{}_API_KEY", provider_id.to_uppercase());

    if let Ok(val) = std::env::var(&env_key) {
        if !val.is_empty() {
            tracing::debug!(%provider_id, source = "env", "resolved API key");
            return Some(val);
        }
    }

    match crate::keyring::get_api_key(provider_id) {
        Ok(Some(val)) if !val.is_empty() => {
            tracing::debug!(%provider_id, source = "keyring", "resolved API key");
            Some(val)
        }
        Ok(_) => {
            tracing::debug!(%provider_id, "no API key found");
            None
        }
        Err(e) => {
            tracing::warn!(%provider_id, error = %e, "keyring lookup failed");
            None
        }
    }
}

/// Returns init config entries for a provider (legacy compat).
pub fn init_config(provider_id: &str) -> Vec<(String, String)> {
    resolve_api_key(provider_id)
        .map(|key| vec![("api_key".into(), key)])
        .unwrap_or_default()
}
