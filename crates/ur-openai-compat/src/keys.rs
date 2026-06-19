//! API-key resolution and end-user identifier validation shared by the
//! OpenAI-compatible providers.

use ur_core::{Error, Result};

/// Resolves an API key from an explicit value or the environment, rejecting an
/// empty result. `env_name` names the environment variable for the error
/// message.
pub fn resolve_api_key(
    explicit: Option<String>,
    from_env: Option<String>,
    env_name: &str,
) -> Result<String> {
    explicit
        .or(from_env)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| Error::Config {
            message: format!("no API key set and {env_name} is empty or unset"),
        })
}

/// Validates an end-user identifier: non-empty, at most `max_len` bytes, and
/// limited to ASCII alphanumerics, `_`, and `-`.
pub fn validate_user(user: &str, max_len: usize) -> Result<()> {
    let valid = !user.is_empty()
        && user.len() <= max_len
        && user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));

    if valid {
        Ok(())
    } else {
        Err(Error::Config {
            message: format!("invalid user '{user}'"),
        })
    }
}
