//! Shared provider helpers used by both `model` and `turn` modules.

/// Resolves the API key for a provider: env var > keyring > empty.
///
/// Returns a list of `(key, value)` config entries suitable for passing
/// to `ExtensionInstance::init()`.
pub fn init_config(provider_id: &str) -> Vec<(String, String)> {
    let env_key = format!("{}_API_KEY", provider_id.to_uppercase());

    if let Ok(val) = std::env::var(&env_key) {
        tracing::debug!(%provider_id, source = "env", "resolved API key");
        return vec![("api_key".into(), val)];
    }

    match crate::keyring::get_api_key(provider_id) {
        Ok(Some(val)) => {
            tracing::debug!(%provider_id, source = "keyring", "resolved API key");
            return vec![("api_key".into(), val)];
        }
        Ok(None) => {
            tracing::debug!(%provider_id, "no API key found");
        }
        Err(e) => tracing::warn!(%provider_id, error = %e, "keyring lookup failed"),
    }

    vec![]
}
