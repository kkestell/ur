//! Loads and runs WASM-based extensions via wasmtime.

use std::fmt;
use std::path::Path;

use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    path: "wit",
    world: "ur-extension",
});

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

impl ur::extension::host::Host for HostState {
    fn log(&mut self, msg: String) {
        println!("[host log] {msg}");
    }
}

/// A loaded WASM extension with callable tool interface.
pub struct ExtensionInstance {
    store: Store<HostState>,
    bindings: UrExtension,
}

impl fmt::Debug for ExtensionInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionInstance").finish_non_exhaustive()
    }
}

impl ExtensionInstance {
    /// Compiles and instantiates an extension from a WASM component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component cannot be loaded from `path`,
    /// or if linker setup or instantiation fails.
    pub fn load(engine: &Engine, path: &Path) -> wasmtime::Result<Self> {
        let component = Component::from_file(engine, path)?;

        let mut linker = Linker::new(engine);
        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)?;
        UrExtension::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state)?;

        let wasi_ctx = WasiCtxBuilder::new().inherit_stdio().build();
        let mut store = Store::new(
            engine,
            HostState {
                wasi_ctx,
                resource_table: ResourceTable::new(),
            },
        );

        let bindings = UrExtension::instantiate(&mut store, &component, &linker)?;

        Ok(Self { store, bindings })
    }

    /// Calls the extension's `register` function.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps or fails.
    pub fn register(
        &mut self,
    ) -> wasmtime::Result<exports::ur::extension::extension::ExtensionManifest> {
        self.bindings
            .ur_extension_extension()
            .call_register(&mut self.store)
    }

    /// Invokes a named tool with JSON arguments.
    ///
    /// The inner `Result` is the guest's success/error response.
    ///
    /// # Errors
    ///
    /// Returns an error if the guest call traps or fails.
    pub fn call_tool(
        &mut self,
        name: &str,
        args_json: &str,
    ) -> wasmtime::Result<Result<String, String>> {
        self.bindings
            .ur_extension_extension()
            .call_call_tool(&mut self.store, name, args_json)
    }
}

// Rust guideline compliant 2026-02-21
