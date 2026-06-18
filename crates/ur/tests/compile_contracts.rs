use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("facade crate lives under crates/ur")
        .to_owned()
}

fn check_fixture(name: &str, ur_dependency: &str, source: &str) -> Output {
    let root = workspace_root();
    let dir = root.join("target").join("compile-contracts").join(name);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{name}"
version = "0.0.0"
edition = "2024"

[dependencies]
ur = {{ {ur_dependency} }}
serde_json = "1"

[workspace]
"#
        ),
    )
    .unwrap();
    fs::write(dir.join("src").join("main.rs"), source).unwrap();

    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(dir.join("Cargo.toml"))
        .env(
            "CARGO_TARGET_DIR",
            root.join("target").join("compile-contracts-target"),
        )
        .output()
        .unwrap()
}

fn ur_dependency(features: &[&str]) -> String {
    let root = workspace_root();
    let features = features
        .iter()
        .map(|feature| format!(r#""{feature}""#))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"path = "{}", default-features = false, features = [{features}]"#,
        root.join("crates").join("ur").display()
    )
}

#[test]
fn serde_feature_exposes_serializable_public_records_and_object_safe_traits() {
    let output = check_fixture(
        "serde_public_records",
        &ur_dependency(&["serde"]),
        r##"
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

struct EmptyStream;

impl ur::Stream for EmptyStream {
    type Item = ur::Result<ur::RawEvent>;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

struct FakeProvider;

impl ur::Provider for FakeProvider {
    fn chat(&self, _request: &ur::Request) -> ur::BoxStream<'static, ur::Result<ur::RawEvent>> {
        Box::pin(EmptyStream)
    }

    fn model_spec(&self, _model_id: &str) -> Option<ur::ModelSpec> {
        Some(ur::ModelSpec::new(128, 16))
    }
}

struct EchoTool;

impl ur::Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn schema(&self) -> ur::ToolSchema {
        ur::ToolSchema::new("echo", serde_json::json!({ "type": "object" }))
    }

    fn call(&self, args: ur::ToolArguments) -> ur::BoxFuture<'static, Result<String, String>> {
        Box::pin(async move { Ok(args.to_string()) })
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _provider: Arc<dyn ur::Provider> = Arc::new(FakeProvider);
    let _tool: Arc<dyn ur::Tool> = Arc::new(EchoTool);

    serde_json::to_string(&ur::UserMessage::from("hello"))?;
    serde_json::to_string(&ur::ToolArguments::from(r#"{"text":"hello"}"#))?;
    serde_json::to_string(&ur::ToolOutput::Ok(r#""hello""#.to_owned()))?;

    let request: ur::Request = serde_json::from_value(serde_json::json!({
        "model": "test-model",
        "messages": [
            { "role": "System", "content": "system", "reasoning_content": null, "tool_calls": [], "tool_call_id": null },
            { "role": "User", "content": "hello", "reasoning_content": null, "tool_calls": [], "tool_call_id": null }
        ],
        "tools": [
            { "name": "echo", "description": null, "parameters": { "type": "object" }, "strict": false }
        ],
        "settings": {
            "thinking": "Default",
            "reasoning_effort": null,
            "max_tokens": null,
            "stop": [],
            "response_format": "Text",
            "temperature": null,
            "top_p": null
        }
    }))?;
    serde_json::to_string(&request)?;

    Ok(())
}
"##,
    );

    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn serde_impls_are_absent_without_facade_serde_feature() {
    let output = check_fixture(
        "no_serde_public_records",
        &ur_dependency(&[]),
        r##"
fn main() {
    let _ = serde_json::to_string(&ur::UserMessage::from("hello"));
}
"##,
    );

    assert!(
        !output.status.success(),
        "fixture unexpectedly compiled\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Serialize"),
        "fixture failed for an unexpected reason\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn user_message_has_no_default() {
    let output = check_fixture(
        "user_message_no_default",
        &ur_dependency(&["serde"]),
        r##"
fn main() {
    let _ = ur::UserMessage::default();
}
"##,
    );

    assert!(
        !output.status.success(),
        "fixture unexpectedly compiled\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("default"),
        "fixture failed for an unexpected reason\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
