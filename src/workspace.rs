//! Workspace-scoped coordinator for extensions, roles, and sessions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;

use crate::config::{self, UserConfig};
use crate::manifest::{self, ManifestEntry, WorkspaceManifest};
use crate::model::{self, ProviderModels};
use crate::provider;
use crate::providers::compaction::StubCompactionProvider;
use crate::providers::session_jsonl::JsonlSessionProvider;
use crate::providers::{CompactionProvider, LlmProvider, SessionProvider};
use crate::session::UrSession;
use crate::types::{ModelDescriptor, ToolDescriptor};

/// A role mapping entry.
#[derive(Debug)]
pub struct RoleEntry {
    pub role: String,
    pub model_ref: String,
}

/// Result of resolving a role.
#[derive(Debug)]
pub struct ResolvedRole {
    pub role: String,
    pub provider_id: String,
    pub model_id: String,
}

/// Workspace-scoped coordinator.
pub struct UrWorkspace {
    ur_root: PathBuf,
    workspace_path: PathBuf,
    manifest: WorkspaceManifest,
    config: UserConfig,
    llm_providers: Vec<Arc<dyn LlmProvider>>,
}

impl UrWorkspace {
    pub(crate) fn new(
        ur_root: PathBuf,
        workspace_path: PathBuf,
        manifest: WorkspaceManifest,
        config: UserConfig,
    ) -> Self {
        // Instantiate native LLM providers based on available API keys.
        let mut llm_providers: Vec<Arc<dyn LlmProvider>> = Vec::new();

        let google_key = provider::resolve_api_key("google");
        if let Some(key) = google_key {
            llm_providers.push(Arc::new(crate::providers::google::GoogleProvider::new(key)));
        }

        let openrouter_key = provider::resolve_api_key("openrouter");
        if let Some(key) = openrouter_key {
            llm_providers.push(Arc::new(
                crate::providers::openrouter::OpenRouterProvider::new(key),
            ));
        }

        Self {
            ur_root,
            workspace_path,
            manifest,
            config,
            llm_providers,
        }
    }

    #[must_use]
    pub fn ur_root(&self) -> &Path {
        &self.ur_root
    }

    #[must_use]
    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }

    #[must_use]
    pub fn manifest(&self) -> &WorkspaceManifest {
        &self.manifest
    }

    #[must_use]
    pub fn config(&self) -> &UserConfig {
        &self.config
    }

    // --- Extension management ---

    #[must_use]
    pub fn list_extensions(&self) -> &[ManifestEntry] {
        &self.manifest.extensions
    }

    pub fn enable_extension(&mut self, id: &str) -> Result<()> {
        manifest::enable(&mut self.manifest, id)?;
        manifest::save_manifest(&self.ur_root, &self.workspace_path, &self.manifest)?;
        Ok(())
    }

    pub fn disable_extension(&mut self, id: &str) -> Result<()> {
        manifest::disable(&mut self.manifest, id)?;
        manifest::save_manifest(&self.ur_root, &self.workspace_path, &self.manifest)?;
        Ok(())
    }

    pub fn find_extension(&self, id: &str) -> Result<&ManifestEntry> {
        manifest::find_entry(&self.manifest, id)
    }

    // --- Role management ---

    pub fn list_roles(&self) -> Result<Vec<RoleEntry>> {
        let providers = self.provider_models();
        let mut entries = Vec::new();

        if !self.config.roles.contains_key("default") {
            if let Ok((p, m)) = model::resolve_role(&self.config, "default", &providers) {
                entries.push(RoleEntry {
                    role: "default".into(),
                    model_ref: format!("{p}/{m}"),
                });
            }
        }
        for (role, model_ref) in &self.config.roles {
            entries.push(RoleEntry {
                role: role.clone(),
                model_ref: model_ref.clone(),
            });
        }
        Ok(entries)
    }

    pub fn resolve_role(&self, role: &str) -> Result<ResolvedRole> {
        let providers = self.provider_models();
        let (provider_id, model_id) = model::resolve_role(&self.config, role, &providers)?;
        Ok(ResolvedRole {
            role: role.into(),
            provider_id,
            model_id,
        })
    }

    pub fn set_role(&mut self, role: &str, model_ref: &str) -> Result<ResolvedRole> {
        let providers = self.provider_models();
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

    // --- Session access ---

    pub fn open_session(&self, session_id: &str) -> Result<UrSession> {
        let sessions_dir = self.sessions_dir();
        let session_provider: Arc<dyn SessionProvider> =
            Arc::new(JsonlSessionProvider::new(&sessions_dir));
        let compaction_provider: Arc<dyn CompactionProvider> = Arc::new(StubCompactionProvider);

        UrSession::open(
            self.llm_providers.clone(),
            session_provider,
            compaction_provider,
            self.config.clone(),
            session_id,
            Vec::new(), // no tool handlers yet, Lua extensions will add them
        )
    }

    pub fn list_sessions(&self) -> Result<Vec<crate::types::SessionInfo>> {
        let sessions_dir = self.sessions_dir();
        let provider = JsonlSessionProvider::new(&sessions_dir);
        provider.list_sessions()
    }

    #[must_use]
    pub fn roles(&self) -> &BTreeMap<String, String> {
        &self.config.roles
    }

    fn sessions_dir(&self) -> PathBuf {
        manifest::manifest_dir(&self.ur_root, &self.workspace_path).join("sessions")
    }

    fn provider_models(&self) -> ProviderModels {
        model::collect_provider_models(
            &self
                .llm_providers
                .iter()
                .map(|p| p.as_ref())
                .collect::<Vec<_>>(),
        )
    }
}
