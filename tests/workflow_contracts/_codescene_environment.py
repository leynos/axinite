"""Readers for the rule that only the uploading job enters `codescene`.

The CodeScene token is moving into the `codescene` deployment environment,
whose branch policy admits `main` alone. That keeps the secret away from any
job GitHub would not deploy from `main`, but only if the environment sits on
the right jobs. Three clauses: every job that calls the uploader declares it,
no other job declares it, and no job in a workflow a pull request can start
declares it, whether or not that job uploads.

Pure in the parsed estate, like `_coverage_publication.py`, so
`codescene_environment_test.py` can break each clause in a constructed
workflow. The repository's own workflows are correct and cannot show that a
clause refuses anything.
"""

from __future__ import annotations

import typing as typ

from _coverage_publication import pull_request_surface
from _strict_workflows import Workflows, jobs, steps

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping

#: The environment holding the CodeScene token.
ENVIRONMENT: typ.Final[str] = "codescene"

#: The uploader action, as a `uses:` prefix.
UPLOADER: typ.Final[str] = (
    "leynos/shared-actions/.github/actions/upload-codescene-coverage@"
)


def environment_name(job: Mapping[object, object]) -> str | None:
    """Return the environment a job declares, from either accepted form.

    GitHub takes `environment:` as a name or as a mapping with `name` (and an
    optional `url`), so both must read the same.

    Examples
    --------
    >>> environment_name({"environment": "codescene"})
    'codescene'
    >>> environment_name({"environment": {"name": "codescene", "url": "x"}})
    'codescene'
    >>> environment_name({}) is None
    True
    """
    match job.get("environment"):
        case str() as name:
            return name
        case {"name": str() as name}:
            return name
        case _:
            return None


def _uploads(job: Mapping[object, object]) -> bool:
    """Report whether any step in a job calls the uploader."""
    return any(str(step.get("uses", "")).startswith(UPLOADER) for step in steps(job))


#: What a job's upload and its declaration mean when they disagree, keyed by
#: (uploads, declares).
_MISMATCH: typ.Final[dict[tuple[bool, bool], str]] = {
    (True, False): f"uploads but does not declare `{ENVIRONMENT}`",
    (False, True): f"declares `{ENVIRONMENT}` but uploads nothing",
}


def _job_faults(where: str, job: Mapping[object, object], *, on_surface: bool) -> list[str]:
    """Return the placement faults of one job, identified as `where`."""
    declares = environment_name(job) == ENVIRONMENT
    mismatch = _MISMATCH.get((_uploads(job), declares))
    faults = [] if mismatch is None else [f"{where} {mismatch}"]
    if declares and on_surface:
        faults.append(
            f"{where} is reachable from a pull request and declares `{ENVIRONMENT}`"
        )
    return faults


def _placed_jobs(
    workflows: Workflows,
) -> list[tuple[str, Mapping[object, object], bool]]:
    """Return each job as its location, its body, and whether a PR reaches it."""
    surface = pull_request_surface(workflows)
    return [
        (f"{name}:{job_id}", job, name in surface)
        for name, document in sorted(workflows.items())
        for job_id, job in jobs(document).items()
    ]


def environment_faults(workflows: Workflows) -> list[str]:
    """Return every departure from the `codescene` environment placement.

    Parameters
    ----------
    workflows
        The parsed estate.

    Returns
    -------
    list of str
        One sentence per fault, naming the job; empty when the placement
        holds. An estate with no uploader at all is a fault, since every
        other clause would then pass by finding nothing.
    """
    placed = _placed_jobs(workflows)
    faults = [
        fault
        for where, job, on_surface in placed
        for fault in _job_faults(where, job, on_surface=on_surface)
    ]
    if not any(_uploads(job) for _, job, _ in placed):
        faults.append("no job calls the CodeScene uploader")
    return faults
