//! Deterministic refinement bridge for the proved capability kernel.
//!
//! `../../verus/monty_capabilities.rs` proves the bodies between `KERNEL-BODY`
//! markers. This test requires those bodies to be byte-identical to the
//! shipped bodies in this crate's `src/lib.rs`. A change to
//! either copy without the other fails here, so the verified body cannot drift
//! from production.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn extract_bodies(path: &Path) -> BTreeMap<String, String> {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => panic!("failed to read {}: {error}", path.display()),
    };
    let mut bodies = BTreeMap::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("// KERNEL-BODY: ").map(str::trim) {
            assert!(
                current.is_none(),
                "nested KERNEL-BODY marker in {}",
                path.display()
            );
            current = Some((name.to_owned(), Vec::new()));
            continue;
        }
        if let Some(name) = trimmed.strip_prefix("// END KERNEL-BODY: ").map(str::trim) {
            let Some((open, lines)) = current.take() else {
                panic!("END marker without BEGIN marker in {}", path.display());
            };
            assert_eq!(
                open,
                name,
                "mismatched KERNEL-BODY markers in {}",
                path.display()
            );
            let body = lines.join("\n");
            assert!(
                bodies.insert(open.clone(), body).is_none(),
                "duplicate KERNEL-BODY '{open}' in {}",
                path.display()
            );
            continue;
        }
        if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        }
    }
    assert!(
        current.is_none(),
        "unterminated KERNEL-BODY marker in {}",
        path.display()
    );
    bodies
}

fn normalize(body: &str) -> String {
    let lines: Vec<&str> = body
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect();
    lines.join("\n")
}

#[test]
fn proved_bodies_match_production() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let production = extract_bodies(&root.join("src/lib.rs"));
    let proof = extract_bodies(&root.join("../../verus/monty_capabilities.rs"));

    assert_eq!(
        production.keys().collect::<Vec<_>>(),
        proof.keys().collect::<Vec<_>>(),
        "production and Verus kernels must declare the same bodies"
    );
    assert!(!production.is_empty(), "no kernel bodies found");

    for (name, production_body) in &production {
        let proof_body = proof
            .get(name)
            .unwrap_or_else(|| panic!("missing proof body for '{name}'"));
        assert_eq!(
            normalize(production_body),
            normalize(proof_body),
            "production and proved bodies for '{name}' have drifted"
        );
    }
}
