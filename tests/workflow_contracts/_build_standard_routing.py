"""Judging emitted Cargo and Whitaker invocations against the routing matrix.

`_build_standard_make` evaluates the Makefile and parses what it emits; this
module decides whether each parsed `Invocation` is routed correctly: a
development build keeps the standard, a release build and Whitaker do not take
it, and the build-tools check precedes the first development compile.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import subprocess
import typing as typ

from _build_standard import (
    CRANELIFT_FLAG,
    FAST_FLAGS,
    MOLD_FLAG,
    PLATFORMS,
    THREADS_FLAG,
    effective_rustflags,
)
from _build_standard_make import PREFLIGHT, Invocation

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping


def expand_assignment(value: str, inherited: str | None) -> str:
    """Evaluate a ``RUSTFLAGS`` assignment as the recipe shell would.

    Parameters
    ----------
    value
        The assignment's value with its quotes removed, as ``shlex`` returns it.
    inherited
        The caller's ``RUSTFLAGS``, or ``None`` when unset.

    Returns
    -------
    str
        The value the child process receives.
    """
    environment = {"PATH": "/usr/bin:/bin"}
    if inherited is not None:
        environment["RUSTFLAGS"] = inherited
    script = f'printf "%s" "{value}"'
    completed = subprocess.run(  # noqa: S603 S607 - repository-controlled text
        ["sh", "-c", script],
        env=environment,
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout


def is_release_build(invocation: Invocation) -> bool:
    """Report whether an invocation builds a release profile, or installs.

    ``--profile`` is Cargo's profile selector on a Cargo verb but nextest's
    own profile on ``nextest run``, which says nothing about the build.
    """
    if invocation.verb == "install":
        return True
    arguments = invocation.arguments
    if invocation.tool == "nextest":
        return "--release" in arguments or "--cargo-profile" in arguments
    if "--profile" in arguments:
        selected = arguments[arguments.index("--profile") + 1]
        return selected not in ("dev", "test")
    return "--release" in arguments or "-r" in arguments


def target_triples(invocation: Invocation) -> list[str]:
    """Return the platforms an invocation's effective flags are judged on.

    An explicit ``--target`` fixes the platform; otherwise the build runs on
    its host, which the standard covers for every supported platform.
    """
    arguments = invocation.arguments
    if "--target" in arguments:
        value = arguments[arguments.index("--target") + 1]
        return [value]
    return [triple for triple in PLATFORMS if PLATFORMS[triple] != "wasi"]


def make_violations(
    invocation: Invocation, config: Mapping[str, typ.Any], *, label: str
) -> list[str]:
    """Judge one emitted invocation against the routing matrix.

    Parameters
    ----------
    invocation
        The parsed command.
    config
        The parsed Cargo configuration.
    label
        Where the command came from, for messages.

    Returns
    -------
    list of str
        One message per violation.
    """
    problems: list[str] = []
    if invocation.verb in ("fmt", "clean"):
        return problems
    # A development build is judged with no caller RUSTFLAGS: a caller who
    # exports one has chosen to replace the standard, for bare Cargo as much as
    # for Make. A build that assigns or excludes the flags itself is judged
    # against a caller flag too, because that is what its assignment handles.
    judged = (None,) if _develops(invocation) else (None, "-Ctarget-cpu=native")
    for inherited in judged:
        assigned = (
            expand_assignment(invocation.assignments["RUSTFLAGS"], inherited)
            if "RUSTFLAGS" in invocation.assignments
            else inherited
        )
        for triple in target_triples(invocation):
            effective = effective_rustflags(config, triple, assigned)
            problems.extend(
                _judge_flags(invocation, triple, effective, inherited, label)
            )
    return problems


def _develops(invocation: Invocation) -> bool:
    """Report whether an invocation is a plain development build."""
    return invocation.tool != "whitaker" and not is_release_build(invocation)


def _whitaker_flags(
    invocation: Invocation, effective: list[str], present: set[str], where: str
) -> list[str]:
    """Judge Whitaker: none of the standard, and a literal `-D warnings` boundary."""
    problems = [f"{where}: Whitaker inherits {sorted(present)}"] if present else []
    if CRANELIFT_FLAG in effective:
        problems.append(f"{where}: Whitaker inherits the Cranelift flag")
    boundary = invocation.assignments.get("RUSTFLAGS")
    if boundary is None:
        problems.append(f"{where}: Whitaker has no RUSTFLAGS boundary")
    elif "-D warnings" not in boundary:
        problems.append(f"{where}: Whitaker's boundary dropped -D warnings")
    return problems


def _development_flags(
    triple: str, effective: list[str], present: set[str], where: str
) -> list[str]:
    """Judge a development build: the frontend everywhere, mold on Linux only."""
    expected = {THREADS_FLAG} | ({MOLD_FLAG} if PLATFORMS[triple] == "linux" else set())
    if expected <= present and (MOLD_FLAG in present) == (MOLD_FLAG in expected):
        return []
    return [f"{where}: development build lost the standard, got {effective}"]


def _judge_flags(
    invocation: Invocation,
    triple: str,
    effective: list[str],
    inherited: str | None,
    label: str,
) -> list[str]:
    """Judge one invocation's effective flags on one platform."""
    where = f"{label} on {triple} (caller RUSTFLAGS {inherited!r})"
    present = {flag for flag in FAST_FLAGS if flag in effective}
    if invocation.tool == "whitaker":
        return _whitaker_flags(invocation, effective, present, where)
    if is_release_build(invocation):
        return (
            [f"{where}: a release build inherits {sorted(present)}"] if present else []
        )
    return _development_flags(triple, effective, present, where)


def _is_development_compile(invocation: Invocation) -> bool:
    """Report whether an invocation compiles under the development standard."""
    return (
        invocation.verb not in ("fmt", "clean")
        and not is_release_build(invocation)
        and invocation.tool != "whitaker"
    )


def preflight_violations(lines: list[str], invocations: list[Invocation]) -> list[str]:
    """Require the build-tools check before any development Cargo line.

    Parameters
    ----------
    lines
        Emitted lines for a target.
    invocations
        The invocations parsed from them.

    Returns
    -------
    list of str
        A message when a development compile precedes, or lacks, the check.
    """
    developing = [inv for inv in invocations if _is_development_compile(inv)]
    if not developing:
        return []
    checks = [index for index, line in enumerate(lines) if line.strip() == PREFLIGHT]
    if not checks:
        return [f"no {PREFLIGHT} before the development compile"]
    first = min(inv.index for inv in developing)
    if min(checks) > first:
        return [f"{PREFLIGHT} runs after the first development compile"]
    return []
