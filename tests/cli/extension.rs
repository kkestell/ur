//! CLI integration tests for the extension management commands.

use crate::TestEnv;
use std::fs;

fn install_test_extension(env: &TestEnv) {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("extensions/workspace/test-extension");

    let dest = env.workspace.path().join(".ur/extensions/test-extension");
    fs::create_dir_all(&dest).expect("create extension dir");
    fs::copy(src.join("extension.toml"), dest.join("extension.toml")).expect("copy extension.toml");
    fs::copy(src.join("init.lua"), dest.join("init.lua")).expect("copy init.lua");
}

fn install_read_file_extension(env: &TestEnv) {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/system/read-file");

    let dest = env.ur_root.path().join("extensions/system/read-file");
    fs::create_dir_all(&dest).expect("create system extension dir");
    fs::copy(src.join("extension.toml"), dest.join("extension.toml")).expect("copy extension.toml");
    fs::copy(src.join("init.lua"), dest.join("init.lua")).expect("copy init.lua");
}

#[test]
fn extension_list_shows_discovered_lua_extension() {
    let env = TestEnv::new();
    install_test_extension(&env);

    env.ur()
        .args(["extension", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("test-extension"));
}

#[test]
fn extension_inspect_shows_tools_and_hooks() {
    let env = TestEnv::new();
    install_test_extension(&env);

    // Enable the extension first.
    env.ur()
        .args(["extension", "enable", "test-extension"])
        .assert()
        .success();

    let output = env
        .ur()
        .args(["extension", "inspect", "test-extension"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("test-extension"), "should show id");
    assert!(stdout.contains("echo"), "should show echo tool");
    assert!(stdout.contains("before_completion"), "should show hooks");
}

#[test]
fn extension_enable_disable_toggle() {
    let env = TestEnv::new();
    install_test_extension(&env);

    // Enable it.
    env.ur()
        .args(["extension", "enable", "test-extension"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Enabled"));

    // Disable it.
    env.ur()
        .args(["extension", "disable", "test-extension"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Disabled"));

    // Disabling again should error.
    env.ur()
        .args(["extension", "disable", "test-extension"])
        .assert()
        .failure();
}

// --- read-file system extension CLI tests ---

#[test]
fn read_file_extension_discovered_and_enabled_by_default() {
    let env = TestEnv::new();
    install_read_file_extension(&env);

    let output = env.ur().args(["extension", "list"]).assert().success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        stdout.contains("read-file"),
        "should list read-file extension"
    );
    assert!(
        stdout.contains("\u{2713}"),
        "system extension should be enabled by default"
    );
}

#[test]
fn read_file_extension_inspect_shows_tool() {
    let env = TestEnv::new();
    install_read_file_extension(&env);

    let output = env
        .ur()
        .args(["extension", "inspect", "read-file"])
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(stdout.contains("read-file"), "should show extension id");
    assert!(
        stdout.contains("read_file"),
        "should show the read_file tool"
    );
}

#[allow(
    clippy::single_component_path_imports,
    redundant_imports,
    reason = "Needed for predicates::str usage"
)]
use predicates;
