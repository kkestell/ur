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

const SYSTEM_EXTENSIONS: &[&str] = &[
    "session-jsonl",
    "compaction-llm",
    "llm-google",
    "llm-openrouter",
];

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn wasm_path(tier: &str, name: &str) -> PathBuf {
    let filename = format!("{}.wasm", name.replace('-', "_"));
    let path = project_root()
        .join("extensions")
        .join(tier)
        .join(name)
        .join("target/wasm32-wasip2/release")
        .join(&filename);
    assert!(
        path.exists(),
        "WASM extension not found at {} — run `make build-extensions` first",
        path.display()
    );
    path
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

        for ext_name in SYSTEM_EXTENSIONS {
            let dest = ur_root.path().join("extensions/system").join(ext_name);
            fs::create_dir_all(&dest).expect("create system extension dir");
            let src = wasm_path("system", ext_name);
            let filename = src.file_name().unwrap();
            fs::copy(&src, dest.join(filename)).expect("copy system extension wasm");
        }

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

    pub fn install_workspace_ext(&self, name: &str) {
        let dest = self.workspace.path().join(".ur/extensions").join(name);
        fs::create_dir_all(&dest).expect("create workspace extension dir");
        let src = wasm_path("workspace", name);
        let filename = src.file_name().unwrap();
        fs::copy(&src, dest.join(filename)).expect("copy workspace extension wasm");
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
