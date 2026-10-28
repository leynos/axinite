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
from _build_standard_workflows import install_violations, iter_jobs

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterable, Mapping


def _job_violations(
    label: str,
    document: Mapping[str, object],
    job: Mapping[str, object],
    install_targets: list[str] | None,
) -> list[str]:
    """Judge one job against every contract that applies to it.

    ``install_targets`` is ``None`` for the dist-generated workflow, which is
    exempt from the install rule.
    """
    problems = (
        []
        if install_targets is None
        else install_violations(label, job, install_targets)
    )
    problems += coverage_violations(label, document, job)
    problems += whitaker_violations(label, job)
    if label.startswith("release"):
        problems += release_violations(label, document, job)
    return problems


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
    problems = [
        problem
        for name in documents
        for problem in retired_violations(name, texts.get(name, ""))
    ]
    targets = list(compiling_targets)
    for name, job_id, job in iter_jobs(documents):
        install_targets = None if name == dist_generated else targets
        problems.extend(
            _job_violations(f"{name}:{job_id}", documents[name], job, install_targets)
        )
    return problems
