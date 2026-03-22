//! Loads and runs WASM-based extensions via wasmtime.

use std::fmt;
use std::path::Path;

use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

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
        });
    }

    pub mod llm {
        wasmtime::component::bindgen!({
            path: "wit",
            world: "llm-extension",
            with: {
                "ur:extension/types@0.2.0": super::general::ur::extension::types,
                "ur:extension/host@0.2.0": super::general::ur::extension::host,
            },
        });
    }

    pub mod session {
        wasmtime::component::bindgen!({
            path: "wit",
            world: "session-extension",
            with: {
                "ur:extension/types@0.2.0": super::general::ur::extension::types,
                "ur:extension/host@0.2.0": super::general::ur::extension::host,
            },
        });
    }

    pub mod compaction {
        wasmtime::component::bindgen!({
            path: "wit",
            world: "compaction-extension",
            with: {
                "ur:extension/types@0.2.0": super::general::ur::extension::types,
                "ur:extension/host@0.2.0": super::general::ur::extension::host,
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

/// The `types` interface is record-only but wasmtime generates a
/// `Host` trait for it that must be implemented.
impl worlds::general::ur::extension::types::Host for HostState {}

/// Unified host interface implementation.
///
/// Platform capabilities are stubbed — actual routing to active
/// providers will be implemented in a later step.
impl wit_host::Host for HostState {
    fn log(&mut self, msg: String) {
        println!("[host log] {msg}");
    }

    fn complete(
        &mut self,
        _messages: Vec<wit_types::Message>,
        _opts: Option<wit_types::CompleteOpts>,
    ) -> Result<wit_types::Completion, String> {
        Err("not yet routed".into())
    }

    fn load_session(&mut self, _id: String) -> Result<Vec<wit_types::Message>, String> {
        Err("not yet routed".into())
    }

    fn append_session(&mut self, _id: String, _msg: wit_types::Message) -> Result<(), String> {
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
fn build_host_state() -> HostState {
    let wasi_ctx = WasiCtxBuilder::new().inherit_stdio().build();
    HostState {
        wasi_ctx,
        resource_table: ResourceTable::new(),
    }
}

impl ExtensionInstance {
    /// Compiles and instantiates an extension against the world for its slot.
    ///
    /// The `slot` parameter selects the world: `"llm-provider"`,
    /// `"session-provider"`, `"compaction-provider"`, or `None` for
    /// general-purpose extensions.
    ///
    /// # Errors
    ///
    /// Returns an error if the component cannot be loaded, or if the
    /// WASM component does not satisfy the world's export requirements.
    pub fn load(engine: &Engine, path: &Path, slot: Option<&str>) -> wasmtime::Result<Self> {
        let component = Component::from_file(engine, path)?;

        match slot {
            Some("llm-provider") => {
                let mut linker = Linker::new(engine);
                wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
                worlds::llm::LlmExtension::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| {
                    state
                })?;
                let mut store = Store::new(engine, build_host_state());
                let bindings =
                    worlds::llm::LlmExtension::instantiate(&mut store, &component, &linker)?;
                Ok(Self::Llm { store, bindings })
            }
            Some("session-provider") => {
                let mut linker = Linker::new(engine);
                wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
                worlds::session::SessionExtension::add_to_linker::<_, HasSelf<_>>(
                    &mut linker,
                    |state| state,
                )?;
                let mut store = Store::new(engine, build_host_state());
                let bindings = worlds::session::SessionExtension::instantiate(
                    &mut store, &component, &linker,
                )?;
                Ok(Self::Session { store, bindings })
            }
            Some("compaction-provider") => {
                let mut linker = Linker::new(engine);
                wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
                worlds::compaction::CompactionExtension::add_to_linker::<_, HasSelf<_>>(
                    &mut linker,
                    |state| state,
                )?;
                let mut store = Store::new(engine, build_host_state());
                let bindings = worlds::compaction::CompactionExtension::instantiate(
                    &mut store, &component, &linker,
                )?;
                Ok(Self::Compaction { store, bindings })
            }
            _ => {
                let mut linker = Linker::new(engine);
                wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
                worlds::general::GeneralExtension::add_to_linker::<_, HasSelf<_>>(
                    &mut linker,
                    |state| state,
                )?;
                let mut store = Store::new(engine, build_host_state());
                let bindings = worlds::general::GeneralExtension::instantiate(
                    &mut store, &component, &linker,
                )?;
                Ok(Self::General { store, bindings })
            }
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

    /// Calls `complete` on an LLM provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not an LLM provider.
    pub fn complete(
        &mut self,
        messages: &[wit_types::Message],
        opts: Option<&wit_types::CompleteOpts>,
    ) -> wasmtime::Result<Result<wit_types::Completion, String>> {
        match self {
            Self::Llm { store, bindings } => bindings
                .ur_extension_llm_provider()
                .call_complete(store, messages, opts),
            _ => Ok(Err("not an llm-provider".into())),
        }
    }

    /// Loads session messages from a session provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not a session provider.
    pub fn load_session(
        &mut self,
        id: &str,
    ) -> wasmtime::Result<Result<Vec<wit_types::Message>, String>> {
        match self {
            Self::Session { store, bindings } => bindings
                .ur_extension_session_provider()
                .call_load(store, id),
            _ => Ok(Err("not a session-provider".into())),
        }
    }

    /// Appends a message to a session via a session provider extension.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps, or `Ok(Err(...))`
    /// if this is not a session provider.
    pub fn append_session(
        &mut self,
        id: &str,
        msg: &wit_types::Message,
    ) -> wasmtime::Result<Result<(), String>> {
        match self {
            Self::Session { store, bindings } => bindings
                .ur_extension_session_provider()
                .call_append(store, id, msg),
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

// Rust guideline compliant 2026-02-21
