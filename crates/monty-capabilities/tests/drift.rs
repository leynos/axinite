//! Deterministic refinement bridge for the proved capability kernel.
//!
//! `../../verus/monty_capabilities.rs` proves the bodies between `KERNEL-BODY`
//! markers. This test requires those bodies to be byte-identical to the
//! shipped bodies in this crate's `src/lib.rs` — no whitespace normalization,
//! so a reformat is a drift too — and separately requires each crate root to
//! hold nothing but the marked bodies and their documented scaffolding. A
//! change to either copy without the other fails here, and code added outside
//! the markers fails the scaffolding check, so the verified body cannot drift
//! from production.
//!
//! The negative control is `a_mutated_body_is_rejected`: it mutates one byte of
//! an extracted body and requires the comparison to fail, proving this test can
//! fail at all.

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

/// Compare every marked body between two crates, exactly.
///
/// This is the whole bridge, and the negative control below drives it with a
/// deliberately mutated map, so the rejection path is exercised rather than
/// assumed.
fn compare_bodies(
    production: &BTreeMap<String, String>,
    proof: &BTreeMap<String, String>,
) -> Result<(), String> {
    let production_names: Vec<&String> = production.keys().collect();
    let proof_names: Vec<&String> = proof.keys().collect();
    if production_names != proof_names {
        return Err(format!(
            "production and Verus kernels must declare the same bodies, got \
             {production_names:?} and {proof_names:?}"
        ));
    }
    if production.is_empty() {
        return Err("no kernel bodies found".to_string());
    }
    for (name, production_body) in production {
        let proof_body = proof
            .get(name)
            .ok_or_else(|| format!("missing proof body for '{name}'"))?;
        if production_body != proof_body {
            return Err(format!(
                "production and proved bodies for '{name}' have drifted; the \
                 comparison is byte-exact, so reformatting either copy is drift"
            ));
        }
    }
    Ok(())
}

#[test]
fn proved_bodies_match_production() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let production = extract_bodies(&root.join("src/lib.rs"));
    let proof = extract_bodies(&root.join("../../verus/monty_capabilities.rs"));

    if let Err(message) = compare_bodies(&production, &proof) {
        panic!("{message}");
    }
}

/// The negative control: a single mutated byte must be rejected.
///
/// Without this, a comparison that silently compared nothing — or that
/// normalized away real differences — would pass and prove nothing.
#[test]
fn a_mutated_body_is_rejected() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let production = extract_bodies(&root.join("src/lib.rs"));
    let mut mutated = production.clone();
    let name = mutated
        .keys()
        .next()
        .cloned()
        .expect("at least one kernel body to mutate");
    let body = mutated.get_mut(&name).expect("the name came from this map");
    let last = body.pop().expect("a body with at least one character");
    body.push(if last == 'a' { 'b' } else { 'a' });
    let error = compare_bodies(&mutated, &production).expect_err("a mutated body must be rejected");
    assert!(
        error.contains(&name),
        "the rejection must name the drifted body: {error}"
    );
}
