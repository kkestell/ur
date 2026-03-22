//! Three-tier extension discovery via `extension.toml` scanning.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::slot::validate_slot_name;

/// Which directory tier an extension was discovered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceTier {
    System,
    User,
    Workspace,
}

impl fmt::Display for SourceTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::User => write!(f, "user"),
            Self::Workspace => write!(f, "workspace"),
        }
    }
}

/// Sidecar TOML schema for extension metadata.
#[derive(Debug, Deserialize)]
struct ExtensionTomlFile {
    extension: ExtensionToml,
}

/// The `[extension]` table in `extension.toml`.
#[derive(Debug, Deserialize)]
struct ExtensionToml {
    id: String,
    name: String,
    slot: Option<String>,
    wasm: String,
}

/// An extension found during directory scanning.
#[derive(Debug)]
pub struct DiscoveredExtension {
    pub id: String,
    pub name: String,
    pub slot: Option<String>,
    pub source: SourceTier,
    pub wasm_path: PathBuf,
    pub checksum: String,
}

/// Computes a SHA-256 checksum of a file.
///
/// # Errors
///
/// Returns an error if the file cannot be read.
pub fn compute_checksum(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let hash = Sha256::digest(&bytes);
    Ok(format!("sha256:{hash:x}"))
}

/// Scans all three tiers for `extension.toml` files and parses each.
///
/// No WASM loading occurs at discovery time.
///
/// # Errors
///
/// Returns an error on duplicate extension IDs, unknown slot names,
/// or TOML parse failures.
pub fn discover(ur_root: &Path, workspace: &Path) -> Result<Vec<DiscoveredExtension>> {
    let mut extensions = Vec::new();
    let mut seen_ids = HashSet::new();

    let tiers = [
        (ur_root.join("extensions/system"), SourceTier::System),
        (ur_root.join("extensions/user"), SourceTier::User),
        (workspace.join(".ur/extensions"), SourceTier::Workspace),
    ];

    for (dir, tier) in &tiers {
        if !dir.is_dir() {
            continue;
        }

        for entry in WalkDir::new(dir) {
            let entry = entry.with_context(|| format!("scanning {}", dir.display()))?;
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == "extension.toml") {
                let ext = load_discovered(path, *tier)?;

                if !seen_ids.insert(ext.id.clone()) {
                    bail!("duplicate extension id: {}", ext.id);
                }

                extensions.push(ext);
            }
        }
    }

    Ok(extensions)
}

/// Parses a single `extension.toml` and resolves the WASM path.
fn load_discovered(toml_path: &Path, source: SourceTier) -> Result<DiscoveredExtension> {
    let contents = std::fs::read_to_string(toml_path)
        .with_context(|| format!("reading {}", toml_path.display()))?;

    let parsed: ExtensionTomlFile =
        toml::from_str(&contents).with_context(|| format!("parsing {}", toml_path.display()))?;

    let meta = parsed.extension;

    if let Some(ref slot) = meta.slot {
        validate_slot_name(slot)?;
    }

    let toml_dir = toml_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent for {}", toml_path.display()))?;
    let wasm_path = toml_dir.join(&meta.wasm);

    let checksum = compute_checksum(&wasm_path)
        .with_context(|| format!("checksum for {}", wasm_path.display()))?;

    let abs_path = std::fs::canonicalize(&wasm_path)
        .with_context(|| format!("canonicalizing {}", wasm_path.display()))?;

    Ok(DiscoveredExtension {
        id: meta.id,
        name: meta.name,
        slot: meta.slot,
        source,
        wasm_path: abs_path,
        checksum,
    })
}
