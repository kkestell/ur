use super::{TestEnv, api_key};
use predicates::prelude::*;

fn setup_openrouter_env() -> TestEnv {
    let key = api_key("OPENROUTER_API_KEY");
    let env = TestEnv::new();
    env.install_workspace_ext("test-extension");
    env.ur()
        .args(["extension", "enable", "test-extension"])
        .assert()
        .success();
    // Disable Google so OpenRouter is the sole llm-provider
    env.ur()
        .args(["extension", "disable", "llm-google"])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-openrouter",
            "set",
            "api_key",
            &key,
        ])
        .assert()
        .success();
    env.ur()
        .args(["role", "set", "default", "openrouter/qwen/qwen3.5-9b"])
        .assert()
        .success();
    env
}

#[test]
fn openrouter_basic() {
    let env = setup_openrouter_env();
    env.ur()
        .args(["run", "Say hello in one sentence"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn openrouter_tool_use() {
    let env = setup_openrouter_env();
    env.ur()
        .args([
            "run",
            "What is the weather in Paris, and should I wear a coat?",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}
