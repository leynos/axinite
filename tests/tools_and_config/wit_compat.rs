//! WIT compatibility tests for WASM tools and channels.
//!
//! These tests verify that pre-built WASM components can be compiled and
//! instantiated against the current host linker. If the WIT interface
//! changes, these tests catch any breakage in existing tools/channels.
//!
//! Prerequisites: build WASM extensions first with:
//!   ./scripts/build-wasm-extensions.sh
//!
//! The tests are skipped (not failed) when no WASM artifacts are found,
//! so `cargo test` still passes without building extensions first.
//! CI runs the build script before these tests.

#[path = "wit_compat/harness.rs"]
mod harness;

use std::path::PathBuf;

use harness::{
    ExtensionKind, compile_component, create_engine, discover_extensions,
    instantiate_channel_component, instantiate_tool_component,
};

#[test]
fn wit_compat_tool_components_compile_and_instantiate() {
    let extensions = discover_extensions();
    let engine = create_engine();

    let tool_extensions: Vec<_> = extensions
        .iter()
        .filter(|ext| ext.kind == ExtensionKind::Tool)
        .collect();

    if tool_extensions.is_empty() {
        eprintln!("SKIP: no tool extensions found in registry");
        return;
    }

    let mut found_any = false;
    let mut failures: Vec<String> = Vec::new();

    for ext in &tool_extensions {
        let wasm_path = match ironclaw::registry::artifacts::find_wasm_artifact(
            &ext.source_dir,
            &ext.crate_name,
            "release",
        ) {
            Some(p) => p,
            None => {
                eprintln!(
                    "  SKIP {}: no built WASM artifact (run ./scripts/build-wasm-extensions.sh)",
                    ext.name
                );
                continue;
            }
        };

        found_any = true;
        eprintln!("  TEST {}: {}", ext.name, wasm_path.display());

        let wasm_bytes = match std::fs::read(&wasm_path) {
            Ok(bytes) => bytes,
            Err(e) => panic!("failed to read {}: {e}", wasm_path.display()),
        };

        let component = match compile_component(&engine, &wasm_bytes) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{}: {e}", ext.name));
                continue;
            }
        };

        if let Err(e) = instantiate_tool_component(&engine, &component) {
            failures.push(format!("{}: {e}", ext.name));
        }
    }

    if !found_any {
        eprintln!("SKIP: no WASM artifacts found (build extensions first)");
        return;
    }

    assert!(
        failures.is_empty(),
        "WIT compatibility failures for tools:\n{}",
        failures.join("\n")
    );
}

#[test]
fn wit_compat_channel_components_compile_and_instantiate() {
    let extensions = discover_extensions();
    let engine = create_engine();

    let channel_extensions: Vec<_> = extensions
        .iter()
        .filter(|ext| ext.kind == ExtensionKind::Channel)
        .collect();

    if channel_extensions.is_empty() {
        eprintln!("SKIP: no channel extensions found in registry");
        return;
    }

    let mut found_any = false;
    let mut failures: Vec<String> = Vec::new();

    for ext in &channel_extensions {
        let wasm_path = match ironclaw::registry::artifacts::find_wasm_artifact(
            &ext.source_dir,
            &ext.crate_name,
            "release",
        ) {
            Some(p) => p,
            None => {
                eprintln!(
                    "  SKIP {}: no built WASM artifact (run ./scripts/build-wasm-extensions.sh)",
                    ext.name
                );
                continue;
            }
        };

        found_any = true;
        eprintln!("  TEST {}: {}", ext.name, wasm_path.display());

        let wasm_bytes = match std::fs::read(&wasm_path) {
            Ok(bytes) => bytes,
            Err(e) => panic!("failed to read {}: {e}", wasm_path.display()),
        };

        let component = match compile_component(&engine, &wasm_bytes) {
            Ok(c) => c,
            Err(e) => {
                failures.push(format!("{}: {e}", ext.name));
                continue;
            }
        };

        if let Err(e) = instantiate_channel_component(&engine, &component) {
            failures.push(format!("{}: {e}", ext.name));
        }
    }

    if !found_any {
        eprintln!("SKIP: no WASM artifacts found (build extensions first)");
        return;
    }

    assert!(
        failures.is_empty(),
        "WIT compatibility failures for channels:\n{}",
        failures.join("\n")
    );
}

#[test]
fn wit_compat_all_registry_extensions_have_source() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut missing = Vec::new();

    for dir in &["registry/tools", "registry/channels"] {
        let registry_dir = repo_root.join(dir);
        if !registry_dir.exists() {
            continue;
        }

        for entry in std::fs::read_dir(&registry_dir).expect("failed to read registry dir") {
            let entry = entry.expect("failed to read directory entry");
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let content = std::fs::read_to_string(&path).unwrap();
            let manifest: serde_json::Value = serde_json::from_str(&content).unwrap();

            let name = manifest["name"].as_str().unwrap_or("unknown");
            let source_dir = manifest["source"]["dir"].as_str();
            let crate_name = manifest["source"]["crate_name"].as_str();

            match (source_dir, crate_name) {
                (Some(d), Some(_)) => {
                    if !repo_root.join(d).exists() {
                        missing.push(format!("{name}: source dir '{d}' does not exist"));
                    }
                }
                _ => {
                    missing.push(format!("{name}: missing source.dir or source.crate_name"));
                }
            }
        }
    }

    assert!(
        missing.is_empty(),
        "Registry entries with missing sources:\n{}",
        missing.join("\n")
    );
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
        ironclaw::tools::wasm::WIT_TOOL_VERSION
    );
    let expected_channel = format!(
        "package near:agent@{};",
        ironclaw::tools::wasm::WIT_CHANNEL_VERSION
    );

    assert!(
        tool_wit.contains(&expected_tool),
        "wit/tool.wit version must match WIT_TOOL_VERSION constant ({})",
        ironclaw::tools::wasm::WIT_TOOL_VERSION
    );
    assert!(
        channel_wit.contains(&expected_channel),
        "wit/channel.wit version must match WIT_CHANNEL_VERSION constant ({})",
        ironclaw::tools::wasm::WIT_CHANNEL_VERSION
    );
}
