"""Bounded role-based properties for workflow tool installation ordering.

The reference model represents conditional steps as skippable, so a config
removal only exempts a compile when its role is unguarded. Repository workflows
are checked separately by `build_standard_workflows_test.py`.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

from hypothesis import given
from hypothesis import strategies as st

from _build_standard_workflows import install_violations

COMPILE_TARGETS = {"property-compile"}


@st.composite
def _jobs(draw: st.DrawFn) -> tuple[dict[str, object], list[tuple[str, bool]], bool]:
    """Generate Linux and non-Linux steps with meaningful workflow roles."""
    host = draw(st.sampled_from(("linux", "non-linux")))
    kinds = draw(
        st.lists(
            st.sampled_from(
                ("compile-cargo", "compile-make", "install", "remove", "other")
            ),
            max_size=18,
        )
    )
    guards = draw(st.lists(st.booleans(), min_size=len(kinds), max_size=len(kinds)))
    roles = list(zip(kinds, guards, strict=True))
    steps: list[dict[str, object]] = []
    for index, (kind, guarded) in enumerate(roles):
        run = {
            "compile-cargo": "cargo test",
            "compile-make": "make property-compile",
            "install": "make install-build-tools",
            "remove": "rm .cargo/config.toml",
            "other": "echo setup",
        }[kind]
        step: dict[str, object] = {"name": f"step-{index}", "run": run}
        if guarded:
            step["if"] = "false"
        steps.append(step)
    job: dict[str, object] = {
        "runs-on": "ubuntu-latest" if host == "linux" else "windows-latest",
        "steps": steps,
    }
    job_failure_is_soft = draw(st.booleans())
    if job_failure_is_soft:
        job["continue-on-error"] = True
    return job, roles, host != "non-linux"


def _first_compile_position(roles: list[tuple[str, bool]]) -> int | None:
    """Find the first generated compile step without inspecting YAML."""
    return next(
        (
            index
            for index, (kind, _) in enumerate(roles)
            if kind in ("compile-cargo", "compile-make")
        ),
        None,
    )


def _removes_standard_before_compile(
    roles: list[tuple[str, bool]], first_compile: int
) -> bool:
    """Recognize an unguarded removal before compilation."""
    return any(
        kind == "remove" and not guarded and index < first_compile
        for index, (kind, guarded) in enumerate(roles)
    )


def _installs_unconditionally_before_compile(
    roles: list[tuple[str, bool]], first_compile: int
) -> bool:
    """Require at least one unguarded install before the first compile."""
    early_installs = [
        guarded
        for index, (kind, guarded) in enumerate(roles)
        if kind == "install" and index < first_compile
    ]
    return bool(early_installs) and not any(early_installs)


def _reference_is_valid(
    job: dict[str, object], roles: list[tuple[str, bool]], is_linux: bool
) -> bool:
    """Model the policy from generated roles, without workflow parsing helpers."""
    if not is_linux:
        return True
    first_compile = _first_compile_position(roles)
    if first_compile is None:
        return True
    if _removes_standard_before_compile(roles, first_compile):
        return True
    return (
        _installs_unconditionally_before_compile(roles, first_compile)
        and job.get("continue-on-error") is not True
    )


@given(_jobs())
def test_install_violations_match_the_workflow_reference_model(
    generated: tuple[dict[str, object], list[tuple[str, bool]], bool],
) -> None:
    """Install must precede compilation and be unconditional and hard-failing."""
    job, roles, is_linux = generated
    actual = bool(install_violations("property.yml:job", job, COMPILE_TARGETS))
    expected_violation = not _reference_is_valid(job, roles, is_linux)
    assert actual == expected_violation, (
        f"roles={roles!r}, linux={is_linux}, job={job!r}, "
        f"expected_violation={expected_violation}, actual={actual}"
    )
