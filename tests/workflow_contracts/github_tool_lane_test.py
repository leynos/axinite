"""Contract for the credential the GitHub tool lane's checkout keeps.

`actions/checkout` writes the job token into the checkout's Git
configuration unless told not to, and repository code runs after it: the
resource sampler and `make test-github-tool`. Nothing in
`github-tool-tests` performs an authenticated Git operation, so the lane
checks out with `persist-credentials: false`. Removing that line changes
nothing a test run can observe, which is why it is asserted here.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

from _estate import read_workflow
from _workflow_files import jobs_of
from _workflow_policy import WORKFLOW_DIR

#: The workflow and job this contract reads.
WORKFLOW = "test.yml"
JOB = "github-tool-tests"

#: The action whose credential persistence is at issue.
CHECKOUT_ACTION = "actions/checkout@"


def _checkout_steps() -> list[dict[str, object]]:
    """Return the `actions/checkout` steps of the GitHub tool lane."""
    document = read_workflow(WORKFLOW_DIR / WORKFLOW)
    lanes = [job for job in jobs_of(WORKFLOW, document) if job.job_id == JOB]
    assert len(lanes) == 1, f"{WORKFLOW} should declare one {JOB} job"
    return [
        step
        for step in lanes[0].steps
        if str(step.get("uses", "")).startswith(CHECKOUT_ACTION)
    ]


def test_the_github_tool_lane_does_not_persist_its_token() -> None:
    """The lane's one checkout sets `persist-credentials` to exactly `false`.

    Missing, `true`, or the string `"false"` all fail: the first two leave
    the token in the Git configuration, and a quoted value is a string the
    action happens to accept today rather than the boolean the contract
    means. The setting is read from this job's own checkout, so moving it
    to another job fails too.
    """
    checkouts = _checkout_steps()
    assert len(checkouts) == 1, (
        f"{JOB} should check out once; found {len(checkouts)} checkout steps"
    )
    options = checkouts[0].get("with")
    declared = options.get("persist-credentials") if isinstance(options, dict) else None
    assert declared is False, (
        f"{JOB}'s checkout must set `persist-credentials: false`; it sets "
        f"{declared!r}, which leaves the job token in the checkout's Git "
        "configuration for the repository code that runs after it"
    )
