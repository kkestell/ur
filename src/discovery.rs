//! Three-tier extension discovery via WASM component inspection.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use wasmtime::Engine;

use crate::extension_host::ExtensionInstance;

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

/// Scans all three tiers for `.wasm` files, compiles each, inspects
/// exports for slot detection, and instantiates to query identity.
///
/// # Errors
///
/// Returns an error on duplicate extension IDs, unknown slot names,
/// or WASM loading failures.
pub fn discover(
    engine: &Engine,
    ur_root: &Path,
    workspace: &Path,
) -> Result<Vec<DiscoveredExtension>> {
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

        // Each immediate subdirectory is an extension.
        let entries =
            std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;

        for entry in entries {
            let entry = entry.with_context(|| format!("scanning {}", dir.display()))?;
            let ext_dir = entry.path();
            if !ext_dir.is_dir() {
                continue;
            }

            let wasm_path = find_wasm_file(&ext_dir);
            let Some(wasm_path) = wasm_path else {
                continue;
            };

            let ext = load_discovered(engine, &wasm_path, *tier)?;

            if !seen_ids.insert(ext.id.clone()) {
                bail!("duplicate extension id: {}", ext.id);
            }

            extensions.push(ext);
        }
    }

    Ok(extensions)
}

/// Finds the first `.wasm` file in a directory, checking common locations.
fn find_wasm_file(ext_dir: &Path) -> Option<PathBuf> {
    // Check root of the extension directory first.
    if let Some(path) = find_wasm_in_dir(ext_dir) {
        return Some(path);
    }

    // Check target/wasm32-wasip2/release/ for source extensions.
    let target_dir = ext_dir.join("target/wasm32-wasip2/release");
    if target_dir.is_dir()
        && let Some(path) = find_wasm_in_dir(&target_dir)
    {
        return Some(path);
    }

    None
}

/// Finds the first `.wasm` file directly inside a directory (non-recursive).
fn find_wasm_in_dir(dir: &Path) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "wasm") && path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Compiles a WASM component, detects its slot, instantiates to query identity.
fn load_discovered(
    engine: &Engine,
    wasm_path: &Path,
    source: SourceTier,
) -> Result<DiscoveredExtension> {
    let checksum = compute_checksum(wasm_path)
        .with_context(|| format!("checksum for {}", wasm_path.display()))?;

    let abs_path = std::fs::canonicalize(wasm_path)
        .with_context(|| format!("canonicalizing {}", wasm_path.display()))?;

    // Load and detect slot.
    let mut instance = ExtensionInstance::load(engine, &abs_path)
        .map_err(|e| anyhow::anyhow!("loading {}: {e}", wasm_path.display()))?;

    let detected_slot = instance.slot_name().map(str::to_owned);

    // Query identity from the component.
    let id = instance
        .id()
        .map_err(|e| anyhow::anyhow!("calling id() on {}: {e}", wasm_path.display()))?;
    let name = instance
        .name()
        .map_err(|e| anyhow::anyhow!("calling name() on {}: {e}", wasm_path.display()))?;

    Ok(DiscoveredExtension {
        id,
        name,
        slot: detected_slot,
        source,
        wasm_path: abs_path,
        checksum,
    })
}
