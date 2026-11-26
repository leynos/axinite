//! Terminal output for `axinite registry info`.

use crate::registry::catalog::RegistryCatalog;
use crate::registry::manifest::{ArtifactSpec, AuthSummary, BundleDefinition, ExtensionManifest};

/// Print a bundle's name, description, member extensions and shared auth.
pub(super) fn print_bundle_info(catalogue: &RegistryCatalog, bundle: &BundleDefinition) {
    println!("Bundle: {}", bundle.display_name);
    if let Some(desc) = &bundle.description {
        println!("  {}", desc);
    }
    println!("\nExtensions:");
    for ext_key in &bundle.extensions {
        if let Some(m) = catalogue.get(ext_key) {
            println!("  {} - {} ({})", ext_key, m.description, m.kind);
        } else {
            println!("  {} (not found in registry)", ext_key);
        }
    }
    if let Some(shared) = &bundle.shared_auth {
        println!("\nShared auth: {}", shared);
    }
}

/// Print a single extension's details, section by section.
pub(super) fn print_extension_info(manifest: &ExtensionManifest) {
    println!("{} ({})", manifest.display_name, manifest.kind);
    println!("  Version: {}", manifest.version);
    println!("  {}", manifest.description);

    if !manifest.keywords.is_empty() {
        println!("  Keywords: {}", manifest.keywords.join(", "));
    }

    println!("\nSource:");
    println!("  Directory: {}", manifest.source.dir);
    println!("  Crate: {}", manifest.source.crate_name);
    println!("  Capabilities: {}", manifest.source.capabilities);

    if let Some(artefact) = manifest.artifacts.get("wasm32-wasip2") {
        print_artefact_info(artefact);
    }

    if let Some(auth) = &manifest.auth_summary {
        print_auth_info(auth);
    }

    if !manifest.tags.is_empty() {
        println!("\nTags: {}", manifest.tags.join(", "));
    }
}

/// Print the wasm32-wasip2 artefact's URL and digest, or why they are absent.
fn print_artefact_info(artefact: &ArtifactSpec) {
    println!("\nArtifact (wasm32-wasip2):");
    match &artefact.url {
        Some(url) => println!("  URL: {}", url),
        None => println!("  URL: (not yet published)"),
    }
    match &artefact.sha256 {
        Some(sha) => println!("  SHA256: {}", sha),
        None => println!("  SHA256: (not yet computed)"),
    }
}

/// Print the authentication summary: method, provider, secrets, sharing and setup.
fn print_auth_info(auth: &AuthSummary) {
    println!("\nAuthentication:");
    if let Some(method) = &auth.method {
        println!("  Method: {}", method);
    }
    if let Some(provider) = &auth.provider {
        println!("  Provider: {}", provider);
    }
    if !auth.secrets.is_empty() {
        println!("  Secrets: {}", auth.secrets.join(", "));
    }
    if let Some(shared) = &auth.shared_auth {
        println!("  Shared with: {}", shared);
    }
    if let Some(url) = &auth.setup_url {
        println!("  Setup: {}", url);
    }
}
