"""Contract for the inputs the CodeScene uploader step is given.

The shared CV-005 library pins the uploader's `mode` and `access-token` and
refuses the retired checksum input, but it does not look at `cli-version`,
`archive-checksum` or `project-url`. Setting either of the first two changes
which CodeScene CLI the upload installs, and this repository has not chosen to
pin one, so the step takes the three inputs it takes today and nothing more.
An undeclared input is not covered either way: GitHub ignores it with a
warning, and the library treats it as harmless.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

from _workflow_files import load
from _workflow_policy import WORKFLOW_DIR

WORKFLOW: typ.Final[str] = "coverage.yml"
UPLOADER: typ.Final[str] = "upload-codescene-coverage@"

#: The inputs the upload step passes, and no others.
ALLOWED_INPUTS: typ.Final[frozenset[str]] = frozenset(
    {"format", "mode", "access-token"}
)


def _uploads() -> list[dict[str, object]]:
    """Return every uploader step in the publisher."""
    jobs = load(WORKFLOW_DIR / WORKFLOW).get("jobs")
    assert isinstance(jobs, dict), f"{WORKFLOW} must declare a jobs mapping"
    return [
        step
        for job in jobs.values()
        if isinstance(job, dict)
        for step in job.get("steps", [])
        if isinstance(step, dict) and UPLOADER in str(step.get("uses", ""))
    ]


def test_the_uploader_takes_only_the_inputs_this_repository_chose() -> None:
    """Keep the CodeScene CLI version, checksum and project URL out of the step."""
    uploads = _uploads()
    assert len(uploads) == 1, f"expected one uploader step, found {len(uploads)}"
    inputs = uploads[0].get("with")
    assert isinstance(inputs, dict), "the uploader must declare its inputs"
    extra = set(inputs) - ALLOWED_INPUTS
    assert not extra, (
        f"the uploader passes {sorted(extra)}; `cli-version` and "
        "`archive-checksum` decide which CodeScene CLI it installs, which "
        "this repository does not pin, and a new input needs a decision here"
    )
