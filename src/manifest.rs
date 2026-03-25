//! Workspace manifest: persistence, merge, and state transitions.

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
                source: ext.source.to_string(),
                dir_path: ext.dir_path.to_string_lossy().into_owned(),
                enabled,
                capabilities: ext.capabilities,
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
pub fn enable(manifest: &mut WorkspaceManifest, id: &str) -> Result<()> {
    let idx = find_entry_index(manifest, id)?;

    if manifest.extensions[idx].enabled {
        bail!("{id} is already enabled");
    }

    manifest.extensions[idx].enabled = true;
    Ok(())
}

/// Disables an extension by ID.
pub fn disable(manifest: &mut WorkspaceManifest, id: &str) -> Result<()> {
    let idx = find_entry_index(manifest, id)?;

    if !manifest.extensions[idx].enabled {
        bail!("{id} is already disabled");
    }

    manifest.extensions[idx].enabled = false;
    Ok(())
}

/// Finds an extension entry by id.
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
}
