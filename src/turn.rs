//! User message resolution for the tracer-bullet CLI flow.
//!
//! The agent turn orchestration itself lives in [`crate::session`].
//! This module retains the user-message helpers that the CLI uses
//! to determine what to send into `UrSession::run_turn()`.

const DEFAULT_RUN_USER_MESSAGE: &str = "Hello, please greet the world";

/// Environment variable name for overriding the run user message.
pub const RUN_USER_MESSAGE_ENV_VAR: &str = "UR_RUN_USER_MESSAGE";

/// Resolves the user message for a run, with env var override.
pub fn resolve_run_user_message(env_value: Option<String>) -> String {
    match env_value {
        Some(value) if !value.trim().is_empty() => value,
        _ => DEFAULT_RUN_USER_MESSAGE.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_run_user_message_uses_default_when_env_is_absent() {
        assert_eq!(
            resolve_run_user_message(None),
            "Hello, please greet the world"
        );
    }

    #[test]
    fn resolve_run_user_message_prefers_env_override() {
        assert_eq!(
            resolve_run_user_message(Some(
                "What is the weather in Paris, and should I wear a coat?".into(),
            )),
            "What is the weather in Paris, and should I wear a coat?"
        );
    }
}
