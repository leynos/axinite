"""The short utility jobs that run on Ubicloud because the hosted pool queues.

A job that neither builds nor tests the product belongs on a GitHub-hosted
runner, except where the hosted pool makes a developer wait for it. On 25
September 2026 the user ruled that such a job runs on the smallest Ubicloud
shape instead: `ubicloud-standard-2-arm` when it does not depend on the CPU
architecture, `ubicloud-standard-2` when it does. Contention is the only
reason to move one, so each entry below carries the wait that justifies it,
read from the jobs API over the workflow's last ten completed runs. The
scheduled `audit` job waited one or two seconds and stays hosted.

`runner_placement_test.py` admits exactly these jobs to Ubicloud without a
build or test step, and requires each to hold the smallest shape named here.
`runner_sizing_test.py` exempts them from the resource sampler, whose purpose
is to decide a resize, because a job pinned to the smallest shape has no
smaller one to move to. See ADR 013.
"""

from __future__ import annotations

import typing as typ

#: The smallest shape for an architecture-independent utility job.
SMALLEST_ARM: typ.Final[str] = "ubicloud-standard-2-arm"

#: The smallest shape for a utility job that needs x86-64.
SMALLEST_X86: typ.Final[str] = "ubicloud-standard-2"

#: Each utility job moved for contention, by workflow and job ID, with the
#: shape it must hold and the measurement that moved it.
UTILITY_JOBS: typ.Final[dict[tuple[str, str], tuple[str, str]]] = {
    ("pr-label-classify.yml", "classify"): (
        SMALLEST_ARM,
        "13-20 s of bash and `gh`; waited up to 2,098 s (median 316 s) for a "
        "hosted runner",
    ),
    ("pr-label-scope.yml", "scope"): (
        SMALLEST_ARM,
        "4-7 s of `actions/labeler`; waited up to 1,495 s for a hosted runner",
    ),
    ("regression-test-check.yml", "regression-test"): (
        SMALLEST_ARM,
        "6-8 s of git, grep and awk; waited up to 734 s for a hosted runner",
    ),
}


def is_utility_job(workflow: str, job_id: str) -> bool:
    """Report whether a job is one of the named utility jobs."""
    return (workflow, job_id) in UTILITY_JOBS
