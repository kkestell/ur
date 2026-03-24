// Rust guideline compliant 2026-02-21

//! Core library crate for the `ur` workspace assistant.
//!
//! Exposes all modules so that additional binaries (e.g. `ur-tui`) can
//! import them without duplicating the compilation unit.

pub mod app;
pub mod cli;
pub mod config;
pub mod discovery;
pub mod extension_host;
pub mod extension_settings;
pub mod keyring;
pub mod manifest;
pub mod model;
pub mod provider;
pub mod session;
pub mod slot;
pub mod workspace;
