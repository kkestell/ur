//! Application entry point and workspace access.
//!
//! `UrApp` is the top-level object clients construct first. It owns
//! the `ur_root` path and provides `open_workspace()` to obtain a
//! workspace coordinator.

use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::info;

use crate::config::UserConfig;
use crate::manifest;
use crate::workspace::UrWorkspace;

/// Application-level entry point for Ur.
///
/// Owns `ur_root` path. Construct one per process, then call
/// `open_workspace()` to work with a specific workspace directory.
#[derive(Debug)]
pub struct UrApp {
    ur_root: PathBuf,
}

impl UrApp {
    /// Creates a new application instance.
    pub fn new(ur_root: PathBuf) -> Result<Self> {
        info!(ur_root = %ur_root.display(), "app initialized");
        Ok(Self { ur_root })
    }

    /// Opens a workspace at `path`, running extension discovery.
    ///
    /// Discovers extensions across all tiers, merges with any existing
    /// manifest, and returns a workspace coordinator ready for use.
    ///
    /// # Errors
    ///
    /// Returns an error if discovery or manifest I/O fails.
    pub fn open_workspace(&self, path: impl AsRef<Path>) -> Result<UrWorkspace> {
        let workspace_path = std::fs::canonicalize(path.as_ref())?;
        info!(workspace = %workspace_path.display(), "opening workspace");
        let m = manifest::scan_and_load(&self.ur_root, &workspace_path)?;
        let config = UserConfig::load(&self.ur_root)?;
        info!(
            workspace = %workspace_path.display(),
            extensions = m.extensions.len(),
            "workspace ready"
        );

        Ok(UrWorkspace::new(
            self.ur_root.clone(),
            workspace_path,
            m,
            config,
        ))
    }

    /// Returns a reference to the `ur_root` path.
    #[must_use]
    pub fn ur_root(&self) -> &Path {
        &self.ur_root
    }
}
