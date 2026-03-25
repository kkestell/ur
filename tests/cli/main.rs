#![allow(
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::needless_pass_by_value,
    clippy::new_without_default,
    reason = "test helpers don't need these pedantic lints"
)]

mod extension;
mod google;
mod openrouter;
mod role;
mod run;

use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[derive(Debug)]
pub struct TestEnv {
    pub workspace: TempDir,
    pub ur_root: TempDir,
}

impl TestEnv {
    pub fn new() -> Self {
        let workspace = TempDir::new().expect("create workspace tempdir");
        let ur_root = TempDir::new().expect("create ur_root tempdir");
        Self { workspace, ur_root }
    }

    pub fn ur(&self) -> Command {
        let mut cmd = Command::cargo_bin("ur").expect("find ur binary");
        cmd.arg("-w")
            .arg(self.workspace.path())
            .env("UR_ROOT", self.ur_root.path())
            .env_remove("HOME");
        cmd
    }
}

pub fn api_key(name: &str) -> String {
    let env_path = project_root().join(".env");
    let _ = dotenvy::from_path(&env_path);
    std::env::var(name).unwrap_or_else(|_| {
        panic!(
            "API key {name} not found in .env — add it to {}",
            env_path.display()
        )
    })
}
