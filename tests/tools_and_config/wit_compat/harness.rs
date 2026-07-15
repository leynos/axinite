//! Shared harness for the WIT compatibility tests: extension discovery,
//! component compilation, host-function stubs, and instantiation helpers.

use std::path::PathBuf;

use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

/// Minimal store data that satisfies WasiView for component instantiation.
struct TestStoreData {
    wasi: WasiCtx,
    table: ResourceTable,
}

impl TestStoreData {
    fn new() -> Self {
        Self {
            wasi: WasiCtxBuilder::new().build(),
            table: ResourceTable::new(),
        }
    }
}

impl WasiView for TestStoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// Extension kind from the registry manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExtensionKind {
    Tool,
    Channel,
}

/// A discovered WASM extension from the registry.
pub(super) struct DiscoveredExtension {
    pub(super) name: String,
    pub(super) source_dir: PathBuf,
    pub(super) crate_name: String,
    pub(super) kind: ExtensionKind,
}

/// Parse registry manifests to discover all WASM extensions.
pub(super) fn discover_extensions() -> Vec<DiscoveredExtension> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut extensions = Vec::new();

    for dir in &["registry/tools", "registry/channels"] {
        let registry_dir = repo_root.join(dir);
        if registry_dir.exists() {
            extensions.extend(discover_extensions_in_dir(&registry_dir, &repo_root));
        }
    }

    extensions
}

/// Discover extensions from the JSON manifests directly under one registry
/// directory.
fn discover_extensions_in_dir(
    registry_dir: &std::path::Path,
    repo_root: &std::path::Path,
) -> Vec<DiscoveredExtension> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(registry_dir).expect("failed to read registry dir") {
        let entry = entry.expect("failed to read directory entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(extension) = parse_extension_manifest(&path, repo_root) {
            found.push(extension);
        }
    }
    found
}

/// Parse one registry manifest into a discovered extension.
///
/// Returns `None` when the manifest's kind is not a recognised tool or
/// channel, when the source directory or crate name is missing, or when
/// the source directory does not exist on disk.
fn parse_extension_manifest(
    path: &std::path::Path,
    repo_root: &std::path::Path,
) -> Option<DiscoveredExtension> {
    let content = std::fs::read_to_string(path).expect("failed to read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&content).expect("failed to parse manifest");

    let name = manifest["name"].as_str().unwrap_or("unknown").to_string();
    let kind = match manifest["kind"].as_str() {
        Some("tool") => ExtensionKind::Tool,
        Some("channel") => ExtensionKind::Channel,
        _ => return None,
    };
    let source_dir = manifest["source"]["dir"]
        .as_str()
        .map(|d| repo_root.join(d))?;
    let crate_name = manifest["source"]["crate_name"].as_str()?.to_string();

    if !source_dir.exists() {
        return None;
    }

    Some(DiscoveredExtension {
        name,
        source_dir,
        crate_name,
        kind,
    })
}

pub(super) fn compile_component(
    engine: &wasmtime::Engine,
    wasm_bytes: &[u8],
) -> Result<wasmtime::component::Component, String> {
    wasmtime::component::Component::new(engine, wasm_bytes)
        .map_err(|e| format!("compilation failed: {e}"))
}

/// Stub host functions shared between tool and channel interfaces:
/// log, now-millis, workspace-read, http-request, secret-exists.
fn stub_shared_host_functions(
    host: &mut wasmtime::component::LinkerInstance<'_, TestStoreData>,
) -> Result<(), String> {
    host.func_new("log", |_ctx, _func, _args, _results| Ok(()))
        .map_err(|e| format!("stub 'log': {e}"))?;

    host.func_new("now-millis", |_ctx, _func, _args, results| {
        results[0] = wasmtime::component::Val::U64(0);
        Ok(())
    })
    .map_err(|e| format!("stub 'now-millis': {e}"))?;

    host.func_new("workspace-read", |_ctx, _func, _args, results| {
        results[0] = wasmtime::component::Val::Option(None);
        Ok(())
    })
    .map_err(|e| format!("stub 'workspace-read': {e}"))?;

    host.func_new("http-request", |_ctx, _func, _args, results| {
        results[0] = wasmtime::component::Val::Result(Err(Some(Box::new(
            wasmtime::component::Val::String("stub".into()),
        ))));
        Ok(())
    })
    .map_err(|e| format!("stub 'http-request': {e}"))?;

    host.func_new("secret-exists", |_ctx, _func, _args, results| {
        results[0] = wasmtime::component::Val::Bool(false);
        Ok(())
    })
    .map_err(|e| format!("stub 'secret-exists': {e}"))?;

    Ok(())
}

/// Instantiate a tool component (world: sandboxed-tool, imports: near:agent/host).
pub(super) fn instantiate_tool_component(
    engine: &wasmtime::Engine,
    component: &wasmtime::component::Component,
) -> Result<(), String> {
    use wasmtime::Store;
    use wasmtime::component::Linker;

    let mut linker: Linker<TestStoreData> = Linker::new(engine);

    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
        .map_err(|e| format!("WASI linker failed: {e}"))?;

    // If the WIT added/removed/renamed a function, stub registration
    // or instantiation will fail.
    // Register stubs for both versioned (0.3.0+) and unversioned (pre-0.3.0) interface
    // paths so that both old and new WASM artifacts can instantiate.
    for interface in &["near:agent/host", "near:agent/host@0.3.0"] {
        let mut root = linker.root();
        if let Ok(mut host) = root.instance(interface) {
            stub_shared_host_functions(&mut host)?;

            host.func_new("tool-invoke", |_ctx, _func, _args, results| {
                results[0] = wasmtime::component::Val::Result(Err(Some(Box::new(
                    wasmtime::component::Val::String("stub".into()),
                ))));
                Ok(())
            })
            .map_err(|e| format!("stub 'tool-invoke': {e}"))?;
        }
    }

    let mut store = Store::new(engine, TestStoreData::new());
    linker
        .instantiate(&mut store, component)
        .map_err(|e| format!("instantiation failed: {e}"))?;

    Ok(())
}

/// Instantiate a channel component (world: sandboxed-channel, imports: near:agent/channel-host).
pub(super) fn instantiate_channel_component(
    engine: &wasmtime::Engine,
    component: &wasmtime::component::Component,
) -> Result<(), String> {
    use wasmtime::Store;
    use wasmtime::component::Linker;

    let mut linker: Linker<TestStoreData> = Linker::new(engine);

    wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
        .map_err(|e| format!("WASI linker failed: {e}"))?;

    // Register stubs for both versioned (0.3.0+) and unversioned (pre-0.3.0) interface
    // paths so that both old and new WASM artifacts can instantiate.
    // Register stubs under both versioned and unversioned interface paths.
    // This helper avoids repeating the stub registration code.
    fn stub_channel_host(
        host: &mut wasmtime::component::LinkerInstance<'_, TestStoreData>,
    ) -> Result<(), String> {
        stub_shared_host_functions(host)?;

        host.func_new("store-attachment-data", |_ctx, _func, _args, results| {
            results[0] = wasmtime::component::Val::Result(Ok(None));
            Ok(())
        })
        .map_err(|e| format!("stub 'store-attachment-data': {e}"))?;

        host.func_new("emit-message", |_ctx, _func, _args, _results| Ok(()))
            .map_err(|e| format!("stub 'emit-message': {e}"))?;

        host.func_new("workspace-write", |_ctx, _func, _args, results| {
            results[0] = wasmtime::component::Val::Result(Ok(None));
            Ok(())
        })
        .map_err(|e| format!("stub 'workspace-write': {e}"))?;

        host.func_new("pairing-upsert-request", |_ctx, _func, _args, results| {
            results[0] = wasmtime::component::Val::Result(Err(Some(Box::new(
                wasmtime::component::Val::String("stub".into()),
            ))));
            Ok(())
        })
        .map_err(|e| format!("stub 'pairing-upsert-request': {e}"))?;

        host.func_new("pairing-is-allowed", |_ctx, _func, _args, results| {
            results[0] = wasmtime::component::Val::Result(Err(Some(Box::new(
                wasmtime::component::Val::String("stub".into()),
            ))));
            Ok(())
        })
        .map_err(|e| format!("stub 'pairing-is-allowed': {e}"))?;

        host.func_new("pairing-read-allow-from", |_ctx, _func, _args, results| {
            results[0] = wasmtime::component::Val::Result(Err(Some(Box::new(
                wasmtime::component::Val::String("stub".into()),
            ))));
            Ok(())
        })
        .map_err(|e| format!("stub 'pairing-read-allow-from': {e}"))?;

        Ok(())
    }

    {
        let mut root = linker.root();
        let mut host = root
            .instance("near:agent/channel-host")
            .map_err(|e| format!("failed to create unversioned channel-host: {e}"))?;
        stub_channel_host(&mut host)?;
    }
    {
        let mut root = linker.root();
        let mut host = root
            .instance("near:agent/channel-host@0.3.0")
            .map_err(|e| format!("failed to create versioned channel-host@0.3.0: {e}"))?;
        stub_channel_host(&mut host)?;
    }

    let mut store = Store::new(engine, TestStoreData::new());
    linker
        .instantiate(&mut store, component)
        .map_err(|e| format!("instantiation failed: {e}"))?;

    Ok(())
}

pub(super) fn create_engine() -> wasmtime::Engine {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    config.wasm_threads(false);
    wasmtime::Engine::new(&config).expect("failed to create wasmtime engine")
}
