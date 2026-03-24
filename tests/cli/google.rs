use super::{TestEnv, api_key};
use predicates::prelude::*;

fn setup_google_env() -> TestEnv {
    let key = api_key("GOOGLE_API_KEY");
    let env = TestEnv::new();
    env.install_workspace_ext("test-extension");
    env.ur()
        .args(["extension", "enable", "test-extension"])
        .assert()
        .success();
    env.ur()
        .args(["extension", "config", "llm-google", "set", "api_key", &key])
        .assert()
        .success();
    env
}

#[test]
fn google_flash_basic() {
    let env = setup_google_env();
    env.ur()
        .args(["role", "set", "default", "google/gemini-3-flash-preview"])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.thinking_level",
            "low",
        ])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.max_output_tokens",
            "1024",
        ])
        .assert()
        .success();
    env.ur()
        .args(["run", "Say hello in one sentence"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn google_pro_model() {
    let env = setup_google_env();
    env.ur()
        .args(["role", "set", "default", "google/gemini-3.1-pro-preview"])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3.1-pro-preview.thinking_level",
            "medium",
        ])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3.1-pro-preview.max_output_tokens",
            "1536",
        ])
        .assert()
        .success();
    env.ur()
        .args(["run", "Say hello in one sentence"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn google_thinking_level() {
    let env = setup_google_env();
    env.ur()
        .args(["role", "set", "default", "google/gemini-3-flash-preview"])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.thinking_level",
            "high",
        ])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.max_output_tokens",
            "2048",
        ])
        .assert()
        .success();
    env.ur()
        .args(["run", "Say hello in one sentence"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn google_max_output_tokens() {
    let env = setup_google_env();
    env.ur()
        .args(["role", "set", "default", "google/gemini-3-flash-preview"])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.max_output_tokens",
            "512",
        ])
        .assert()
        .success();
    env.ur()
        .args(["run", "Say hello in one sentence"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn google_consecutive_runs() {
    let env = setup_google_env();
    env.ur()
        .args(["role", "set", "default", "google/gemini-3-flash-preview"])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.thinking_level",
            "low",
        ])
        .assert()
        .success();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.max_output_tokens",
            "256",
        ])
        .assert()
        .success();
    env.ur()
        .args(["run", "Say hello"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
    env.ur()
        .args(["run", "Say goodbye"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}
