//! Unit tests for `RepairClaims` and `ToolRepairClaim`.

use chrono::Utc;
use rstest::{fixture, rstest};

use crate::agent::self_repair::BrokenTool;
use crate::agent::self_repair::repair_claim::RepairClaims;

// === helpers ===

#[fixture]
fn claims() -> RepairClaims {
    RepairClaims::default()
}

#[fixture]
fn tool() -> BrokenTool {
    broken_tool("my-tool")
}

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

enum ClaimScenario {
    UnclaimedTool,
    DuplicateClaimWhileHeld,
    ConcurrentDifferentTools,
    DropReleasesClaim,
    SequentialReacquireAfterRelease,
}

#[rstest]
#[case::unclaimed_tool(ClaimScenario::UnclaimedTool)]
#[case::duplicate_claim_while_held(ClaimScenario::DuplicateClaimWhileHeld)]
#[case::concurrent_different_tools(ClaimScenario::ConcurrentDifferentTools)]
#[case::drop_releases_claim(ClaimScenario::DropReleasesClaim)]
#[case::sequential_reacquire_after_release(ClaimScenario::SequentialReacquireAfterRelease)]
fn claim_tool_respects_claim_lifecycle(
    claims: RepairClaims,
    tool: BrokenTool,
    #[case] scenario: ClaimScenario,
) {
    match scenario {
        ClaimScenario::UnclaimedTool => {
            let claim = claims
                .claim_tool(&tool)
                .expect("claim_tool should not error for an unclaimed tool");

            assert!(
                claim.is_some(),
                "claim_tool must return Some when the tool is not currently claimed"
            );
        }
        ClaimScenario::DuplicateClaimWhileHeld => {
            let _first_claim = claims
                .claim_tool(&tool)
                .expect("first claim should succeed")
                .expect("first claim should return Some");

            let second = claims
                .claim_tool(&tool)
                .expect("claim_tool should not error for a second attempt");

            assert!(
                second.is_none(),
                "claim_tool must return None when the tool is already claimed"
            );
        }
        ClaimScenario::ConcurrentDifferentTools => {
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
        ClaimScenario::DropReleasesClaim => {
            {
                let _claim = claims
                    .claim_tool(&tool)
                    .expect("first claim should succeed")
                    .expect("first claim should return Some");
            }

            let second_claim = claims
                .claim_tool(&tool)
                .expect("claim_tool should not error after drop");

            assert!(
                second_claim.is_some(),
                "claim_tool must return Some after the previous claim was dropped"
            );
        }
        ClaimScenario::SequentialReacquireAfterRelease => {
            let first = claims
                .claim_tool(&tool)
                .expect("first sequential claim should not error")
                .expect("first sequential claim should return Some");
            drop(first);

            let second = claims
                .claim_tool(&tool)
                .expect("second sequential claim should not error");

            assert!(
                second.is_some(),
                "sequential repair must succeed after the preceding claim is released"
            );
        }
    }
}

// === Poison-lock handling ===

#[test]
#[ignore = "RepairClaims does not expose a way to poison its private mutex from this module"]
fn claim_tool_returns_error_when_mutex_is_poisoned() {
    // `RepairClaims::claim_tool` releases its internal mutex before returning
    // `ToolRepairClaim`. A spawned thread that panics after `claim_tool`
    // returns therefore panics after the lock guard has been dropped, so it
    // cannot poison the mutex through the public API.
}
