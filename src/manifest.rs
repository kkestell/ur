//! Workspace manifest: persistence, merge, and state transitions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::discovery::{self, DiscoveredExtension, SourceTier};

/// Persisted state for all extensions in a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub workspace: String,
    pub extensions: Vec<ManifestEntry>,
    /// Per-hook-point extension ordering.
    ///
    /// Keys are hook names (e.g. `"before_completion"`), values are ordered
    /// extension IDs. Extensions not in a list are appended at the end.
    /// Disabled extensions stay in position but are skipped at runtime.
    #[serde(default)]
    pub hook_ordering: BTreeMap<String, Vec<String>>,
}

/// A single extension's persisted state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub name: String,
    pub source: String,
    pub dir_path: String,
    pub enabled: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

// --- Persistence ---

/// Returns the directory where the manifest for a workspace is stored.
#[must_use]
pub fn manifest_dir(ur_root: &Path, workspace: &Path) -> PathBuf {
    let escaped = escape_workspace_path(workspace);
    ur_root.join("workspaces").join(escaped)
}

/// Escapes a workspace path for use as a directory name.
#[must_use]
pub fn escape_workspace_path(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = canonical.to_string_lossy().replace('/', "_");
    s.strip_prefix('_').unwrap_or(&s).to_owned()
}

/// Loads a manifest from disk, returning `None` if it doesn't exist.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn load_manifest(ur_root: &Path, workspace: &Path) -> Result<Option<WorkspaceManifest>> {
    let path = manifest_dir(ur_root, workspace).join("manifest.json");
    match std::fs::read_to_string(&path) {
        Ok(contents) => {
            let manifest: WorkspaceManifest = serde_json::from_str(&contents)
                .with_context(|| format!("parsing {}", path.display()))?;
            debug!(
                path = %path.display(),
                extensions = manifest.extensions.len(),
                "loaded existing manifest"
            );
            Ok(Some(manifest))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!(path = %path.display(), "no existing manifest");
            Ok(None)
        }
        Err(e) => Err(anyhow::Error::from(e).context(format!("reading {}", path.display()))),
    }
}

/// Writes a manifest to disk, creating parent directories as needed.
///
/// # Errors
///
/// Returns an error if the operation fails.
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
#[must_use]
pub fn merge(
    existing: Option<WorkspaceManifest>,
    discovered: Vec<DiscoveredExtension>,
    workspace: &Path,
) -> WorkspaceManifest {
    let (old_entries, old_hook_ordering) = existing
        .map(|m| (m.extensions, m.hook_ordering))
        .unwrap_or_default();

    let extensions: Vec<ManifestEntry> = discovered
        .into_iter()
        .map(|ext| {
            let enabled = old_entries
                .iter()
                .find(|e| e.id == ext.id)
                .map_or(ext.source == SourceTier::System, |e| e.enabled);

            ManifestEntry {
                id: ext.id,
                name: ext.name,
                source: ext.source.to_string(),
                dir_path: ext.dir_path.to_string_lossy().into_owned(),
                enabled,
                capabilities: ext.capabilities,
            }
        })
        .collect();

    // Preserve existing hook ordering, pruning removed extensions.
    let discovered_ids: Vec<&str> = extensions.iter().map(|e| e.id.as_str()).collect();
    let hook_ordering: BTreeMap<String, Vec<String>> = old_hook_ordering
        .into_iter()
        .map(|(hook, ids)| {
            let pruned: Vec<String> = ids
                .into_iter()
                .filter(|id| discovered_ids.contains(&id.as_str()))
                .collect();
            (hook, pruned)
        })
        .collect();

    WorkspaceManifest {
        workspace: workspace.to_string_lossy().into_owned(),
        extensions,
        hook_ordering,
    }
}

/// Ensures an extension is present in the ordering for a hook point.
///
/// If the extension is not already in the ordering, it is appended to
/// the end. Call this after discovering which hooks an extension
/// registered.
pub fn ensure_hook_ordering(manifest: &mut WorkspaceManifest, hook_name: &str, ext_id: &str) {
    let ordering = manifest
        .hook_ordering
        .entry(hook_name.to_owned())
        .or_default();
    if !ordering.iter().any(|id| id == ext_id) {
        ordering.push(ext_id.to_owned());
    }
}

/// Returns the ordered list of extension IDs for a hook point.
///
/// Extensions not in the persisted ordering are appended at the end.
#[must_use]
pub fn hook_order<'a>(manifest: &'a WorkspaceManifest, hook_name: &str) -> Vec<&'a str> {
    let all_ids: Vec<&str> = manifest.extensions.iter().map(|e| e.id.as_str()).collect();

    if let Some(ordering) = manifest.hook_ordering.get(hook_name) {
        let mut result: Vec<&str> = ordering
            .iter()
            .filter(|id| all_ids.contains(&id.as_str()))
            .map(String::as_str)
            .collect();
        // Append any extensions not yet in the ordering.
        for id in &all_ids {
            if !result.contains(id) {
                result.push(id);
            }
        }
        result
    } else {
        all_ids
    }
}

// --- Discovery + manifest orchestration ---

/// Discovers extensions, loads or creates the manifest, merges, saves,
/// and returns the updated manifest.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn scan_and_load(ur_root: &Path, workspace: &Path) -> Result<WorkspaceManifest> {
    info!(workspace = %workspace.display(), "scanning for extensions");
    let discovered = discovery::discover(ur_root, workspace)?;
    let existing = load_manifest(ur_root, workspace)?;
    let merged = merge(existing, discovered, workspace);
    save_manifest(ur_root, workspace, &merged)?;
    let enabled = merged.extensions.iter().filter(|e| e.enabled).count();
    info!(total = merged.extensions.len(), enabled, "manifest ready");
    Ok(merged)
}

// --- State transitions ---

/// Enables an extension by ID.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn enable(manifest: &mut WorkspaceManifest, id: &str) -> Result<()> {
    let idx = find_entry_index(manifest, id)?;

    if manifest.extensions[idx].enabled {
        bail!("{id} is already enabled");
    }

    manifest.extensions[idx].enabled = true;
    Ok(())
}

/// Disables an extension by ID.
///
/// # Errors
///
/// Returns an error if the operation fails.
pub fn disable(manifest: &mut WorkspaceManifest, id: &str) -> Result<()> {
    let idx = find_entry_index(manifest, id)?;

    if !manifest.extensions[idx].enabled {
        bail!("{id} is already disabled");
    }

    manifest.extensions[idx].enabled = false;
    Ok(())
}

/// Finds an extension entry by id.
///
/// # Errors
///
/// Returns an error if the operation fails.
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

    fn entry(id: &str, source: &str, enabled: bool) -> ManifestEntry {
        ManifestEntry {
            id: id.to_owned(),
            name: id.to_owned(),
            source: source.to_owned(),
            dir_path: String::new(),
            enabled,
            capabilities: Vec::new(),
        }
    }

    fn discovered(id: &str, source: SourceTier) -> DiscoveredExtension {
        DiscoveredExtension {
            id: id.to_owned(),
            name: id.to_owned(),
            source,
            dir_path: PathBuf::new(),
            capabilities: Vec::new(),
        }
    }

    fn manifest(entries: Vec<ManifestEntry>) -> WorkspaceManifest {
        WorkspaceManifest {
            workspace: "/test".to_owned(),
            extensions: entries,
            hook_ordering: BTreeMap::new(),
        }
    }

    #[test]
    fn merge_fresh_defaults_system_enabled_user_disabled() {
        let result = merge(
            None,
            vec![
                discovered("sys", SourceTier::System),
                discovered("usr", SourceTier::User),
                discovered("ws", SourceTier::Workspace),
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
        let existing = manifest(vec![entry("a", "system", false)]);
        let result = merge(
            Some(existing),
            vec![discovered("a", SourceTier::System)],
            Path::new("/test"),
        );
        assert!(!result.extensions[0].enabled);
    }

    #[test]
    fn merge_drops_extensions_no_longer_discovered() {
        let existing = manifest(vec![
            entry("gone", "system", true),
            entry("kept", "system", true),
        ]);
        let result = merge(
            Some(existing),
            vec![discovered("kept", SourceTier::System)],
            Path::new("/test"),
        );
        assert_eq!(result.extensions.len(), 1);
        assert_eq!(result.extensions[0].id, "kept");
    }

    #[test]
    fn enable_disabled_extension_succeeds() {
        let mut m = manifest(vec![entry("a", "system", false)]);
        enable(&mut m, "a").unwrap();
        assert!(m.extensions[0].enabled);
    }

    #[test]
    fn enable_already_enabled_returns_error() {
        let mut m = manifest(vec![entry("a", "system", true)]);
        assert!(enable(&mut m, "a").is_err());
    }

    #[test]
    fn disable_enabled_extension_succeeds() {
        let mut m = manifest(vec![entry("a", "system", true)]);
        disable(&mut m, "a").unwrap();
        assert!(!m.extensions[0].enabled);
    }

    #[test]
    fn disable_already_disabled_returns_error() {
        let mut m = manifest(vec![entry("a", "system", false)]);
        assert!(disable(&mut m, "a").is_err());
    }

    #[test]
    fn find_entry_returns_correct_entry() {
        let m = manifest(vec![entry("a", "system", true), entry("b", "user", false)]);
        let e = find_entry(&m, "b").unwrap();
        assert_eq!(e.id, "b");
        assert_eq!(e.source, "user");
    }

    #[test]
    fn escape_workspace_path_replaces_slashes() {
        let escaped = escape_workspace_path(Path::new("/foo/bar/baz"));
        assert_eq!(escaped, "foo_bar_baz");
    }

    // --- hook ordering tests ---

    #[test]
    fn ensure_hook_ordering_appends_new_extension() {
        let mut m = manifest(vec![entry("a", "system", true), entry("b", "user", true)]);
        ensure_hook_ordering(&mut m, "before_completion", "a");
        ensure_hook_ordering(&mut m, "before_completion", "b");

        let ordering = m.hook_ordering.get("before_completion").unwrap();
        assert_eq!(ordering, &["a", "b"]);
    }

    #[test]
    fn ensure_hook_ordering_does_not_duplicate() {
        let mut m = manifest(vec![entry("a", "system", true)]);
        ensure_hook_ordering(&mut m, "before_tool", "a");
        ensure_hook_ordering(&mut m, "before_tool", "a");

        let ordering = m.hook_ordering.get("before_tool").unwrap();
        assert_eq!(ordering, &["a"]);
    }

    #[test]
    fn hook_order_uses_persisted_ordering() {
        let mut m = manifest(vec![
            entry("a", "system", true),
            entry("b", "user", true),
            entry("c", "workspace", true),
        ]);
        // Persisted order: c, a (b is not in the ordering yet).
        m.hook_ordering.insert(
            "before_tool".to_owned(),
            vec!["c".to_owned(), "a".to_owned()],
        );

        let order = hook_order(&m, "before_tool");
        // c first, then a, then b (appended since not in ordering).
        assert_eq!(order, vec!["c", "a", "b"]);
    }

    #[test]
    fn hook_order_defaults_to_extension_order() {
        let m = manifest(vec![entry("x", "system", true), entry("y", "user", true)]);

        let order = hook_order(&m, "before_completion");
        assert_eq!(order, vec!["x", "y"]);
    }

    #[test]
    fn merge_preserves_hook_ordering() {
        let mut existing = manifest(vec![entry("a", "system", true), entry("b", "user", true)]);
        existing.hook_ordering.insert(
            "before_tool".to_owned(),
            vec!["b".to_owned(), "a".to_owned()],
        );

        let result = merge(
            Some(existing),
            vec![
                discovered("a", SourceTier::System),
                discovered("b", SourceTier::User),
            ],
            Path::new("/test"),
        );

        let ordering = result.hook_ordering.get("before_tool").unwrap();
        assert_eq!(ordering, &["b", "a"]);
    }

    #[test]
    fn merge_prunes_removed_extensions_from_hook_ordering() {
        let mut existing = manifest(vec![entry("a", "system", true), entry("b", "user", true)]);
        existing.hook_ordering.insert(
            "before_tool".to_owned(),
            vec!["b".to_owned(), "a".to_owned()],
        );

        // Only "a" is rediscovered; "b" is gone.
        let result = merge(
            Some(existing),
            vec![discovered("a", SourceTier::System)],
            Path::new("/test"),
        );

        let ordering = result.hook_ordering.get("before_tool").unwrap();
        assert_eq!(ordering, &["a"]);
    }
}
