//! Extension settings utilities.
//!
//! Simplified for the native provider model. Settings are now managed
//! directly through the provider traits and user config.

/// Simple glob matching: supports `*` wildcard segments.
pub(crate) fn glob_match(pattern: &str, key: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return key.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return key.starts_with(prefix);
    }
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        return key.starts_with(prefix) && key.ends_with(suffix);
    }
    pattern == key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_exact() {
        assert!(glob_match("api_key", "api_key"));
        assert!(!glob_match("api_key", "other"));
    }

    #[test]
    fn glob_match_star_only() {
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn glob_match_suffix_star() {
        assert!(glob_match("gemini-flash.*", "gemini-flash.thinking_level"));
        assert!(!glob_match("gemini-flash.*", "gemini-pro.thinking_level"));
    }

    #[test]
    fn glob_match_prefix_star() {
        assert!(glob_match(
            "*.thinking_level",
            "gemini-flash.thinking_level"
        ));
        assert!(!glob_match("*.thinking_level", "gemini-flash.cost_in"));
    }
}
