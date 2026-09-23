"""Contracts holding CodeScene coverage publication to `main`.

Pull requests generate coverage and ratchet it against a baseline, and never
contact CodeScene. `coverage.yml` is the one publisher: its libsql-only leg
writes the ratchet baseline on a push to `main` and uploads that report to
CodeScene, guarded on the token's presence and on the ref. The readers are in
`_coverage_publication.py`; `coverage_publication_reader_test.py` drives each
clause with constructed workflows, because every assertion here is over the
repository's own files, which are correct.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _coverage_publication import (
    pull_request_faults,
    pull_request_surface,
    push_surface,
    scalars,
    upload_guard_faults,
)
from _strict_workflows import Workflows, jobs, read_workflows, steps, triggers
from _workflow_policy import WORKFLOW_DIR

#: The publisher, its job, and the pull-request coverage lane.
PUBLISHER = "coverage.yml"
PUBLISHER_JOB = "coverage"
PULL_REQUEST_LANE = "codescene-coverage.yml"
PULL_REQUEST_JOB = "coverage-check"

#: The shared actions, and the pins each may carry. a5765019 is the floor;
#: a pin is acceptable only if it is that commit or one descended from it, and
#: this list is how the contract knows which are.
UPLOADER = "leynos/shared-actions/.github/actions/upload-codescene-coverage@"
GENERATOR = "leynos/shared-actions/.github/actions/generate-coverage@"
ALLOWED_PINS: frozenset[str] = frozenset(
    {
        "a5765019912a8ab6882b12db049c7cde635f3a85",
        "dbe2e22ceaf498d85512679ccded38be9dbe7777",
    }
)

#: The check step's id and its one exact command. The expression is evaluated
#: before the shell runs, so the step binds nothing and there is no shell
#: conditional that could neutralize the write.
TOKEN_CHECK_ID = "codescene-token"
TOKEN_CHECK_COMMAND = (
    'echo "available=${{ secrets.CS_ACCESS_TOKEN != \'\' }}" >> "$GITHUB_OUTPUT"'
)

#: The publisher's concurrency group, exactly. Keyed on the ref alone: an
#: event in the key would put a dispatch and a push to `main` in different
#: groups, and the two would then race to upload and to write the baseline.
PUBLISHER_GROUP = "coverage-${{ github.ref }}"

#: The conjuncts the upload guard must contain, whole.
REF_GUARD = "github.ref == 'refs/heads/main'"
AVAILABLE_GUARD = f"steps.{TOKEN_CHECK_ID}.outputs.available == 'true'"
LEG_GUARD = "matrix.name == 'libsql-only'"

#: The token, passed to the uploader as its input and nowhere else.
TOKEN_INPUT = "${{ secrets.CS_ACCESS_TOKEN }}"

#: Inputs the withdrawn installer verification used, which no step may pass.
WITHDRAWN = ("installer-checksum", "codescene_cli_sha256", "get-codescene-sha")

#: The generator inputs that decide what a coverage run measures. The ratchet
#: compares the pull-request lane against the publisher's baseline, so these
#: must agree between the two.
SELECTION_INPUTS = (
    "features",
    "with-default-features",
    "all-features",
    "use-cargo-nextest",
)


@pytest.fixture(scope="module")
def workflows() -> Workflows:
    """Return the estate, read through the strict loader."""
    return read_workflows(WORKFLOW_DIR)


def _uses_steps(
    workflows: Workflows, prefix: str
) -> list[tuple[str, str, dict[object, object]]]:
    """Return every step whose `uses` starts with a prefix, with its location."""
    return [
        (name, job_id, step)
        for name, document in workflows.items()
        for job_id, job in jobs(document).items()
        for step in steps(job)
        if str(step.get("uses", "")).startswith(prefix)
    ]


def _publisher_steps(workflows: Workflows) -> list[dict[object, object]]:
    """Return the publisher job's steps."""
    return steps(jobs(workflows[PUBLISHER])[PUBLISHER_JOB])


def test_every_workflow_reads_strictly(workflows: Workflows) -> None:
    """No workflow declares a key twice, and the reader found the estate."""
    assert len(workflows) >= 10, f"the strict read found only {sorted(workflows)}"


def test_the_pull_request_surface_cannot_reach_codescene(workflows: Workflows) -> None:
    """Nothing a pull request can start names the token, the uploader or the host.

    The surface is asserted first: every fault check is satisfied by a
    surface that found nothing.
    """
    surface = pull_request_surface(workflows)
    expected = {PULL_REQUEST_LANE, "test.yml", "e2e.yml", "code_style.yml"}
    assert expected <= surface, f"the surface missed {sorted(expected - surface)}"
    faults = pull_request_faults(workflows)
    assert not faults, "\n".join(faults)
    assert PUBLISHER not in surface, (
        f"{PUBLISHER} must not be reachable from a pull request"
    )


def test_the_publisher_is_the_one_uploader(workflows: Workflows) -> None:
    """One upload step, in the publisher job, in upload mode, pinned at the floor."""
    found = _uses_steps(workflows, UPLOADER)
    assert [(name, job) for name, job, _ in found] == [(PUBLISHER, PUBLISHER_JOB)], (
        f"exactly one uploader, in {PUBLISHER}:{PUBLISHER_JOB}, is expected; "
        f"found {found}"
    )
    step = found[0][2]
    pin = str(step["uses"]).removeprefix(UPLOADER)
    assert pin in ALLOWED_PINS, (
        f"the uploader is pinned at {pin}, not at or above the floor"
    )
    assert step.get("with") == {
        "format": "lcov",
        "mode": "upload",
        "access-token": TOKEN_INPUT,
    }, (
        "the uploader must take the token as its input, in upload mode; it "
        f"has {step.get('with')}"
    )


def test_the_token_is_in_no_environment_on_the_publisher(workflows: Workflows) -> None:
    """The composite uploader hands its step environment to nested steps.

    So the token may appear in no `env` at any scope of the publisher: the
    workflow, the job, or any step.
    """
    document = workflows[PUBLISHER]
    job = jobs(document)[PUBLISHER_JOB]
    scopes = [document.get("env"), job.get("env")] + [
        step.get("env") for step in steps(job)
    ]
    bound = [
        scope
        for scope in scopes
        if "cs_access_token" in " ".join(scalars(scope)).casefold()
    ]
    assert not bound, f"the token is bound in an environment on the publisher: {bound}"


def test_the_token_check_is_one_exact_unconditional_command(
    workflows: Workflows,
) -> None:
    """The check step exists, precedes the upload, and cannot be neutralized."""
    publisher = _publisher_steps(workflows)
    checks = [step for step in publisher if step.get("id") == TOKEN_CHECK_ID]
    assert len(checks) == 1, f"the publisher needs one {TOKEN_CHECK_ID!r} step"
    check = checks[0]
    assert check.get("run") == TOKEN_CHECK_COMMAND, (
        f"the check must run exactly {TOKEN_CHECK_COMMAND!r}; it runs "
        f"{check.get('run')!r}"
    )
    assert "if" not in check and "env" not in check, (
        "the check step must carry no `if:` and bind nothing"
    )
    uploads = [
        step for step in publisher if str(step.get("uses", "")).startswith(UPLOADER)
    ]
    assert publisher.index(check) < publisher.index(uploads[0]), (
        "the check must run before the upload that reads its output"
    )


def test_the_upload_is_guarded_on_the_leg_the_token_and_the_ref(
    workflows: Workflows,
) -> None:
    """Every required conjunct whole, and no `||` anywhere in the guard."""
    (upload,) = [
        step
        for step in _publisher_steps(workflows)
        if str(step.get("uses", "")).startswith(UPLOADER)
    ]
    faults = upload_guard_faults(
        str(upload.get("if", "")), (LEG_GUARD, AVAILABLE_GUARD, REF_GUARD)
    )
    assert not faults, "\n".join(faults)


def test_the_publisher_runs_on_main_and_never_cancels(workflows: Workflows) -> None:
    """Push to main and dispatch only; one pending run per ref, never cancelled."""
    document = workflows[PUBLISHER]
    assert set(triggers(document)) == {"push", "workflow_dispatch"}, (
        f"{PUBLISHER} must answer only a push and a dispatch; it answers "
        f"{sorted(triggers(document))}"
    )
    assert PUBLISHER in push_surface(workflows), (
        f"{PUBLISHER} must run on a push to main"
    )
    concurrency = document.get("concurrency")
    assert isinstance(concurrency, dict), (
        f"{PUBLISHER} must declare a workflow concurrency group"
    )
    assert concurrency.get("group") == PUBLISHER_GROUP, (
        f"the group must be exactly {PUBLISHER_GROUP!r}: keyed on the ref, so a "
        "branch dispatch cannot take main's pending slot, and on nothing else, "
        "so a dispatch and a push to main queue behind one another rather "
        f"than race; it is {concurrency.get('group')!r}"
    )
    assert concurrency.get("cancel-in-progress", False) is False, (
        "a cancelled publisher abandons its upload and its baseline; "
        "cancel-in-progress must be false"
    )


def test_nothing_passes_the_withdrawn_installer_verification(
    workflows: Workflows,
) -> None:
    """No workflow names the withdrawn checksum input, variable or workflow."""
    for name, document in workflows.items():
        folded = " ".join(scalars(document)).casefold()
        named = [needle for needle in WITHDRAWN if needle in folded]
        assert not named, f"{name} still names {named}"
    assert "get-codescene-sha.yml" not in workflows, (
        "the SHA-fetching workflow must be gone"
    )


def test_one_writer_of_the_ratchet_baseline(workflows: Workflows) -> None:
    """Among push-to-main workflows, one generator ratchets: the publisher's leg."""
    surface = push_surface(workflows)
    writers = [
        (name, job_id, step.get("if"))
        for name, job_id, step in _uses_steps(workflows, GENERATOR)
        if name in surface and (step.get("with") or {}).get("with-ratchet") == "true"
    ]
    assert writers == [(PUBLISHER, PUBLISHER_JOB, LEG_GUARD)], (
        f"exactly the libsql-only leg of {PUBLISHER} may write the baseline; "
        f"found {writers}"
    )


def _generator(workflows: Workflows, name: str, job_id: str) -> dict[object, object]:
    """Return the one generate-coverage step's inputs in a job."""
    found = [
        step.get("with") or {}
        for step in steps(jobs(workflows[name])[job_id])
        if str(step.get("uses", "")).startswith(GENERATOR)
    ]
    assert len(found) == 1, f"{name}:{job_id} must run generate-coverage once"
    return _mapping(found[0])


def _mapping(value: object) -> dict[object, object]:
    """Return a mapping unchanged, or an empty one for anything else."""
    return value if isinstance(value, dict) else {}


def test_the_pull_request_lane_ratchets_without_publishing(
    workflows: Workflows,
) -> None:
    """The lane compares against the baseline and uploads no artefact."""
    inputs = _generator(workflows, PULL_REQUEST_LANE, PULL_REQUEST_JOB)
    assert inputs.get("with-ratchet") == "true", "the pull-request lane must ratchet"
    assert inputs.get("publish-artefact") == "false", (
        "the pull-request lane must not publish"
    )


def test_the_lane_and_its_baseline_measure_the_same_selection(
    workflows: Workflows,
) -> None:
    """A ratchet over different features measures the features, not the commit."""
    lane = _generator(workflows, PULL_REQUEST_LANE, PULL_REQUEST_JOB)
    baseline = _generator(workflows, PUBLISHER, PUBLISHER_JOB)
    differing = [key for key in SELECTION_INPUTS if lane.get(key) != baseline.get(key)]
    assert not differing, f"the lane and the baseline differ on {differing}"


def test_every_shared_coverage_action_is_pinned_at_the_floor(
    workflows: Workflows,
) -> None:
    """Both shared actions carry a pin at or descended from a5765019."""
    found = _uses_steps(workflows, UPLOADER) + _uses_steps(workflows, GENERATOR)
    assert len(found) >= 3, f"expected the uploader and two generators; found {found}"
    for name, job_id, step in found:
        pin = str(step["uses"]).rsplit("@", 1)[1]
        assert pin in ALLOWED_PINS, (
            f"{name}:{job_id} pins {pin}, below or outside the floor"
        )
