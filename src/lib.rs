//! Core library crate for the `ur` workspace assistant.
//!
//! Exposes all modules so that additional binaries (e.g. `ur-tui`) can
//! import them without duplicating the compilation unit.

use std::env;
use std::path::PathBuf;

pub mod app;
pub mod cli;
pub mod config;
pub mod discovery;
pub mod hooks;
pub mod host_api;
pub mod keyring;
pub mod logging;
pub mod lua_host;
pub mod manifest;
pub mod model;
pub mod provider;
pub mod providers;
pub mod session;
pub mod types;
pub mod workspace;

/// Returns the user's home directory from `$HOME`.
///
/// # Panics
///
/// Panics if `HOME` is not set.
#[must_use]
pub fn home_dir() -> PathBuf {
    env::var("HOME").map(PathBuf::from).expect("HOME not set")
}

/// Resolves the ur root directory from `$UR_ROOT` or `~/.ur`.
#[must_use]
pub fn resolve_ur_root() -> PathBuf {
    env::var("UR_ROOT").map_or_else(|_| home_dir().join(".ur"), PathBuf::from)
}
