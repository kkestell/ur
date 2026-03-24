//! Three-tier extension discovery via WASM component inspection.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use wasmtime::Engine;

use crate::extension_host::{self, ExtensionInstance, LoadOptions, wit_types};

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
    pub capabilities: wit_types::ExtensionCapabilities,
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

            let Some(wasm_path) = find_wasm_file(&ext_dir)? else {
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

/// Recursively locates the `.wasm` file inside an extension directory.
///
/// Returns `Ok(Some(path))` when exactly one `.wasm` file exists,
/// `Ok(None)` when none are found, and `Err` when multiple candidates
/// would make the choice ambiguous.
fn find_wasm_file(ext_dir: &Path) -> Result<Option<PathBuf>> {
    let mut candidates = Vec::new();
    collect_wasm_files(ext_dir, &mut candidates);
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(Some(candidates.remove(0))),
        _ => bail!(
            "multiple .wasm files in {}: {}",
            ext_dir.display(),
            candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Collects all `.wasm` files under `dir`, recursing into subdirectories.
fn collect_wasm_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_wasm_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "wasm") && path.is_file() {
            out.push(path);
        }
    }
}

/// Compiles a WASM component, detects its slot, instantiates to query identity
/// and declared capabilities.
fn load_discovered(
    engine: &Engine,
    wasm_path: &Path,
    source: SourceTier,
) -> Result<DiscoveredExtension> {
    let checksum = compute_checksum(wasm_path)
        .with_context(|| format!("checksum for {}", wasm_path.display()))?;

    let abs_path = std::fs::canonicalize(wasm_path)
        .with_context(|| format!("canonicalizing {}", wasm_path.display()))?;

    // Load with all capabilities linked (discovery needs to call
    // declare_capabilities before we know what to restrict).
    let mut instance = ExtensionInstance::load(engine, &abs_path, &LoadOptions::default())
        .map_err(|e| anyhow::anyhow!("loading {}: {e}", wasm_path.display()))?;

    let detected_slot = instance.slot_name().map(str::to_owned);

    // Query identity and capabilities from the component.
    let id = instance
        .id()
        .map_err(|e| anyhow::anyhow!("calling id() on {}: {e}", wasm_path.display()))?;
    let name = instance
        .name()
        .map_err(|e| anyhow::anyhow!("calling name() on {}: {e}", wasm_path.display()))?;
    let capabilities = instance.declare_capabilities().map_err(|e| {
        anyhow::anyhow!(
            "calling declare_capabilities() on {}: {e}",
            wasm_path.display()
        )
    })?;

    // Validate declared capabilities match actual component imports.
    let component = wasmtime::component::Component::from_file(engine, &abs_path)
        .map_err(|e| anyhow::anyhow!("re-loading component {}: {e}", wasm_path.display()))?;
    extension_host::validate_capabilities(engine, &component, capabilities, &id);

    Ok(DiscoveredExtension {
        id,
        name,
        slot: detected_slot,
        source,
        wasm_path: abs_path,
        checksum,
        capabilities,
    })
}
