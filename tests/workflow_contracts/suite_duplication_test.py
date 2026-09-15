"""Contracts against running one test suite twice for the same commit.

Nothing fails when a suite runs twice. Both runs pass, both report, and the
only evidence is the bill and the wait. Axinite paid that twice over: on a
pull request `test.yml`'s libsql-only leg compiled and ran the workspace under
exactly the flags `codescene-coverage.yml` was already running it under, and
on a push to `main` all three `test.yml` legs repeated what `coverage.yml` had
just done with instrumentation. The GitHub tool crate's suite was worse: it
rode along inside `make test`, so it ran once per leg, three times a trigger.
A fourth was hidden in a name: the leg called `all-features` passed three
features that were already members of `default`, so it was the default leg
under another name.

The rule enforced here is therefore about work rather than about job names: on
a trigger a developer waits for, no two lanes may run the same suite over the
same feature set under the same nextest profile. The profile is part of the
identity because it selects which tests run at all: the default profile drops
the trybuild compile contracts, so a default-profile lane does not stand in
for a `ci` one, however identical the flags.

Only `pull_request` and `push` are in scope. Cron work runs GitHub-hosted,
where this repository pays nothing and nobody is waiting, so duplication there
is not what this contract is for.

The reading these assertions rest on lives in `_suite_reader.py` and
`_suite_targets.py`, and `suite_reader_test.py` holds its unit tests.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _suite_reader import ESTATE, PAID_EVENTS, duplicates_in, suite_runs_for
from _workflow_policy import REPOSITORY_ROOT, jobs_of, matrix_legs
from _suite_targets import (
    DEFAULT_PROFILE,
    GITHUB_TOOL_MANIFEST,
    GITHUB_TOOL_RECIPE,
    GITHUB_TOOL_SCOPE,
    MAKEFILE_PROFILE_DEFAULT,
    WASM_PREREQUISITE,
    WORKSPACE,
    WORKSPACE_RECIPE,
    make_rule,
)


def test_the_scan_finds_the_suite_at_all() -> None:
    """Guard against a selector that silently matches nothing.

    Every assertion below is satisfied by finding no runs, so the scan's own
    reach is asserted first. Two is the floor a working estate cannot go
    under: a pull request runs the suite, and so does a push.
    """
    for event in PAID_EVENTS:
        assert len(suite_runs_for(ESTATE, event)) >= 2, (
            f"no workspace suite runs were found for {event}; the contract "
            "below would pass with the suite deleted"
        )


def test_make_test_is_both_halves_and_nothing_else() -> None:
    """`make test` must remain exactly the two lanes CI runs separately.

    A developer runs one target; CI runs the halves on different lanes and
    different triggers. If the whole stopped being the sum of the parts, the
    local command and the gate would test different things, and this module
    would still read `make test` as covering both.
    """
    prerequisites, recipe = make_rule("test")
    assert set(prerequisites) == {"test-workspace", "test-github-tool"}, (
        f"`make test` builds {prerequisites}, which is no longer the two "
        "lanes CI runs; a developer and the gate would test different things"
    )
    assert not recipe, (
        f"`make test` has grown a recipe of its own ({recipe}), so it runs "
        "work neither lane runs and nothing in CI covers it"
    )


def test_the_make_targets_still_run_what_this_module_reads_them_as() -> None:
    """Tie each target name in a workflow to what the Makefile makes it do.

    A workflow step says `make test-workspace`. If that target stopped running
    the workspace suite, or started running the GitHub tool crate again, every
    judgement here about what a lane executes would be wrong while still
    reading correctly.
    """
    _, workspace = make_rule("test-workspace")
    suite_line = next(
        (
            index
            for index, line in enumerate(workspace)
            if line.startswith(WORKSPACE_RECIPE)
        ),
        None,
    )
    assert suite_line is not None, (
        f"`make test-workspace` no longer runs {WORKSPACE_RECIPE!r}, so the "
        "workflow steps that call it do not run what this contract reads "
        f"them as running. Its recipe is {workspace}."
    )
    # The order is part of the target, not a detail of it. The metadata and
    # schema tests load the artefact the WASM build produces, so a recipe
    # that ran the suite first would test the previous build on a warm tree
    # and fail outright on a clean checkout.
    wasm_line = next(
        (
            index
            for index, line in enumerate(workspace)
            if line.startswith(WASM_PREREQUISITE)
        ),
        None,
    )
    assert wasm_line is not None and wasm_line < suite_line, (
        f"`make test-workspace` does not run {WASM_PREREQUISITE!r} before its "
        "suite, so the tests that load the WASM artefact read whatever the "
        f"last build left behind. Its recipe is {workspace}."
    )
    assert not any("--manifest-path" in line for line in workspace), (
        "`make test-workspace` builds or tests an out-of-workspace crate, "
        "which is the duplication this split removed: the GitHub tool crate "
        "has its own lane, and running it here runs it once per leg again"
    )
    _, tool = make_rule("test-github-tool")
    assert tool == (GITHUB_TOOL_RECIPE,), (
        f"`make test-github-tool` runs {tool}, not {(GITHUB_TOOL_RECIPE,)}; "
        "the scope and the empty feature set this module gives it both come "
        "from that one command"
    )
    makefile = (REPOSITORY_ROOT / "Makefile").read_text(encoding="utf-8")
    assert MAKEFILE_PROFILE_DEFAULT in makefile, (
        f"the Makefile no longer falls back to the {DEFAULT_PROFILE!r} nextest "
        "profile, so a step that names no profile does not run the tests this "
        "module reads it as running"
    )
    assert f"GITHUB_TOOL_MANIFEST := {GITHUB_TOOL_MANIFEST}\n" in makefile, (
        f"the GitHub tool manifest is no longer {GITHUB_TOOL_MANIFEST}, so "
        "its lane and a Cargo command naming the same crate would be read as "
        "two different scopes and never compared"
    )


#: The legs `test.yml`'s `tests` job resolves to, per event. The matrix is an
#: expression, so a typo in either arm silently changes what runs and the
#: duplication contract below would report the result as merely fewer runs.
#: Naming both arms here means the leg list is asserted rather than inferred.
#:
#: There is no all-features leg. There was one in name: it passed three
#: features that are already members of `default` and no
#: `--no-default-features`, so it resolved to the default leg exactly.
REVIEWED_TEST_LEGS: dict[str, tuple[tuple[str, str], ...]] = {
    "pull_request": (("default", ""),),
    "push": (
        ("default", ""),
        ("libsql-only", "--no-default-features --features libsql"),
    ),
    "schedule": (
        ("default", ""),
        ("libsql-only", "--no-default-features --features libsql"),
    ),
}


@pytest.mark.parametrize("event", sorted(REVIEWED_TEST_LEGS))
def test_the_tests_matrix_resolves_to_the_reviewed_legs(event: str) -> None:
    """Assert the leg names and flags each event produces, both arms.

    `matrix_legs` resolves the expression the way GitHub does, so this reads
    what the job runs rather than what the expression looks like. The `push`
    entry is the arm a schedule also takes; the job stands down on a push, and
    the leg list is still what it would run, which is what the mutation-proof
    for the guard needs to stay meaningful.
    """
    job = next(
        job for job in jobs_of("test.yml", ESTATE["test.yml"]) if job.job_id == "tests"
    )
    resolved = tuple(
        (leg.get("name", ""), leg.get("flags", "")) for leg in matrix_legs(job, event)
    )
    assert resolved == REVIEWED_TEST_LEGS[event], (
        f"on {event} the tests matrix resolves to {resolved}, not "
        f"{REVIEWED_TEST_LEGS[event]}"
    )


#: Every suite this repository has, and the scope each covers. The workspace
#: leaves two crates out, and `--workspace` cannot reach either, so each needs
#: a lane of its own or it is not tested at all.
EXPECTED_SCOPES: frozenset[str] = frozenset(
    {WORKSPACE, GITHUB_TOOL_SCOPE, "crate:channels-src/telegram/Cargo.toml"}
)


@pytest.mark.parametrize("event", PAID_EVENTS)
def test_every_suite_still_runs_on_every_paid_trigger(event: str) -> None:
    """The other half of de-duplication: nothing may go missing instead.

    Removing a duplicate lane and removing the only lane look identical in a
    diff and identical in a green run. This names the suites, so a lane
    deleted rather than de-duplicated fails here, and a suite added without a
    lane on one trigger fails here too.
    """
    found = {run.scope for run in suite_runs_for(ESTATE, event)}
    assert found == EXPECTED_SCOPES, (
        f"on {event} the suites that run are {sorted(found)}, not "
        f"{sorted(EXPECTED_SCOPES)}. A suite with no lane on a trigger is not "
        "tested there, whatever the gate reports."
    )


#: The profile that runs every test the suite has. Anything narrower leaves
#: the trybuild compile contracts unexecuted.
FULL_PROFILE = "ci"


@pytest.mark.parametrize("event", PAID_EVENTS)
def test_every_trigger_runs_the_workspace_suite_in_full(event: str) -> None:
    """A lane that replaces another must not be narrower than it was.

    De-duplication removes the second run of a suite, so whichever run is left
    has to be the whole of it. The default nextest profile drops the trybuild
    compile contracts, and a lane running it looks in every other respect like
    the lane it replaced.
    """
    profiles = {
        run.profile for run in suite_runs_for(ESTATE, event) if run.scope == WORKSPACE
    }
    assert profiles == {FULL_PROFILE}, (
        f"on {event} the workspace suite runs under {sorted(profiles)}, not "
        f"only {FULL_PROFILE!r}. A lane on a narrower profile runs fewer "
        "tests than the lane it stands in for, and nothing else on this "
        "trigger makes up the difference."
    )


@pytest.mark.parametrize("event", PAID_EVENTS)
def test_no_trigger_runs_the_same_suite_twice(event: str) -> None:
    """The contract itself: one feature selection, one run, per trigger."""
    duplicated = duplicates_in(suite_runs_for(ESTATE, event))
    assert not duplicated, "\n".join(
        f"on {event}, {scope} under {sorted(features) or 'no features'} and "
        f"the {profile} profile is run by " + ", ".join(str(run) for run in runs)
        for (scope, profile, features), runs in duplicated.items()
    )
