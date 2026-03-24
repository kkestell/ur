use super::TestEnv;
use predicates::prelude::*;

#[test]
fn role_list_shows_default() {
    let env = TestEnv::new();
    env.ur()
        .args(["role", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("default"));
}

#[test]
fn role_get_default() {
    let env = TestEnv::new();
    env.ur()
        .args(["role", "get", "default"])
        .assert()
        .success()
        .stdout(predicate::str::contains("google/gemini-3-flash-preview"));
}

#[test]
fn role_set_and_get() {
    let env = TestEnv::new();
    env.ur()
        .args(["role", "set", "myrole", "google/gemini-3-flash-preview"])
        .assert()
        .success();
    env.ur()
        .args(["role", "get", "myrole"])
        .assert()
        .success()
        .stdout(predicate::str::contains("google/gemini-3-flash-preview"));
}

#[test]
fn role_set_persists_to_config() {
    let env = TestEnv::new();
    env.ur()
        .args(["role", "set", "myrole", "google/gemini-3-flash-preview"])
        .assert()
        .success();
    let config_path = env.ur_root.path().join("config.toml");
    let contents = std::fs::read_to_string(&config_path).expect("read config.toml");
    assert!(
        contents.contains("myrole"),
        "config.toml should contain role entry: {contents}"
    );
}

#[test]
fn role_set_unknown_provider_fails() {
    let env = TestEnv::new();
    env.ur()
        .args(["role", "set", "x", "fake/nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn role_set_malformed_ref_fails() {
    let env = TestEnv::new();
    env.ur()
        .args(["role", "set", "x", "notaref"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid model reference"));
}

#[test]
fn role_set_unknown_model_fails() {
    let env = TestEnv::new();
    env.ur()
        .args(["role", "set", "x", "google/nonexistent-model"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}
