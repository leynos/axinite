//! Unit tests for `RepairClaims` and `ToolRepairClaim`.

use chrono::Utc;

use crate::agent::self_repair::BrokenTool;
use crate::agent::self_repair::repair_claim::RepairClaims;

// === helpers ===

fn broken_tool(name: &str) -> BrokenTool {
    BrokenTool {
        name: name.to_string(),
        failure_count: 1,
        last_error: None,
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    }
}

// === claim_tool: successful acquisition ===

#[test]
fn claim_tool_returns_some_for_unclaimed_tool() {
    let claims = RepairClaims::default();
    let tool = broken_tool("my-tool");

    let claim = claims
        .claim_tool(&tool)
        .expect("claim_tool should not error for an unclaimed tool");

    assert!(
        claim.is_some(),
        "claim_tool must return Some when the tool is not currently claimed"
    );
}

// === claim_tool: duplicate rejection ===

#[test]
fn claim_tool_returns_none_while_same_tool_is_claimed() {
    let claims = RepairClaims::default();
    let tool = broken_tool("my-tool");

    // Acquire the first claim but do not drop it yet.
    let _first_claim = claims
        .claim_tool(&tool)
        .expect("first claim should succeed")
        .expect("first claim should return Some");

    // A second attempt for the same tool must be rejected.
    let second = claims
        .claim_tool(&tool)
        .expect("claim_tool should not error for a second attempt");

    assert!(
        second.is_none(),
        "claim_tool must return None when the tool is already claimed"
    );
}

// === claim_tool: different tools do not block each other ===

#[test]
fn claim_tool_allows_concurrent_claims_for_different_tools() {
    let claims = RepairClaims::default();
    let tool_a = broken_tool("tool-alpha");
    let tool_b = broken_tool("tool-beta");

    let claim_a = claims
        .claim_tool(&tool_a)
        .expect("claim for tool-alpha should not error");
    let claim_b = claims
        .claim_tool(&tool_b)
        .expect("claim for tool-beta should not error");

    assert!(claim_a.is_some(), "claim_tool must succeed for tool-alpha");
    assert!(
        claim_b.is_some(),
        "claim_tool must succeed for tool-beta even while tool-alpha is claimed"
    );
}

// === Drop releases the claim ===

#[test]
fn drop_releases_claim_so_same_tool_can_be_claimed_again() {
    let claims = RepairClaims::default();
    let tool = broken_tool("my-tool");

    // Acquire and immediately drop the claim.
    {
        let _claim = claims
            .claim_tool(&tool)
            .expect("first claim should succeed")
            .expect("first claim should return Some");
        // `_claim` drops here.
    }

    // After the drop, the same tool must be claimable again.
    let second_claim = claims
        .claim_tool(&tool)
        .expect("claim_tool should not error after drop");

    assert!(
        second_claim.is_some(),
        "claim_tool must return Some after the previous claim was dropped"
    );
}

// === Sequential re-acquisition after concurrent use ===

#[test]
fn sequential_repair_succeeds_after_concurrent_claim_is_released() {
    let claims = RepairClaims::default();
    let tool = broken_tool("my-tool");

    // Simulate first repair: acquire, do work (no-op here), release.
    let first = claims
        .claim_tool(&tool)
        .expect("first sequential claim should not error")
        .expect("first sequential claim should return Some");
    drop(first);

    // Simulate second repair immediately after.
    let second = claims
        .claim_tool(&tool)
        .expect("second sequential claim should not error");

    assert!(
        second.is_some(),
        "sequential repair must succeed after the preceding claim is released"
    );
}

// === Poison-lock handling ===

#[test]
#[ignore = "RepairClaims does not expose a way to poison its private mutex from this module"]
fn claim_tool_returns_error_when_mutex_is_poisoned() {
    use std::sync::Arc;

    let claims = Arc::new(RepairClaims::default());
    let claims_for_thread = Arc::clone(&claims);
    let tool = broken_tool("my-tool");

    // This demonstrates the attempted public-API route: `claim_tool` releases
    // the mutex before returning `ToolRepairClaim`, so this panic does not
    // poison the mutex. Keep the ignored test as a marker for the unreachable
    // branch without adding unsafe layout access or changing production code.
    let _ = std::thread::spawn(move || {
        let tool = broken_tool("my-tool");
        let _claim = claims_for_thread
            .claim_tool(&tool)
            .expect("pre-poison claim should succeed");
        panic!("intentional panic after claiming the tool");
    })
    .join();

    match claims.claim_tool(&tool) {
        Err(crate::error::RepairError::Failed { .. }) => {}
        Err(error) => panic!("poisoned-mutex error must be RepairError::Failed, got: {error:?}"),
        Ok(_) => panic!("claim_tool must return Err when the mutex is poisoned"),
    }
}
