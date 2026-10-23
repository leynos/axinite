"""The single entry point that judges every workflow.

`judge_all` composes the provisioning judge (`_build_standard_workflows`) and the
exclusion judges (`_build_standard_exclusions`) over parsed workflows, so the
tests and their mutations share one definition of "the workflows conform".

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

from _build_standard_exclusions import (
    coverage_violations,
    release_violations,
    retired_violations,
    whitaker_violations,
)
from _build_standard_workflows import install_violations

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterable, Mapping


def judge_all(
    documents: Mapping[str, Mapping[str, object]],
    compiling_targets: Iterable[str],
    *,
    texts: Mapping[str, str],
    dist_generated: str = "release.yml",
) -> list[str]:
    """Judge every workflow; the single entry the tests and mutations share.

    Parameters
    ----------
    documents
        Parsed workflows by file name.
    compiling_targets
        Make targets that reach Cargo.
    texts
        Workflow source text by file name, for the retired-mechanism scan.
    dist_generated
        The dist-generated release workflow, exempt from the install rule
        because its matrix comes from a previous job's output.

    Returns
    -------
    list of str
        Every violation found.
    """
    problems: list[str] = []
    for name, document in documents.items():
        problems.extend(retired_violations(name, texts.get(name, "")))
        jobs = document.get("jobs")
        for job_id, job in jobs.items() if isinstance(jobs, dict) else []:
            if not isinstance(job, dict):
                continue
            label = f"{name}:{job_id}"
            if name != dist_generated:
                problems.extend(install_violations(label, job, compiling_targets))
            problems.extend(coverage_violations(label, document, job))
            problems.extend(whitaker_violations(label, job))
            if name.startswith("release"):
                problems.extend(release_violations(label, document, job))
    return problems
