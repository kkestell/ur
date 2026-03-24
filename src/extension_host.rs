//! Loads and runs WASM-based extensions via wasmtime.

use std::fmt;
use std::path::Path;

use wasmtime::component::{Component, HasSelf, Linker, ResourceAny, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};
use wasmtime_wasi_http::WasiHttpCtx;
use wasmtime_wasi_http::p2::{WasiHttpCtxView, WasiHttpView};

use crate::slot;

/// Bindgen-generated bindings for all four extension worlds.
///
/// The `general` module generates full bindings. Other modules
/// share `types` and `host` interfaces via `with` to avoid
/// duplicate type definitions.
mod worlds {
    pub mod general {
        wasmtime::component::bindgen!({
            path: "wit",
            world: "general-extension",
            require_store_data_send: true,
            with: {
                "wasi:http/types@0.2.6": wasmtime_wasi_http::p2::bindings::http::types,
                "wasi:http/outgoing-handler@0.2.6": wasmtime_wasi_http::p2::bindings::http::outgoing_handler,
                "wasi:io/error@0.2.6": wasmtime_wasi::p2::bindings::io::error,
                "wasi:io/streams@0.2.6": wasmtime_wasi::p2::bindings::sync::io::streams,
                "wasi:io/poll@0.2.6": wasmtime_wasi::p2::bindings::io::poll,
                "wasi:filesystem/types@0.2.6": wasmtime_wasi::p2::bindings::sync::filesystem::types,
                "wasi:filesystem/preopens@0.2.6": wasmtime_wasi::p2::bindings::filesystem::preopens,
                "wasi:clocks/monotonic-clock@0.2.6": wasmtime_wasi::p2::bindings::clocks::monotonic_clock,
            },
        });
    }

    pub mod llm {
        wasmtime::component::bindgen!({
            path: "wit",
            world: "llm-extension",
            require_store_data_send: true,
            with: {
                "ur:extension/types@0.4.0": super::general::ur::extension::types,
                "ur:extension/host@0.4.0": super::general::ur::extension::host,
                "wasi:http/types@0.2.6": wasmtime_wasi_http::p2::bindings::http::types,
                "wasi:http/outgoing-handler@0.2.6": wasmtime_wasi_http::p2::bindings::http::outgoing_handler,
                "wasi:io/error@0.2.6": wasmtime_wasi::p2::bindings::io::error,
                "wasi:io/streams@0.2.6": wasmtime_wasi::p2::bindings::sync::io::streams,
                "wasi:io/poll@0.2.6": wasmtime_wasi::p2::bindings::io::poll,
                "wasi:filesystem/types@0.2.6": wasmtime_wasi::p2::bindings::sync::filesystem::types,
                "wasi:filesystem/preopens@0.2.6": wasmtime_wasi::p2::bindings::filesystem::preopens,
                "wasi:clocks/monotonic-clock@0.2.6": wasmtime_wasi::p2::bindings::clocks::monotonic_clock,
            },
        });
    }

    pub mod session {
        wasmtime::component::bindgen!({
            path: "wit",
            world: "session-extension",
            require_store_data_send: true,
            with: {
                "ur:extension/types@0.4.0": super::general::ur::extension::types,
                "ur:extension/host@0.4.0": super::general::ur::extension::host,
                "wasi:http/types@0.2.6": wasmtime_wasi_http::p2::bindings::http::types,
                "wasi:http/outgoing-handler@0.2.6": wasmtime_wasi_http::p2::bindings::http::outgoing_handler,
                "wasi:io/error@0.2.6": wasmtime_wasi::p2::bindings::io::error,
                "wasi:io/streams@0.2.6": wasmtime_wasi::p2::bindings::sync::io::streams,
                "wasi:io/poll@0.2.6": wasmtime_wasi::p2::bindings::io::poll,
                "wasi:filesystem/types@0.2.6": wasmtime_wasi::p2::bindings::sync::filesystem::types,
                "wasi:filesystem/preopens@0.2.6": wasmtime_wasi::p2::bindings::filesystem::preopens,
                "wasi:clocks/monotonic-clock@0.2.6": wasmtime_wasi::p2::bindings::clocks::monotonic_clock,
            },
        });
    }

    pub mod compaction {
        wasmtime::component::bindgen!({
            path: "wit",
            world: "compaction-extension",
            require_store_data_send: true,
            with: {
                "ur:extension/types@0.4.0": super::general::ur::extension::types,
                "ur:extension/host@0.4.0": super::general::ur::extension::host,
                "wasi:http/types@0.2.6": wasmtime_wasi_http::p2::bindings::http::types,
                "wasi:http/outgoing-handler@0.2.6": wasmtime_wasi_http::p2::bindings::http::outgoing_handler,
                "wasi:io/error@0.2.6": wasmtime_wasi::p2::bindings::io::error,
                "wasi:io/streams@0.2.6": wasmtime_wasi::p2::bindings::sync::io::streams,
                "wasi:io/poll@0.2.6": wasmtime_wasi::p2::bindings::io::poll,
                "wasi:filesystem/types@0.2.6": wasmtime_wasi::p2::bindings::sync::filesystem::types,
                "wasi:filesystem/preopens@0.2.6": wasmtime_wasi::p2::bindings::filesystem::preopens,
                "wasi:clocks/monotonic-clock@0.2.6": wasmtime_wasi::p2::bindings::clocks::monotonic_clock,
            },
        });
    }
}

use worlds::general::ur::extension::host as wit_host;
/// Shared WIT types re-exported for use outside this module.
pub use worlds::general::ur::extension::types as wit_types;

/// WASI host state for an extension instance.
pub struct HostState {
    wasi_ctx: WasiCtx,
    http_ctx: WasiHttpCtx,
    resource_table: ResourceTable,
}

impl fmt::Debug for HostState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostState").finish_non_exhaustive()
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}

impl WasiHttpView for HostState {
    fn http(&mut self) -> WasiHttpCtxView<'_> {
        WasiHttpCtxView {
            ctx: &mut self.http_ctx,
            table: &mut self.resource_table,
            hooks: Default::default(),
        }
    }
}

/// The `types` interface is record-only but wasmtime generates a
/// `Host` trait for it that must be implemented.
impl worlds::general::ur::extension::types::Host for HostState {}

/// Unified host interface implementation.
///
/// Platform capabilities are stubbed — actual routing to active
/// providers will be wired up via role resolution.
impl wit_host::Host for HostState {
    fn log(&mut self, msg: String) {
        tracing::debug!(%msg, "extension log");
    }

    fn complete(
        &mut self,
        _messages: Vec<wit_types::Message>,
        _role: Option<String>,
    ) -> Result<wit_types::Completion, String> {
        Err("not yet routed".into())
    }

    fn load_session(&mut self, _id: String) -> Result<Vec<wit_types::SessionEvent>, String> {
        Err("not yet routed".into())
    }

    fn append_session(
        &mut self,
        _id: String,
        _event: wit_types::SessionEvent,
    ) -> Result<(), String> {
        Err("not yet routed".into())
    }

    fn list_sessions(&mut self) -> Result<Vec<wit_types::SessionInfo>, String> {
        Err("not yet routed".into())
    }

    fn compact(
        &mut self,
        _messages: Vec<wit_types::Message>,
    ) -> Result<Vec<wit_types::Message>, String> {
        Err("not yet routed".into())
    }
}

/// A loaded WASM extension, instantiated against its slot's world.
pub enum ExtensionInstance {
    Llm {
        store: Store<HostState>,
        bindings: worlds::llm::LlmExtension,
    },
    Session {
        store: Store<HostState>,
        bindings: worlds::session::SessionExtension,
    },
    Compaction {
        store: Store<HostState>,
        bindings: worlds::compaction::CompactionExtension,
    },
    General {
        store: Store<HostState>,
        bindings: worlds::general::GeneralExtension,
    },
}

impl fmt::Debug for ExtensionInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let variant = match self {
            Self::Llm { .. } => "Llm",
            Self::Session { .. } => "Session",
            Self::Compaction { .. } => "Compaction",
            Self::General { .. } => "General",
        };
        write!(f, "ExtensionInstance::{variant}")
    }
}

/// Creates a fresh WASI host state with inherited stdio.
///
/// When `capabilities` and `data_dir` are provided, preopens the data
/// directory with the declared filesystem permissions.
fn build_host_state(
    capabilities: Option<&wit_types::ExtensionCapabilities>,
    data_dir: Option<&Path>,
) -> HostState {
    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();

    if let (Some(caps), Some(dir)) = (capabilities, data_dir) {
        let read = caps.contains(wit_types::ExtensionCapabilities::FILESYSTEM_READ);
        let write = caps.contains(wit_types::ExtensionCapabilities::FILESYSTEM_WRITE);
        if read || write {
            let _ = std::fs::create_dir_all(dir);
            let dir_perms = if write {
                wasmtime_wasi::DirPerms::all()
            } else {
                wasmtime_wasi::DirPerms::READ
            };
            let file_perms = if write {
                wasmtime_wasi::FilePerms::all()
            } else {
                wasmtime_wasi::FilePerms::READ
            };
            builder
                .preopened_dir(dir, "/data", dir_perms, file_perms)
                .expect("preopened_dir");
        }
    }

    HostState {
        wasi_ctx: builder.build(),
        http_ctx: WasiHttpCtx::new(),
        resource_table: ResourceTable::new(),
    }
}

/// Converts `ExtensionCapabilities` flags to a list of string tags.
pub fn capabilities_to_strings(caps: wit_types::ExtensionCapabilities) -> Vec<String> {
    let mut out = Vec::new();
    if caps.contains(wit_types::ExtensionCapabilities::FILESYSTEM_READ) {
        out.push("filesystem-read".into());
    }
    if caps.contains(wit_types::ExtensionCapabilities::FILESYSTEM_WRITE) {
        out.push("filesystem-write".into());
    }
    if caps.contains(wit_types::ExtensionCapabilities::NETWORK) {
        out.push("network".into());
    }
    out
}

/// Converts a list of string tags to `ExtensionCapabilities` flags.
pub fn strings_to_capabilities(tags: &[String]) -> wit_types::ExtensionCapabilities {
    let mut caps = wit_types::ExtensionCapabilities::empty();
    for tag in tags {
        match tag.as_str() {
            "filesystem-read" => caps |= wit_types::ExtensionCapabilities::FILESYSTEM_READ,
            "filesystem-write" => caps |= wit_types::ExtensionCapabilities::FILESYSTEM_WRITE,
            "network" => caps |= wit_types::ExtensionCapabilities::NETWORK,
            _ => {}
        }
    }
    caps
}

/// Load-time options for capability enforcement and filesystem access.
#[derive(Debug, Default)]
pub struct LoadOptions<'a> {
    /// Declared capabilities from the manifest. When `None` (discovery),
    /// all WASI interfaces are linked.
    pub capabilities: Option<wit_types::ExtensionCapabilities>,
    /// Host-side data directory preopened as `/data` inside the guest.
    pub data_dir: Option<&'a Path>,
}

impl LoadOptions<'_> {
    /// Builds options from a manifest entry's capability strings.
    pub fn for_entry(entry: &crate::manifest::ManifestEntry) -> Self {
        Self {
            capabilities: Some(strings_to_capabilities(&entry.capabilities)),
            data_dir: None,
        }
    }
}

/// Validates that declared capabilities match the component's actual WASI imports.
///
/// Panics if the component imports a WASI capability it didn't declare.
pub fn validate_capabilities(
    engine: &Engine,
    component: &Component,
    capabilities: wit_types::ExtensionCapabilities,
    ext_id: &str,
) {
    let ct = component.component_type();
    let has_fs = ct
        .imports(engine)
        .any(|(name, _)| name.contains("wasi:filesystem"));
    let has_http = ct
        .imports(engine)
        .any(|(name, _)| name.contains("wasi:http"));

    let declares_fs = capabilities.contains(wit_types::ExtensionCapabilities::FILESYSTEM_READ)
        || capabilities.contains(wit_types::ExtensionCapabilities::FILESYSTEM_WRITE);

    assert!(
        !has_fs || declares_fs,
        "extension '{ext_id}' imports wasi:filesystem but did not declare \
         filesystem-read or filesystem-write"
    );
    assert!(
        !has_http || capabilities.contains(wit_types::ExtensionCapabilities::NETWORK),
        "extension '{ext_id}' imports wasi:http but did not declare network"
    );
}

impl ExtensionInstance {
    /// Compiles and instantiates an extension, auto-detecting its slot
    /// from the component's exports.
    ///
    /// When `opts.capabilities` is `Some`, HTTP is only linked if
    /// `network` is declared. When `None` (discovery), all interfaces
    /// are linked so that `declare_capabilities()` can be called.
    ///
    /// # Errors
    ///
    /// Returns an error if the component cannot be loaded, or if the
    /// WASM component does not satisfy the world's export requirements.
    pub fn load(engine: &Engine, path: &Path, opts: &LoadOptions<'_>) -> wasmtime::Result<Self> {
        let (instance, _component) = Self::load_returning_component(engine, path, opts)?;
        Ok(instance)
    }

    /// Like `load`, but also returns the compiled `Component` so callers
    /// (e.g. discovery) can inspect it without recompiling.
    pub fn load_returning_component(
        engine: &Engine,
        path: &Path,
        opts: &LoadOptions<'_>,
    ) -> wasmtime::Result<(Self, Component)> {
        let component = Component::from_file(engine, path)?;
        let detected = slot::detect_slot(engine, &component);

        let link_http = match &opts.capabilities {
            Some(caps) => caps.contains(wit_types::ExtensionCapabilities::NETWORK),
            None => true, // discovery: link everything
        };

        let mut linker = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        if link_http {
            wasmtime_wasi_http::p2::add_only_http_to_linker_sync(&mut linker)?;
        }
        worlds::general::ur::extension::host::add_to_linker::<_, HasSelf<_>>(
            &mut linker,
            |state| state,
        )?;

        let host_state = build_host_state(opts.capabilities.as_ref(), opts.data_dir);

        let instance = match detected {
            Some("llm-provider") => {
                let mut store = Store::new(engine, host_state);
                let bindings =
                    worlds::llm::LlmExtension::instantiate(&mut store, &component, &linker)?;
                Self::Llm { store, bindings }
            }
            Some("session-provider") => {
                let mut store = Store::new(engine, host_state);
                let bindings = worlds::session::SessionExtension::instantiate(
                    &mut store, &component, &linker,
                )?;
                Self::Session { store, bindings }
            }
            Some("compaction-provider") => {
                let mut store = Store::new(engine, host_state);
                let bindings = worlds::compaction::CompactionExtension::instantiate(
                    &mut store, &component, &linker,
                )?;
                Self::Compaction { store, bindings }
            }
            Some(_) | None => {
                let mut store = Store::new(engine, host_state);
                let bindings = worlds::general::GeneralExtension::instantiate(
                    &mut store, &component, &linker,
                )?;
                Self::General { store, bindings }
            }
        };

        Ok((instance, component))
    }

    /// Returns the slot name for this extension, or `None` for general extensions.
    pub fn slot_name(&self) -> Option<&'static str> {
        match self {
            Self::Llm { .. } => Some("llm-provider"),
            Self::Session { .. } => Some("session-provider"),
            Self::Compaction { .. } => Some("compaction-provider"),
            Self::General { .. } => None,
        }
    }

    /// Returns the extension's self-declared identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps.
    pub fn id(&mut self) -> wasmtime::Result<String> {
        match self {
            Self::Llm { store, bindings } => bindings.ur_extension_extension().call_id(store),
            Self::Session { store, bindings } => bindings.ur_extension_extension().call_id(store),
            Self::Compaction { store, bindings } => {
                bindings.ur_extension_extension().call_id(store)
            }
            Self::General { store, bindings } => bindings.ur_extension_extension().call_id(store),
        }
    }

    /// Returns the extension's declared WASI capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps.
    pub fn declare_capabilities(&mut self) -> wasmtime::Result<wit_types::ExtensionCapabilities> {
        match self {
            Self::Llm { store, bindings } => bindings
                .ur_extension_extension()
                .call_declare_capabilities(store),
            Self::Session { store, bindings } => bindings
                .ur_extension_extension()
                .call_declare_capabilities(store),
            Self::Compaction { store, bindings } => bindings
                .ur_extension_extension()
                .call_declare_capabilities(store),
            Self::General { store, bindings } => bindings
                .ur_extension_extension()
                .call_declare_capabilities(store),
        }
    }

    /// Returns the extension's self-declared human-readable name.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps.
    pub fn name(&mut self) -> wasmtime::Result<String> {
        match self {
            Self::Llm { store, bindings } => bindings.ur_extension_extension().call_name(store),
            Self::Session { store, bindings } => bindings.ur_extension_extension().call_name(store),
            Self::Compaction { store, bindings } => {
                bindings.ur_extension_extension().call_name(store)
            }
            Self::General { store, bindings } => bindings.ur_extension_extension().call_name(store),
        }
    }

    /// Calls the extension's `init` function with configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps.
    pub fn init(&mut self, config: &[(String, String)]) -> wasmtime::Result<Result<(), String>> {
        let entries: Vec<wit_types::ConfigEntry> = config
            .iter()
            .map(|(k, v)| wit_types::ConfigEntry {
                key: k.clone(),
                value: v.clone(),
            })
            .collect();

        match self {
            Self::Llm { store, bindings } => {
                bindings.ur_extension_extension().call_init(store, &entries)
            }
            Self::Session { store, bindings } => {
                bindings.ur_extension_extension().call_init(store, &entries)
            }
            Self::Compaction { store, bindings } => {
                bindings.ur_extension_extension().call_init(store, &entries)
            }
            Self::General { store, bindings } => {
                bindings.ur_extension_extension().call_init(store, &entries)
            }
        }
    }

    /// Lists the tools this extension can handle.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps.
    pub fn list_tools(&mut self) -> wasmtime::Result<Vec<wit_types::ToolDescriptor>> {
        match self {
            Self::Llm { store, bindings } => {
                bindings.ur_extension_extension().call_list_tools(store)
            }
            Self::Session { store, bindings } => {
                bindings.ur_extension_extension().call_list_tools(store)
            }
            Self::Compaction { store, bindings } => {
                bindings.ur_extension_extension().call_list_tools(store)
            }
            Self::General { store, bindings } => {
                bindings.ur_extension_extension().call_list_tools(store)
            }
        }
    }

    /// Lists the configurable settings declared by this extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps.
    pub fn list_settings(&mut self) -> wasmtime::Result<Vec<wit_types::SettingDescriptor>> {
        match self {
            Self::Llm { store, bindings } => {
                bindings.ur_extension_extension().call_list_settings(store)
            }
            Self::Session { store, bindings } => {
                bindings.ur_extension_extension().call_list_settings(store)
            }
            Self::Compaction { store, bindings } => {
                bindings.ur_extension_extension().call_list_settings(store)
            }
            Self::General { store, bindings } => {
                bindings.ur_extension_extension().call_list_settings(store)
            }
        }
    }

    /// Calls a named tool on the extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps.
    pub fn call_tool(
        &mut self,
        name: &str,
        args: &str,
    ) -> wasmtime::Result<Result<String, String>> {
        match self {
            Self::Llm { store, bindings } => bindings
                .ur_extension_extension()
                .call_call_tool(store, name, args),
            Self::Session { store, bindings } => bindings
                .ur_extension_extension()
                .call_call_tool(store, name, args),
            Self::Compaction { store, bindings } => bindings
                .ur_extension_extension()
                .call_call_tool(store, name, args),
            Self::General { store, bindings } => bindings
                .ur_extension_extension()
                .call_call_tool(store, name, args),
        }
    }

    /// Returns the provider ID declared by an LLM provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not an LLM provider.
    pub fn provider_id(&mut self) -> wasmtime::Result<Result<String, String>> {
        match self {
            Self::Llm { store, bindings } => {
                let id = bindings
                    .ur_extension_llm_provider()
                    .call_provider_id(store)?;
                Ok(Ok(id))
            }
            _ => Ok(Err("not an llm-provider".into())),
        }
    }

    /// Lists models declared by an LLM provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not an LLM provider.
    pub fn list_models(
        &mut self,
    ) -> wasmtime::Result<Result<Vec<wit_types::ModelDescriptor>, String>> {
        match self {
            Self::Llm { store, bindings } => {
                let models = bindings
                    .ur_extension_llm_provider()
                    .call_list_models(store)?;
                Ok(Ok(models))
            }
            _ => Ok(Err("not an llm-provider".into())),
        }
    }

    /// Begins a streaming completion and pulls chunks via a callback.
    ///
    /// Calls `complete` on the LLM provider, then pulls
    /// `completion-chunk` values from the returned resource until
    /// exhausted. Each chunk is passed to `on_chunk`.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not an LLM provider.
    pub fn complete(
        &mut self,
        messages: &[wit_types::Message],
        model: &str,
        settings: &[wit_types::ConfigSetting],
        tools: &[wit_types::ToolDescriptor],
        tool_choice: Option<&wit_types::ToolChoice>,
        mut on_chunk: impl FnMut(&wit_types::CompletionChunk),
    ) -> wasmtime::Result<Result<(), String>> {
        let Self::Llm { store, bindings } = self else {
            return Ok(Err("not an llm-provider".into()));
        };

        let stream: ResourceAny = match bindings.ur_extension_llm_provider().call_complete(
            &mut *store,
            messages,
            model,
            settings,
            tools,
            tool_choice,
        )? {
            Ok(s) => s,
            Err(e) => return Ok(Err(e)),
        };

        let accessor = bindings.ur_extension_llm_provider().completion_stream();

        while let Some(chunk) = accessor.call_next(&mut *store, stream)? {
            on_chunk(&chunk);
        }

        stream.resource_drop(&mut *store)?;
        Ok(Ok(()))
    }

    /// Loads session events from a session provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not a session provider.
    pub fn load_session(
        &mut self,
        id: &str,
    ) -> wasmtime::Result<Result<Vec<wit_types::SessionEvent>, String>> {
        match self {
            Self::Session { store, bindings } => bindings
                .ur_extension_session_provider()
                .call_load(store, id),
            _ => Ok(Err("not a session-provider".into())),
        }
    }

    /// Appends an event to a session via a session provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not a session provider.
    pub fn append_session(
        &mut self,
        id: &str,
        event: &wit_types::SessionEvent,
    ) -> wasmtime::Result<Result<(), String>> {
        match self {
            Self::Session { store, bindings } => bindings
                .ur_extension_session_provider()
                .call_append(store, id, event),
            _ => Ok(Err("not a session-provider".into())),
        }
    }

    /// Lists sessions from a session provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not a session provider.
    pub fn list_sessions(
        &mut self,
    ) -> wasmtime::Result<Result<Vec<wit_types::SessionInfo>, String>> {
        match self {
            Self::Session { store, bindings } => bindings
                .ur_extension_session_provider()
                .call_list_sessions(store),
            _ => Ok(Err("not a session-provider".into())),
        }
    }

    /// Compacts messages via a compaction provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not a compaction provider.
    pub fn compact(
        &mut self,
        messages: &[wit_types::Message],
    ) -> wasmtime::Result<Result<Vec<wit_types::Message>, String>> {
        match self {
            Self::Compaction { store, bindings } => bindings
                .ur_extension_compaction_provider()
                .call_compact(store, messages),
            _ => Ok(Err("not a compaction-provider".into())),
        }
    }
}
