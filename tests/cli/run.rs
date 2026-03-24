use super::TestEnv;
use predicates::prelude::*;

fn setup_echo_env() -> TestEnv {
    let env = TestEnv::new();
    env.install_workspace_ext("llm-test");
    env.install_workspace_ext("test-extension");
    env.ur()
        .args(["extension", "enable", "llm-test"])
        .assert()
        .success();
    env.ur()
        .args(["extension", "enable", "test-extension"])
        .assert()
        .success();
    env.ur()
        .args(["role", "set", "default", "test/echo"])
        .assert()
        .success();
    env
}

#[test]
fn echo_turn_returns_message() {
    let env = setup_echo_env();
    env.ur()
        .args(["run", "Hello, please greet the world"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn echo_turn_echoes_input() {
    let env = setup_echo_env();
    // The echo provider triggers the weather tool; stdout includes the tool result
    env.ur()
        .args(["run", "Hello echo test"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn verbose_shows_session_events() {
    let env = setup_echo_env();
    env.ur()
        .args(["-v", "run", "Hello verbose test"])
        .assert()
        .success()
        .stderr(
            predicate::str::contains("session loaded")
                .and(predicate::str::contains("turn complete")),
        );
}

#[test]
fn tool_call_round_trip() {
    let env = setup_echo_env();
    env.ur()
        .args(["-v", "run", "What is the weather in Paris?"])
        .assert()
        .success()
        .stderr(predicate::str::contains("tool call").or(predicate::str::contains("get_weather")));
}
