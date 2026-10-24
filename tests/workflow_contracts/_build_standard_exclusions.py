"""Judging how the workflows exclude the build standard where it must not apply.

Coverage, release and Whitaker are the builds that opt out. Each opts out with
``RUSTFLAGS``, which displaces the configuration's rustflags tables. The judges here take a parsed job (and its workflow document for
the environment layers) and return messages. They share the step model of
`_build_standard_workflows`.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ

from _build_standard import FAST_FLAGS, split_flags
from _build_standard_workflows import (
    GENERATE_COVERAGE,
    INSTALL_COMMAND,
    INSTALL_WHITAKER,
    Step,
    steps_of,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping

#: Things the standard replaced. Any of them in a workflow is a second source.
RETIRED = (
    (re.compile(r"rui314/setup-mold"), "an unpinned mold installer"),
    (
        re.compile(r"dtolnay/rust-toolchain"),
        "a toolchain step that ignores rust-toolchain.toml",
    ),
    (
        re.compile(r"CARGO_TARGET_[A-Z0-9_]+_(?:LINKER|RUSTFLAGS)"),
        "a second source of linker flags",
    ),
    (
        re.compile(r"apt-get\s+install[^\n]*\bmold\b"),
        "a distribution mold that drifts from the pin",
    ),
)


def _merged_env(
    document: Mapping[str, object], job: Mapping[str, object], step: Step
) -> dict[str, object]:
    """Return the environment a step sees: workflow, then job, then step."""
    merged: dict[str, object] = {}
    for layer in (document.get("env"), job.get("env"), step.env):
        if isinstance(layer, dict):
            merged.update(layer)
    return merged


def _is_coverage(step: Step) -> bool:
    """Report whether a step measures coverage."""
    return step.uses.startswith(GENERATE_COVERAGE) or bool(
        re.search(r"\bcargo\s+llvm-cov\s+(?!--version\b)", step.run)
    )


def _rustflags_violation(
    label: str, env: Mapping[str, object], where: str
) -> list[str]:
    """Judge that an environment displaces the rustflags tables without the standard.

    ``RUSTFLAGS`` must be present, so the configuration's rustflags tables are
    displaced, and carry none of the standard's flags.
    """
    if "RUSTFLAGS" not in env:
        return [f"{label}: RUSTFLAGS is unset for {where}, so the rustflags apply"]
    flags = split_flags(str(env["RUSTFLAGS"]))
    leaked = [flag for flag in FAST_FLAGS if flag in flags]
    return [f"{label}: {where} carries {leaked}"] if leaked else []


def coverage_violations(
    label: str, document: Mapping[str, object], job: Mapping[str, object]
) -> list[str]:
    """Judge that coverage steps exclude the development standard."""
    coverage = [step for step in steps_of(job) if _is_coverage(step)]
    return [
        problem
        for step in coverage
        for problem in _rustflags_violation(
            label, _merged_env(document, job, step), f"coverage step {step.name!r}"
        )
    ]


_RELEASE_BUILD = re.compile(r"\bdist\s+build\b|\bcargo\s|release-plz/action")


def release_violations(
    label: str, document: Mapping[str, object], job: Mapping[str, object]
) -> list[str]:
    """Judge that release and packaging jobs exclude the development standard."""
    sequence = steps_of(job)
    problems = [
        f"{label}: a release job installs the development build tools"
        for step in sequence
        if INSTALL_COMMAND.search(step.run)
    ]
    builds = any(_RELEASE_BUILD.search(step.run + step.uses) for step in sequence)
    if not builds:
        return problems
    env = _merged_env(document, job, Step("", "", "", False, {}, {}))
    return problems + _rustflags_violation(label, env, "the release")


_WHITAKER_GATE = re.compile(r"\bmake\s+lint-whitaker\b")
_WHITAKER_ANY = re.compile(r"lint-whitaker|whitaker")


def _whitaker_indices(sequence: list[Step]) -> tuple[list[int], list[int]]:
    """Return the indices of the install steps and of the gate steps."""
    installs = [
        i for i, s in enumerate(sequence) if s.uses.startswith(INSTALL_WHITAKER)
    ]
    gates = [i for i, s in enumerate(sequence) if _WHITAKER_GATE.search(s.run)]
    return installs, gates


def _flag_injections(label: str, sequence: list[Step]) -> list[str]:
    """Report steps that put build flags in the environment of a Whitaker run."""
    return [
        f"{label}: {step.name!r} injects build flags into Whitaker"
        for step in sequence
        if "RUSTFLAGS" in step.env and _WHITAKER_ANY.search(step.run)
    ]


def whitaker_violations(label: str, job: Mapping[str, object]) -> list[str]:
    """Judge the Whitaker job: install through the action, then the gate."""
    sequence = steps_of(job)
    installs, gates = _whitaker_indices(sequence)
    if not installs:
        gate = [f"{label}: runs make lint-whitaker without the install-whitaker action"]
        return gate if gates else []
    pinned = any("suite-version" in sequence[i].inputs for i in installs)
    ordered = not gates or min(installs) < min(gates)
    problems = []
    if pinned:
        problems.append(f"{label}: install-whitaker pins the rolling suite")
    if not ordered:
        problems.append(f"{label}: make lint-whitaker runs before install-whitaker")
    return problems + _flag_injections(label, sequence)


def retired_violations(name: str, text: str) -> list[str]:
    """Report any mechanism the standard retired, found in a workflow's text."""
    return [
        f"{name}: still uses {why} ({pattern.pattern})"
        for pattern, why in RETIRED
        if pattern.search(text)
    ]
