"""Property tests for workflow tool installation ordering.

A small generated workflow model independently decides whether a Linux job's
build-tools install is unconditional and precedes every compile.

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
            st.sampled_from(("compile-cargo", "compile-make", "install", "remove", "other")),
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
            step["if"] = "always()"
        steps.append(step)
    job: dict[str, object] = {
        "runs-on": "ubuntu-latest" if host == "linux" else "windows-latest",
        "steps": steps,
    }
    job_failure_is_soft = draw(st.booleans())
    if job_failure_is_soft:
        job["continue-on-error"] = True
    return job, roles, host != "non-linux"


def _reference_is_valid(
    job: dict[str, object], roles: list[tuple[str, bool]], is_linux: bool
) -> bool:
    """Model the policy from generated roles, without workflow parsing helpers."""
    if not is_linux:
        return True
    compile_positions = [
        index
        for index, (kind, _) in enumerate(roles)
        if kind in ("compile-cargo", "compile-make")
    ]
    if not compile_positions:
        return True
    first_compile = compile_positions[0]
    removed_before_compile = any(
        kind == "remove" and index < first_compile
        for index, (kind, _) in enumerate(roles)
    )
    if removed_before_compile:
        return True
    early_installs = [
        guarded
        for index, (kind, guarded) in enumerate(roles)
        if kind == "install" and index < first_compile
    ]
    return (
        bool(early_installs)
        and not any(early_installs)
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
