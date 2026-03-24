//! Application entry point and workspace access.
//!
//! `UrApp` is the top-level object clients construct first. It owns
//! the Wasmtime engine (with caching) and the `ur_root` path, and
//! provides `open_workspace()` to obtain a workspace coordinator.

use std::path::{Path, PathBuf};

use anyhow::Result;
use wasmtime::Engine;

use crate::config::UserConfig;
use crate::manifest;
use crate::workspace::UrWorkspace;

/// Application-level entry point for Ur.
///
/// Owns the Wasmtime engine and `ur_root` path. Construct one per
/// process, then call `open_workspace()` to work with a specific
/// workspace directory.
///
/// # Examples
///
/// ```ignore
/// let app = UrApp::new("/home/user/.ur".into())?;
/// let ws = app.open_workspace("/home/user/project")?;
/// ```
#[derive(Debug)]
pub struct UrApp {
    engine: Engine,
    ur_root: PathBuf,
}

impl UrApp {
    /// Creates a new application instance with engine caching.
    ///
    /// # Errors
    ///
    /// Returns an error if the Wasmtime engine or cache cannot be
    /// initialized.
    pub fn new(ur_root: PathBuf) -> Result<Self> {
        let cache = wasmtime::Cache::new(wasmtime::CacheConfig::new())?;
        let mut config = wasmtime::Config::new();
        config.cache(Some(cache));
        let engine = Engine::new(&config)?;

        Ok(Self { engine, ur_root })
    }

    /// Opens a workspace at `path`, running extension discovery.
    ///
    /// Discovers extensions across all tiers, merges with any existing
    /// manifest, validates required slots, and returns a workspace
    /// coordinator ready for use.
    ///
    /// # Errors
    ///
    /// Returns an error if discovery, manifest I/O, or slot validation
    /// fails.
    pub fn open_workspace(&self, path: impl AsRef<Path>) -> Result<UrWorkspace> {
        let workspace_path = std::fs::canonicalize(path.as_ref())?;
        let m = manifest::scan_and_load(&self.engine, &self.ur_root, &workspace_path)?;
        let config = UserConfig::load(&self.ur_root)?;

        Ok(UrWorkspace::new(
            self.engine.clone(),
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

    /// Returns a reference to the Wasmtime engine.
    #[must_use]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
}
