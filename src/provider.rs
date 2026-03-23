//! Shared provider helpers used by both `model` and `turn` modules.

/// Resolves the API key for a provider: env var > keyring > empty.
///
/// Returns a list of `(key, value)` config entries suitable for passing
/// to `ExtensionInstance::init()`.
pub fn init_config(provider_id: &str) -> Vec<(String, String)> {
    let env_key = format!("{}_API_KEY", provider_id.to_uppercase());

    if let Ok(val) = std::env::var(&env_key) {
        return vec![("api_key".into(), val)];
    }

    match crate::keyring::get_api_key(provider_id) {
        Ok(Some(val)) => return vec![("api_key".into(), val)],
        Ok(None) => {}
        Err(e) => eprintln!("warning: keyring lookup failed for {provider_id}: {e}"),
    }

    vec![]
}
