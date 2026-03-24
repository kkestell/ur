use super::TestEnv;
use predicates::prelude::*;

// --- Discovery & listing ---

#[test]
fn list_shows_system_extensions() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("llm-google")
                .and(predicate::str::contains("llm-openrouter"))
                .and(predicate::str::contains("compaction-llm"))
                .and(predicate::str::contains("session-jsonl"))
                .and(predicate::str::contains("system")),
        );
}

#[test]
fn list_shows_enabled_status() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("✓"));
}

// --- Enable/disable ---

#[test]
fn disable_extension() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "disable", "llm-google"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Disabled llm-google"));
}

#[test]
fn enable_extension() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "disable", "llm-google"])
        .assert()
        .success();
    env.ur()
        .args(["extension", "enable", "llm-google"])
        .assert()
        .success();
}

#[test]
fn enable_unknown_extension_fails() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "enable", "nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("extension not found"));
}

#[test]
fn disable_violating_exactly_one_fails() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "disable", "session-jsonl"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("only"));
}

#[test]
fn disable_violating_at_least_one_fails() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "disable", "llm-google"])
        .assert()
        .success();
    env.ur()
        .args(["extension", "disable", "llm-openrouter"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("only"));
}

// --- Inspect ---

#[test]
fn inspect_shows_extension_details() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "inspect", "session-jsonl"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("id:")
                .and(predicate::str::contains("session-jsonl"))
                .and(predicate::str::contains("name:"))
                .and(predicate::str::contains("slot:"))
                .and(predicate::str::contains("source:"))
                .and(predicate::str::contains("path:"))
                .and(predicate::str::contains("checksum:"))
                .and(predicate::str::contains("sha256:")),
        );
}

#[test]
fn inspect_unknown_extension_fails() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "inspect", "nonexistent"])
        .assert()
        .failure();
}

// --- Workspace extensions ---

#[test]
fn workspace_extension_discovered() {
    let env = TestEnv::new();
    env.install_workspace_ext("test-extension");
    env.ur()
        .args(["extension", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("test-extension").and(predicate::str::contains("workspace")),
        );
}

#[test]
fn workspace_extension_enable_disable() {
    let env = TestEnv::new();
    env.install_workspace_ext("test-extension");

    env.ur()
        .args(["extension", "enable", "test-extension"])
        .assert()
        .success();

    env.ur()
        .args(["extension", "enable", "test-extension"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already enabled"));

    env.ur()
        .args(["extension", "disable", "test-extension"])
        .assert()
        .success();
}

// --- Config (settings) ---

#[test]
fn config_list() {
    let env = TestEnv::new();
    env.ur()
        .args(["extension", "config", "llm-google", "list"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("thinking_level")
                .and(predicate::str::contains("max_output_tokens")),
        );
}

#[test]
fn config_list_pattern() {
    let env = TestEnv::new();
    let output = env
        .ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "list",
            "gemini-3-flash*",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&output);
    assert!(stdout.contains("gemini-3-flash-preview"));
    assert!(!stdout.contains("gemini-3.1-pro-preview"));
}

#[test]
fn config_get() {
    let env = TestEnv::new();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "get",
            "gemini-3-flash-preview.context_window_in",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("1048576"));
}

#[test]
fn config_set_enum() {
    let env = TestEnv::new();
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
            "get",
            "gemini-3-flash-preview.thinking_level",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("low"));
}

#[test]
fn config_set_invalid_enum_fails() {
    let env = TestEnv::new();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.thinking_level",
            "ultra",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not one of"));
}

#[test]
fn config_set_integer_bounds_fails() {
    let env = TestEnv::new();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.max_output_tokens",
            "0",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("outside range"));
}

#[test]
fn config_set_readonly_fails() {
    let env = TestEnv::new();
    env.ur()
        .args([
            "extension",
            "config",
            "llm-google",
            "set",
            "gemini-3-flash-preview.context_window_in",
            "500000",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("read-only"));
}
