//! WIT compatibility tests for WASM tools and channels.
//!
//! These tests verify that pre-built WASM components can be compiled and
//! instantiated against the current host linker. If the WIT interface
//! changes, these tests catch any breakage in existing tools/channels.
//!
//! Prerequisites: build WASM extensions first with:
//!   ./scripts/build-wasm-extensions.sh
//!
//! The tests are skipped (not failed) when no WASM artefacts are found,
//! so `cargo test` still passes without building extensions first.
//! CI runs the build script before these tests.

#[path = "wit_compat/harness.rs"]
mod harness;

use std::path::{Path, PathBuf};

use harness::{
    DiscoveredExtension, ExtensionKind, compile_component, create_engine, discover_extensions,
    instantiate_channel_component, instantiate_tool_component,
};

#[test]
fn wit_compat_tool_components_compile_and_instantiate() {
    let Some(failures) = verify_components(ExtensionKind::Tool, instantiate_tool_component) else {
        return;
    };

    assert!(
        failures.is_empty(),
        "WIT compatibility failures for tools:\n{}",
        failures.join("\n")
    );
}

#[test]
fn wit_compat_channel_components_compile_and_instantiate() {
    let Some(failures) = verify_components(ExtensionKind::Channel, instantiate_channel_component)
    else {
        return;
    };

    assert!(
        failures.is_empty(),
        "WIT compatibility failures for channels:\n{}",
        failures.join("\n")
    );
}

/// Compile and instantiate one component, returning the failure text on error.
type Instantiator = fn(&wasmtime::Engine, &wasmtime::component::Component) -> Result<(), String>;

/// Compile and instantiate every discovered extension of `kind`.
///
/// Returns the collected failures, or `None` when there is nothing to verify:
/// either the registry has no extension of this kind, or none of them has a
/// built WASM artefact. Both skip rather than pass, so the suite still works
/// before `./scripts/build-wasm-extensions.sh` has run.
fn verify_components(kind: ExtensionKind, instantiate: Instantiator) -> Option<Vec<String>> {
    let extensions = discover_extensions();
    let engine = create_engine();

    let selected: Vec<_> = extensions.iter().filter(|ext| ext.kind == kind).collect();
    if selected.is_empty() {
        eprintln!("SKIP: no {} extensions found in registry", kind.label());
        return None;
    }

    let outcomes: Vec<_> = selected
        .iter()
        .map(|ext| verify_extension(&engine, ext, instantiate))
        .collect();

    if outcomes.iter().all(Option::is_none) {
        eprintln!("SKIP: no WASM artefacts found (build extensions first)");
        return None;
    }

    Some(
        outcomes
            .into_iter()
            .flatten()
            .filter_map(Result::err)
            .collect(),
    )
}

/// Verify one extension, or report that it has no built artefact.
fn verify_extension(
    engine: &wasmtime::Engine,
    ext: &DiscoveredExtension,
    instantiate: Instantiator,
) -> Option<Result<(), String>> {
    let Some(wasm_path) = axinite::registry::artifacts::find_wasm_artifact(
        &ext.source_dir,
        &ext.crate_name,
        "release",
    ) else {
        eprintln!(
            "  SKIP {}: no built WASM artefact (run ./scripts/build-wasm-extensions.sh)",
            ext.name
        );
        return None;
    };

    eprintln!("  TEST {}: {}", ext.name, wasm_path.display());

    let wasm_bytes = match std::fs::read(&wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => panic!("failed to read {}: {e}", wasm_path.display()),
    };

    let outcome = compile_component(engine, &wasm_bytes)
        .and_then(|component| instantiate(engine, &component));
    Some(outcome.map_err(|e| format!("{}: {e}", ext.name)))
}

#[test]
fn wit_compat_all_registry_extensions_have_source() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let missing: Vec<String> = REGISTRY_DIRS
        .iter()
        .flat_map(|dir| missing_sources_in_registry_dir(&repo_root, dir))
        .collect();

    assert!(
        missing.is_empty(),
        "Registry entries with missing sources:\n{}",
        missing.join("\n")
    );
}

/// Registry directories whose manifests are checked for a usable source.
const REGISTRY_DIRS: [&str; 2] = ["registry/tools", "registry/channels"];

/// Collect the source problems of every JSON manifest directly under `dir`.
///
/// A registry directory that does not exist contributes nothing.
fn missing_sources_in_registry_dir(repo_root: &Path, dir: &str) -> Vec<String> {
    let registry_dir = repo_root.join(dir);
    if !registry_dir.exists() {
        return Vec::new();
    }

    json_manifest_paths(&registry_dir)
        .iter()
        .filter_map(|path| manifest_source_problem(repo_root, path))
        .collect()
}

/// The paths of the JSON manifests directly under one registry directory.
fn json_manifest_paths(registry_dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(registry_dir)
        .expect("failed to read registry dir")
        .map(|entry| entry.expect("failed to read directory entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect()
}

/// Describe what is wrong with one manifest's `source` block, if anything.
fn manifest_source_problem(repo_root: &Path, path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&content).unwrap();

    let name = manifest["name"].as_str().unwrap_or("unknown");
    match (
        manifest["source"]["dir"].as_str(),
        manifest["source"]["crate_name"].as_str(),
    ) {
        (Some(dir), Some(_)) if repo_root.join(dir).exists() => None,
        (Some(dir), Some(_)) => Some(format!("{name}: source dir '{dir}' does not exist")),
        _ => Some(format!("{name}: missing source.dir or source.crate_name")),
    }
}

#[test]
fn wit_files_contain_version_annotation() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    for wit_file in &["wit/tool.wit", "wit/channel.wit"] {
        let path = repo_root.join(wit_file);
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => panic!("failed to read {wit_file}: {e}"),
        };

        assert!(
            content.contains("package near:agent@"),
            "{wit_file} must contain a versioned package declaration (e.g., 'package near:agent@0.3.0;')"
        );
    }
}

#[test]
fn wit_version_constants_match_wit_files() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let tool_wit = std::fs::read_to_string(repo_root.join("wit/tool.wit"))
        .expect("failed to read wit/tool.wit");
    let channel_wit = std::fs::read_to_string(repo_root.join("wit/channel.wit"))
        .expect("failed to read wit/channel.wit");

    let expected_tool = format!(
        "package near:agent@{};",
        axinite::tools::wasm::WIT_TOOL_VERSION
    );
    let expected_channel = format!(
        "package near:agent@{};",
        axinite::tools::wasm::WIT_CHANNEL_VERSION
    );

    assert!(
        tool_wit.contains(&expected_tool),
        "wit/tool.wit version must match WIT_TOOL_VERSION constant ({})",
        axinite::tools::wasm::WIT_TOOL_VERSION
    );
    assert!(
        channel_wit.contains(&expected_channel),
        "wit/channel.wit version must match WIT_CHANNEL_VERSION constant ({})",
        axinite::tools::wasm::WIT_CHANNEL_VERSION
    );
}
