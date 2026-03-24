//! Workspace-scoped coordinator for extensions, roles, and settings.
//!
//! `UrWorkspace` is the primary object for managing a workspace. It
//! owns the merged manifest, user config, and engine reference, and
//! provides structured access to extension management, role mapping,
//! and extension configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use wasmtime::Engine;

use crate::config::{self, UserConfig};
use crate::extension_host::wit_types;
use crate::extension_settings;
use crate::keyring;
use crate::manifest::{self, ManifestEntry, WorkspaceManifest};
use crate::model::{self, ProviderModels};
use crate::session::UrSession;

/// Workspace-scoped coordinator.
///
/// Owns the manifest and user config for a single workspace directory.
/// All extension management, role resolution, and config operations go
/// through this type. The CLI formats the returned data for display.
#[derive(Debug)]
pub struct UrWorkspace {
    engine: Engine,
    ur_root: PathBuf,
    workspace_path: PathBuf,
    manifest: WorkspaceManifest,
    config: UserConfig,
}

// --- Structured return types ---

/// A resolved setting with its current value and metadata.
#[derive(Debug)]
pub struct SettingInfo {
    /// The full dotted key (e.g. "gemini-flash.thinking_level").
    pub key: String,
    /// Schema type name (e.g. "integer", "enum").
    pub type_name: &'static str,
    /// Display-ready value string.
    pub value_display: String,
}

/// The resolved value of a single extension setting.
#[derive(Debug)]
pub enum SettingGetResult {
    /// A secret that has been stored.
    SecretSet,
    /// A secret that has not been set.
    SecretUnset,
    /// A non-secret value.
    Value(String),
}

/// Result of setting an extension config value.
#[derive(Debug)]
pub enum SettingSetResult {
    /// A secret needs to be stored. The caller must provide the value.
    SecretRequired {
        /// The human-readable setting name (for prompting).
        name: String,
    },
    /// The setting was stored successfully.
    Stored {
        /// The key that was set.
        key: String,
        /// The raw value that was written.
        value: String,
    },
}

/// A role mapping entry.
#[derive(Debug)]
pub struct RoleEntry {
    /// The role name (e.g. "default", "fast").
    pub role: String,
    /// The provider/model reference (e.g. "google/gemini-3-flash").
    pub model_ref: String,
}

/// Result of resolving a role.
#[derive(Debug)]
pub struct ResolvedRole {
    /// The role name that was resolved.
    pub role: String,
    /// The provider ID.
    pub provider_id: String,
    /// The model ID.
    pub model_id: String,
}

impl UrWorkspace {
    /// Constructs a workspace from pre-loaded components.
    pub(crate) fn new(
        engine: Engine,
        ur_root: PathBuf,
        workspace_path: PathBuf,
        manifest: WorkspaceManifest,
        config: UserConfig,
    ) -> Self {
        Self {
            engine,
            ur_root,
            workspace_path,
            manifest,
            config,
        }
    }

    /// Returns a reference to the Wasmtime engine.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Returns a reference to the `ur_root` path.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn ur_root(&self) -> &Path {
        &self.ur_root
    }

    /// Returns a reference to the workspace path.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }

    /// Returns a reference to the workspace manifest.
    pub fn manifest(&self) -> &WorkspaceManifest {
        &self.manifest
    }

    /// Returns a reference to the user config.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn config(&self) -> &UserConfig {
        &self.config
    }

    // --- Extension management ---

    /// Returns the list of all discovered extensions.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn list_extensions(&self) -> &[ManifestEntry] {
        &self.manifest.extensions
    }

    /// Enables an extension by ID with slot cardinality enforcement.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not found or already
    /// enabled.
    pub fn enable_extension(&mut self, id: &str) -> Result<()> {
        manifest::enable(&mut self.manifest, id)?;
        manifest::save_manifest(&self.ur_root, &self.workspace_path, &self.manifest)?;
        Ok(())
    }

    /// Disables an extension by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not found, already
    /// disabled, or is the last provider in a required slot.
    pub fn disable_extension(&mut self, id: &str) -> Result<()> {
        manifest::disable(&mut self.manifest, id)?;
        manifest::save_manifest(&self.ur_root, &self.workspace_path, &self.manifest)?;
        Ok(())
    }

    /// Finds an extension entry by ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension is not found.
    pub fn find_extension(&self, id: &str) -> Result<&ManifestEntry> {
        manifest::find_entry(&self.manifest, id)
    }

    // --- Role management ---

    /// Returns all configured role mappings.
    ///
    /// If no explicit "default" role is configured, the resolved
    /// default is included as the first entry.
    ///
    /// # Errors
    ///
    /// Returns an error if provider models cannot be collected.
    pub fn list_roles(&self) -> Result<Vec<RoleEntry>> {
        let providers = model::collect_provider_models(&self.engine, &self.manifest)?;
        let mut entries = Vec::new();

        if !self.config.roles.contains_key("default") {
            let (p, m) = model::resolve_role(&self.config, "default", &providers)?;
            entries.push(RoleEntry {
                role: "default".into(),
                model_ref: format!("{p}/{m}"),
            });
        }
        for (role, model_ref) in &self.config.roles {
            entries.push(RoleEntry {
                role: role.clone(),
                model_ref: model_ref.clone(),
            });
        }
        Ok(entries)
    }

    /// Resolves a role to its provider and model IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if resolution fails or providers cannot be
    /// queried.
    pub fn resolve_role(&self, role: &str) -> Result<ResolvedRole> {
        let providers = model::collect_provider_models(&self.engine, &self.manifest)?;
        let (provider_id, model_id) = model::resolve_role(&self.config, role, &providers)?;
        Ok(ResolvedRole {
            role: role.into(),
            provider_id,
            model_id,
        })
    }

    /// Maps a role to a provider/model pair, persisting the change.
    ///
    /// # Errors
    ///
    /// Returns an error if the model reference is invalid or not found
    /// in any enabled provider.
    pub fn set_role(&mut self, role: &str, model_ref: &str) -> Result<ResolvedRole> {
        let providers = model::collect_provider_models(&self.engine, &self.manifest)?;
        let (provider_id, model_id) = config::parse_model_ref(model_ref).ok_or_else(|| {
            anyhow::anyhow!("invalid model reference '{model_ref}' (expected provider/model)")
        })?;

        model::find_descriptor(&providers, provider_id, model_id).ok_or_else(|| {
            anyhow::anyhow!("model '{model_ref}' not found in any enabled provider")
        })?;

        self.config
            .roles
            .insert(role.to_owned(), model_ref.to_owned());
        self.config.save(&self.ur_root)?;

        Ok(ResolvedRole {
            role: role.into(),
            provider_id: provider_id.to_owned(),
            model_id: model_id.to_owned(),
        })
    }

    // --- Extension settings ---

    /// Lists settings for an extension, optionally filtered by glob.
    ///
    /// Returns structured setting info with resolved values. Secret
    /// settings show "****" or "(not set)". Readonly settings are
    /// annotated.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension cannot be loaded.
    pub fn list_extension_settings(
        &self,
        id: &str,
        pattern: Option<&str>,
    ) -> Result<Vec<SettingInfo>> {
        let (_, descriptors) =
            extension_settings::load_extension(&self.engine, &self.manifest, id)?;
        let overrides = self.config.extensions.get(id);

        let mut settings = Vec::new();
        for desc in &descriptors {
            if let Some(pat) = pattern
                && !extension_settings::glob_match(pat, &desc.key)
            {
                continue;
            }

            let type_name = config::schema_type_name(&desc.schema);
            let value_display = if desc.secret {
                let has_secret = keyring::get_api_key(id)
                    .ok()
                    .flatten()
                    .is_some_and(|v| !v.is_empty());
                if has_secret {
                    "****".to_owned()
                } else {
                    "(not set)".to_owned()
                }
            } else if desc.readonly {
                let val = config::default_value(&desc.schema);
                format!(
                    "{} (readonly)",
                    extension_settings::format_setting_value(&val)
                )
            } else if let Some(toml_val) = overrides.and_then(|o| o.get(&desc.key)) {
                match config::convert_toml_value(toml_val, &desc.schema, &desc.key) {
                    Ok(val) => extension_settings::format_setting_value(&val),
                    Err(_) => format!("{toml_val}"),
                }
            } else {
                let val = config::default_value(&desc.schema);
                extension_settings::format_setting_value(&val)
            };

            settings.push(SettingInfo {
                key: desc.key.clone(),
                type_name,
                value_display,
            });
        }
        Ok(settings)
    }

    /// Gets the current value of a single extension setting.
    ///
    /// # Errors
    ///
    /// Returns an error if the extension or setting is not found.
    pub fn get_extension_setting(&self, id: &str, key: &str) -> Result<SettingGetResult> {
        let (_, descriptors) =
            extension_settings::load_extension(&self.engine, &self.manifest, id)?;

        let desc = descriptors
            .iter()
            .find(|d| d.key == key)
            .ok_or_else(|| anyhow::anyhow!("unknown setting '{key}' for extension '{id}'"))?;

        if desc.secret {
            let stored = keyring::get_api_key(id).ok().flatten();
            return if stored.is_some() {
                Ok(SettingGetResult::SecretSet)
            } else {
                Ok(SettingGetResult::SecretUnset)
            };
        }

        if desc.readonly {
            let val = config::default_value(&desc.schema);
            return Ok(SettingGetResult::Value(
                extension_settings::format_setting_value(&val),
            ));
        }

        let overrides = self.config.extensions.get(id);
        let value = match overrides.and_then(|o| o.get(&desc.key)) {
            Some(toml_val) => config::convert_toml_value(toml_val, &desc.schema, &desc.key)?,
            None => config::default_value(&desc.schema),
        };
        Ok(SettingGetResult::Value(
            extension_settings::format_setting_value(&value),
        ))
    }

    /// Sets an extension setting to a new value.
    ///
    /// For secret settings, if no value is provided, returns
    /// `SettingSetResult::SecretRequired` so the caller can prompt.
    /// Call `store_secret()` with the obtained value.
    ///
    /// # Errors
    ///
    /// Returns an error if the setting is unknown, readonly, or the
    /// value fails validation.
    pub fn set_extension_setting(
        &mut self,
        id: &str,
        key: &str,
        value: Option<&str>,
    ) -> Result<SettingSetResult> {
        let (mut instance, descriptors) =
            extension_settings::load_extension(&self.engine, &self.manifest, id)?;

        let desc = descriptors
            .iter()
            .find(|d| d.key == key)
            .ok_or_else(|| anyhow::anyhow!("unknown setting '{key}' for extension '{id}'"))?;

        if desc.readonly {
            anyhow::bail!("setting '{key}' is read-only");
        }

        if desc.secret {
            return match value {
                Some(v) => {
                    let v = v.trim();
                    anyhow::ensure!(!v.is_empty(), "value cannot be empty");
                    let keyring_id = extension_settings::resolve_provider_id(&mut instance)
                        .unwrap_or_else(|| id.to_owned());
                    keyring::set_api_key(&keyring_id, v)?;
                    Ok(SettingSetResult::Stored {
                        key: key.to_owned(),
                        value: "****".to_owned(),
                    })
                }
                None => Ok(SettingSetResult::SecretRequired {
                    name: desc.name.clone(),
                }),
            };
        }

        let raw = value.ok_or_else(|| anyhow::anyhow!("value required for non-secret setting"))?;
        let toml_value = config::parse_setting_value(raw, &desc.schema, key)?;

        self.config
            .extensions
            .entry(id.to_owned())
            .or_default()
            .insert(key.to_owned(), toml_value);
        self.config.save(&self.ur_root)?;

        Ok(SettingSetResult::Stored {
            key: key.to_owned(),
            value: raw.to_owned(),
        })
    }

    /// Stores a secret value directly (after caller has prompted).
    ///
    /// # Errors
    ///
    /// Returns an error if the keyring write fails or the setting is
    /// not a secret.
    pub fn store_secret(&mut self, id: &str, key: &str, secret: &str) -> Result<()> {
        let (mut instance, descriptors) =
            extension_settings::load_extension(&self.engine, &self.manifest, id)?;

        let desc = descriptors
            .iter()
            .find(|d| d.key == key)
            .ok_or_else(|| anyhow::anyhow!("unknown setting '{key}' for extension '{id}'"))?;

        anyhow::ensure!(desc.secret, "setting '{key}' is not a secret");
        anyhow::ensure!(!secret.trim().is_empty(), "value cannot be empty");

        let keyring_id =
            extension_settings::resolve_provider_id(&mut instance).unwrap_or_else(|| id.to_owned());
        keyring::set_api_key(&keyring_id, secret.trim())?;
        Ok(())
    }

    // --- Provider models (used internally and by session) ---

    /// Collects provider models from all enabled LLM extensions.
    ///
    /// # Errors
    ///
    /// Returns an error if provider queries fail.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn collect_provider_models(&self) -> Result<ProviderModels> {
        model::collect_provider_models(&self.engine, &self.manifest)
    }

    /// Returns the configured role mappings.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn roles(&self) -> &BTreeMap<String, String> {
        &self.config.roles
    }

    // --- Session access ---

    /// Opens an existing session by ID.
    ///
    /// Loads the session's persisted messages and returns a session
    /// coordinator ready for `run_turn()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the session provider fails to load.
    pub fn open_session(&self, session_id: &str) -> Result<UrSession> {
        UrSession::open(
            self.engine.clone(),
            self.manifest.clone(),
            self.config.clone(),
            session_id,
        )
    }

    /// Lists sessions from the active session provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the session provider fails to load.
    #[expect(dead_code, reason = "public API surface for future clients")]
    pub fn list_sessions(&self) -> Result<Vec<wit_types::SessionInfo>> {
        let mut session_ext = crate::session::load_session_provider(&self.engine, &self.manifest)?;
        session_ext
            .list_sessions()?
            .map_err(|e| anyhow::anyhow!("list_sessions: {e}"))
    }
}
