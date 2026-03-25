//! Three-tier extension discovery via `extension.toml` inspection.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tracing::{debug, info, warn};

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
    pub source: SourceTier,
    pub dir_path: PathBuf,
    pub capabilities: Vec<String>,
}

/// Scans all three tiers for directories containing `extension.toml`.
///
/// # Errors
///
/// Returns an error on duplicate extension IDs or manifest parse failures.
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
            debug!(tier = %tier, dir = %dir.display(), "skipping missing tier directory");
            continue;
        }

        let entries =
            std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;

        for entry in entries {
            let entry = entry.with_context(|| format!("scanning {}", dir.display()))?;
            let ext_dir = entry.path();
            if !ext_dir.is_dir() {
                continue;
            }

            let manifest_path = ext_dir.join("extension.toml");
            if !manifest_path.is_file() {
                warn!(dir = %ext_dir.display(), "no extension.toml found");
                continue;
            }

            let ext = parse_extension_dir(&ext_dir, &manifest_path, *tier)?;
            debug!(
                id = %ext.id,
                name = %ext.name,
                tier = %tier,
                "discovered extension"
            );

            if !seen_ids.insert(ext.id.clone()) {
                bail!("duplicate extension id: {}", ext.id);
            }

            extensions.push(ext);
        }
    }

    info!(count = extensions.len(), "extension discovery complete");
    Ok(extensions)
}

/// Parses an extension directory's `extension.toml` manifest.
fn parse_extension_dir(
    ext_dir: &Path,
    manifest_path: &Path,
    source: SourceTier,
) -> Result<DiscoveredExtension> {
    let contents = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("reading {}", manifest_path.display()))?;

    let doc: toml::Table = toml::from_str(&contents)
        .with_context(|| format!("parsing {}", manifest_path.display()))?;

    let ext_table = doc
        .get("extension")
        .and_then(|v| v.as_table())
        .ok_or_else(|| {
            anyhow::anyhow!("[extension] table missing in {}", manifest_path.display())
        })?;

    let id = ext_table
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("extension.id missing in {}", manifest_path.display()))?
        .to_owned();

    let name = ext_table
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_owned();

    let capabilities = ext_table
        .get("capabilities")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    Ok(DiscoveredExtension {
        id,
        name,
        source,
        dir_path: ext_dir.to_owned(),
        capabilities,
    })
}
