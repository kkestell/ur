//! Workspace manifest: persistence, merge, and state transitions.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use wasmtime::Engine;

use crate::discovery::{self, DiscoveredExtension, SourceTier};
use crate::slot::{Cardinality, find_slot, validate_required_slots};

/// Persisted state for all extensions in a workspace.
#[derive(Debug, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub workspace: String,
    pub extensions: Vec<ManifestEntry>,
}

/// A single extension's persisted state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub name: String,
    pub slot: Option<String>,
    pub source: String,
    pub wasm_path: String,
    pub checksum: String,
    pub enabled: bool,
}

// --- Persistence ---

/// Returns the directory where the manifest for a workspace is stored.
pub fn manifest_dir(ur_root: &Path, workspace: &Path) -> PathBuf {
    let escaped = escape_workspace_path(workspace);
    ur_root.join("workspaces").join(escaped)
}

/// Escapes a workspace path for use as a directory name.
///
/// Canonicalizes the path, replaces `/` with `_`, and strips the
/// leading `_`.
pub fn escape_workspace_path(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy().replace('/', "_");
    s.strip_prefix('_').unwrap_or(&s).to_owned()
}

/// Loads a manifest from disk, returning `None` if it doesn't exist.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be parsed.
pub fn load_manifest(ur_root: &Path, workspace: &Path) -> Result<Option<WorkspaceManifest>> {
    let path = manifest_dir(ur_root, workspace).join("manifest.json");
    if !path.exists() {
        return Ok(None);
    }
    let contents =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let manifest =
        serde_json::from_str(&contents).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(manifest))
}

/// Writes a manifest to disk, creating parent directories as needed.
///
/// # Errors
///
/// Returns an error if the file cannot be written.
pub fn save_manifest(ur_root: &Path, workspace: &Path, manifest: &WorkspaceManifest) -> Result<()> {
    let dir = manifest_dir(ur_root, workspace);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("manifest.json");
    let json = serde_json::to_string_pretty(manifest)?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

// --- Merge ---

/// Merges discovered extensions with an existing manifest.
///
/// New system extensions default to enabled; user and workspace
/// extensions default to disabled. Existing entries keep their
/// enabled state. Extensions no longer discovered are removed.
pub fn merge(
    existing: Option<WorkspaceManifest>,
    discovered: Vec<DiscoveredExtension>,
    workspace: &Path,
) -> WorkspaceManifest {
    let old_entries: Vec<ManifestEntry> = existing.map(|m| m.extensions).unwrap_or_default();

    let extensions = discovered
        .into_iter()
        .map(|ext| {
            let enabled = old_entries
                .iter()
                .find(|e| e.id == ext.id)
                .map_or(ext.source == SourceTier::System, |e| e.enabled);

            ManifestEntry {
                id: ext.id,
                name: ext.name,
                slot: ext.slot,
                source: ext.source.to_string(),
                wasm_path: ext.wasm_path.to_string_lossy().into_owned(),
                checksum: ext.checksum,
                enabled,
            }
        })
        .collect();

    WorkspaceManifest {
        workspace: workspace.to_string_lossy().into_owned(),
        extensions,
    }
}

// --- Discovery + manifest orchestration ---

/// Discovers extensions, loads or creates the manifest, merges, saves,
/// and returns the updated manifest.
///
/// # Errors
///
/// Returns an error if discovery or manifest I/O fails.
pub fn scan_and_load(
    engine: &Engine,
    ur_root: &Path,
    workspace: &Path,
) -> Result<WorkspaceManifest> {
    let discovered = discovery::discover(engine, ur_root, workspace)?;
    let existing = load_manifest(ur_root, workspace)?;
    let merged = merge(existing, discovered, workspace);
    validate_required_slots(merged.extensions.iter().map(|e| (&e.slot, e.enabled)))?;
    save_manifest(ur_root, workspace, &merged)?;
    Ok(merged)
}

// --- State transitions ---

/// Enables an extension, enforcing slot cardinality.
///
/// For exactly-1 slots, the current occupant is disabled automatically
/// (switch semantics).
///
/// # Errors
///
/// Returns an error if the extension is not found or already enabled.
pub fn enable(manifest: &mut WorkspaceManifest, id: &str) -> Result<()> {
    let idx = find_entry_index(manifest, id)?;

    if manifest.extensions[idx].enabled {
        bail!("{id} is already enabled");
    }

    // For exactly-1 slots, disable the current occupant (switch semantics).
    if let Some(ref slot_name) = manifest.extensions[idx].slot.clone()
        && let Some(slot_def) = find_slot(slot_name)
        && slot_def.cardinality == Cardinality::ExactlyOne
    {
        for entry in &mut manifest.extensions {
            if entry.slot.as_deref() == Some(slot_name) && entry.enabled {
                entry.enabled = false;
            }
        }
    }

    manifest.extensions[idx].enabled = true;
    Ok(())
}

/// Disables an extension, preventing removal of required slot providers.
///
/// # Errors
///
/// Returns an error if the extension is not found, already disabled,
/// or is the last provider in a required slot.
pub fn disable(manifest: &mut WorkspaceManifest, id: &str) -> Result<()> {
    let idx = find_entry_index(manifest, id)?;

    if !manifest.extensions[idx].enabled {
        bail!("{id} is already disabled");
    }

    if let Some(ref slot_name) = manifest.extensions[idx].slot
        && let Some(slot_def) = find_slot(slot_name)
        && slot_def.required
    {
        let enabled_count = manifest
            .extensions
            .iter()
            .filter(|e| e.slot.as_deref() == Some(slot_name) && e.enabled)
            .count();

        if enabled_count <= 1 {
            bail!("cannot disable {id}: it is the only {slot_name} provider");
        }
    }

    manifest.extensions[idx].enabled = false;
    Ok(())
}

/// Finds an extension entry by id.
///
/// # Errors
///
/// Returns an error if the extension is not found.
pub fn find_entry<'a>(manifest: &'a WorkspaceManifest, id: &str) -> Result<&'a ManifestEntry> {
    manifest
        .extensions
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow::anyhow!("extension not found: {id}"))
}

/// Finds an extension's index in the manifest by id.
fn find_entry_index(manifest: &WorkspaceManifest, id: &str) -> Result<usize> {
    manifest
        .extensions
        .iter()
        .position(|e| e.id == id)
        .ok_or_else(|| anyhow::anyhow!("extension not found: {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, slot: Option<&str>, source: &str, enabled: bool) -> ManifestEntry {
        ManifestEntry {
            id: id.to_owned(),
            name: id.to_owned(),
            slot: slot.map(str::to_owned),
            source: source.to_owned(),
            wasm_path: String::new(),
            checksum: String::new(),
            enabled,
        }
    }

    fn discovered(id: &str, slot: Option<&str>, source: SourceTier) -> DiscoveredExtension {
        DiscoveredExtension {
            id: id.to_owned(),
            name: id.to_owned(),
            slot: slot.map(str::to_owned),
            source,
            wasm_path: PathBuf::new(),
            checksum: String::new(),
        }
    }

    fn manifest(entries: Vec<ManifestEntry>) -> WorkspaceManifest {
        WorkspaceManifest {
            workspace: "/test".to_owned(),
            extensions: entries,
        }
    }

    // --- merge tests ---

    #[test]
    fn merge_fresh_defaults_system_enabled_user_disabled() {
        let result = merge(
            None,
            vec![
                discovered("sys", Some("llm-provider"), SourceTier::System),
                discovered("usr", Some("llm-provider"), SourceTier::User),
                discovered("ws", Some("llm-provider"), SourceTier::Workspace),
            ],
            Path::new("/test"),
        );
        assert!(
            result
                .extensions
                .iter()
                .find(|e| e.id == "sys")
                .unwrap()
                .enabled
        );
        assert!(
            !result
                .extensions
                .iter()
                .find(|e| e.id == "usr")
                .unwrap()
                .enabled
        );
        assert!(
            !result
                .extensions
                .iter()
                .find(|e| e.id == "ws")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn merge_preserves_existing_enabled_state() {
        let existing = manifest(vec![
            entry("a", Some("llm-provider"), "system", false), // was disabled
        ]);
        let result = merge(
            Some(existing),
            vec![discovered("a", Some("llm-provider"), SourceTier::System)],
            Path::new("/test"),
        );
        assert!(!result.extensions[0].enabled);
    }

    #[test]
    fn merge_drops_extensions_no_longer_discovered() {
        let existing = manifest(vec![
            entry("gone", Some("llm-provider"), "system", true),
            entry("kept", Some("llm-provider"), "system", true),
        ]);
        let result = merge(
            Some(existing),
            vec![discovered("kept", Some("llm-provider"), SourceTier::System)],
            Path::new("/test"),
        );
        assert_eq!(result.extensions.len(), 1);
        assert_eq!(result.extensions[0].id, "kept");
    }

    #[test]
    fn merge_adds_new_extensions_alongside_existing() {
        let existing = manifest(vec![entry("old", Some("llm-provider"), "system", true)]);
        let result = merge(
            Some(existing),
            vec![
                discovered("old", Some("llm-provider"), SourceTier::System),
                discovered("new", Some("llm-provider"), SourceTier::User),
            ],
            Path::new("/test"),
        );
        assert_eq!(result.extensions.len(), 2);
        assert!(
            result
                .extensions
                .iter()
                .find(|e| e.id == "old")
                .unwrap()
                .enabled
        );
        assert!(
            !result
                .extensions
                .iter()
                .find(|e| e.id == "new")
                .unwrap()
                .enabled
        );
    }

    // --- enable tests ---

    #[test]
    fn enable_disabled_extension_succeeds() {
        let mut m = manifest(vec![entry("a", Some("llm-provider"), "system", false)]);
        enable(&mut m, "a").unwrap();
        assert!(m.extensions[0].enabled);
    }

    #[test]
    fn enable_already_enabled_returns_error() {
        let mut m = manifest(vec![entry("a", Some("llm-provider"), "system", true)]);
        assert!(enable(&mut m, "a").is_err());
    }

    #[test]
    fn enable_exactly_one_slot_disables_current_occupant() {
        let mut m = manifest(vec![
            entry("a", Some("session-provider"), "system", true),
            entry("b", Some("session-provider"), "user", false),
        ]);
        enable(&mut m, "b").unwrap();
        assert!(!m.extensions[0].enabled); // a was switched off
        assert!(m.extensions[1].enabled); // b is now on
    }

    #[test]
    fn enable_at_least_one_slot_does_not_disable_others() {
        let mut m = manifest(vec![
            entry("a", Some("llm-provider"), "system", true),
            entry("b", Some("llm-provider"), "user", false),
        ]);
        enable(&mut m, "b").unwrap();
        assert!(m.extensions[0].enabled); // a still on
        assert!(m.extensions[1].enabled);
    }

    #[test]
    fn enable_unknown_extension_returns_error() {
        let mut m = manifest(vec![]);
        assert!(enable(&mut m, "nope").is_err());
    }

    // --- disable tests ---

    #[test]
    fn disable_enabled_extension_succeeds() {
        let mut m = manifest(vec![
            entry("a", Some("llm-provider"), "system", true),
            entry("b", Some("llm-provider"), "system", true),
            entry("c", Some("session-provider"), "system", true),
            entry("d", Some("compaction-provider"), "system", true),
        ]);
        disable(&mut m, "a").unwrap();
        assert!(!m.extensions[0].enabled);
    }

    #[test]
    fn disable_already_disabled_returns_error() {
        let mut m = manifest(vec![entry("a", Some("llm-provider"), "system", false)]);
        assert!(disable(&mut m, "a").is_err());
    }

    #[test]
    fn disable_last_provider_of_required_slot_returns_error() {
        let mut m = manifest(vec![
            entry("a", Some("session-provider"), "system", true),
            entry("b", Some("compaction-provider"), "system", true),
            entry("c", Some("llm-provider"), "system", true),
        ]);
        assert!(disable(&mut m, "a").is_err());
    }

    #[test]
    fn disable_one_of_multiple_at_least_one_succeeds() {
        let mut m = manifest(vec![
            entry("a", Some("llm-provider"), "system", true),
            entry("b", Some("llm-provider"), "system", true),
            entry("c", Some("session-provider"), "system", true),
            entry("d", Some("compaction-provider"), "system", true),
        ]);
        disable(&mut m, "a").unwrap();
    }

    #[test]
    fn disable_unknown_extension_returns_error() {
        let mut m = manifest(vec![]);
        assert!(disable(&mut m, "nope").is_err());
    }

    // --- find_entry / find_entry_index tests ---

    #[test]
    fn find_entry_returns_correct_entry() {
        let m = manifest(vec![
            entry("a", None, "system", true),
            entry("b", None, "user", false),
        ]);
        let e = find_entry(&m, "b").unwrap();
        assert_eq!(e.id, "b");
        assert_eq!(e.source, "user");
    }

    #[test]
    fn find_entry_returns_error_for_unknown() {
        let m = manifest(vec![]);
        find_entry(&m, "nope").unwrap_err();
    }

    #[test]
    fn find_entry_index_returns_correct_index() {
        let m = manifest(vec![
            entry("a", None, "system", true),
            entry("b", None, "user", false),
        ]);
        assert_eq!(find_entry_index(&m, "b").unwrap(), 1);
    }

    #[test]
    fn find_entry_index_returns_error_for_unknown() {
        let m = manifest(vec![]);
        find_entry_index(&m, "nope").unwrap_err();
    }

    // --- escape_workspace_path test ---

    #[test]
    fn escape_workspace_path_replaces_slashes() {
        // Use a path that won't be canonicalized (non-existent)
        let escaped = escape_workspace_path(Path::new("/foo/bar/baz"));
        // canonicalize fails for non-existent, so falls back to raw path
        assert_eq!(escaped, "foo_bar_baz");
    }
}
