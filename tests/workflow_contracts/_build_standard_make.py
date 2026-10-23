"""Judging how the Makefile routes every Cargo and Whitaker invocation.

``make --dry-run <target> CARGO=probe-cargo WHITAKER=probe-whitaker`` prints the
lines a recipe would execute. Every line that mentions a probe executable is
parsed into an `Invocation`, none may be skipped, and each is judged against the
routing matrix in `make_violations`. The effective-flag model it uses
(`effective_rustflags`) lives in `_build_standard`, beside the configuration it
reads.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import shlex
import subprocess
import typing as typ

from _build_standard import (
    CRANELIFT_FLAG,
    FAST_FLAGS,
    MOLD_FLAG,
    PLATFORMS,
    REPOSITORY_ROOT,
    THREADS_FLAG,
    effective_rustflags,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterable, Mapping
    from pathlib import Path

# -- Make ---------------------------------------------------------------------

#: Subcommands whose Cargo line is a development-profile compile.
CARGO_VERBS = {"build", "check", "clippy", "test", "nextest", "install", "fmt", "clean"}
PREFLIGHT = "./scripts/check-build-tools.sh"


class Invocation(typ.NamedTuple):
    """One Cargo (or Whitaker) command a Make recipe emits.

    Attributes
    ----------
    tool
        ``cargo``, ``nextest`` or ``whitaker``.
    verb
        The Cargo subcommand (``build``, ``test``...), or ``run`` for nextest.
    assignments
        Environment assignments preceding the command, as written.
    arguments
        Everything after the subcommand.
    index
        Position among all emitted lines, to compare with the preflight.
    """

    tool: str
    verb: str
    assignments: dict[str, str]
    arguments: tuple[str, ...]
    index: int


def dry_run(
    target: str,
    *,
    cargo: str = "probe-cargo",
    extra: Mapping[str, str] | None = None,
    root: Path = REPOSITORY_ROOT,
    home: Path | None = None,
) -> list[str]:
    """Return the lines ``make --dry-run <target>`` emits.

    Parameters
    ----------
    target
        Make target.
    cargo
        Value for ``CARGO``; ``probe-cargo`` makes every Cargo line obvious.
    extra
        Additional ``VAR=value`` pairs for the command line.
    root
        Directory holding the Makefile.
    home
        ``HOME`` for the run, so ``BUILD_TOOLS_PREFIX`` stays out of the real one.

    Returns
    -------
    list of str
        The emitted command lines.
    """
    command = [
        "make",
        "--no-print-directory",
        "-n",
        target,
        f"CARGO={cargo}",
        "WHITAKER=probe-whitaker",
    ]
    command.extend(f"{key}={value}" for key, value in (extra or {}).items())
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(home or "/nonexistent-home")}
    completed = subprocess.run(  # noqa: S603 - fixed argv, repository Makefile
        command, cwd=root, env=environment, check=True, capture_output=True, text=True
    )
    return completed.stdout.splitlines()


def parse_invocations(lines: Iterable[str]) -> tuple[list[Invocation], list[str]]:
    """Return the Cargo and Whitaker invocations in emitted lines.

    Every line that mentions the probe executables must parse; an unparsed
    one is returned separately so a caller can refuse to skip it silently.

    Parameters
    ----------
    lines
        Output of `dry_run`.

    Returns
    -------
    tuple
        The parsed invocations, and the lines that mention a probe but did
        not parse.
    """
    found: list[Invocation] = []
    unparsed: list[str] = []
    for index, line in enumerate(lines):
        if "probe-cargo" not in line and "probe-whitaker" not in line:
            continue
        try:
            words = shlex.split(line)
        except ValueError:
            unparsed.append(line)
            continue
        assignments: dict[str, str] = {}
        while words and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", words[0]):
            name, _, value = words.pop(0).partition("=")
            assignments[name] = value
        parsed = _command_of(words, assignments, index)
        if parsed is None:
            unparsed.append(line)
        else:
            found.append(parsed)
    return found, unparsed


def _command_of(
    words: list[str], assignments: dict[str, str], index: int
) -> Invocation | None:
    """Return the invocation a command's words describe, or ``None``."""
    if not words:
        return None
    if words[0] == "probe-whitaker":
        return Invocation("whitaker", "dylint", assignments, tuple(words[1:]), index)
    if words[0] != "probe-cargo":
        return None
    rest = words[1:]
    if rest and rest[0] == "nextest":
        return Invocation(
            "nextest",
            rest[1] if len(rest) > 1 else "",
            assignments,
            tuple(rest[2:]),
            index,
        )
    if not rest or rest[0] not in CARGO_VERBS:
        return None
    return Invocation("cargo", rest[0], assignments, tuple(rest[1:]), index)


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
        problems = [f"{where}: Whitaker inherits {sorted(present)}"] if present else []
        if CRANELIFT_FLAG in effective:
            problems.append(f"{where}: Whitaker inherits the Cranelift flag")
        if "RUSTFLAGS" not in invocation.assignments:
            problems.append(f"{where}: Whitaker has no RUSTFLAGS boundary")
        elif "-D warnings" not in invocation.assignments["RUSTFLAGS"]:
            problems.append(f"{where}: Whitaker's boundary dropped -D warnings")
        return problems
    if is_release_build(invocation):
        return (
            [f"{where}: a release build inherits {sorted(present)}"] if present else []
        )
    expected = {THREADS_FLAG} | ({MOLD_FLAG} if PLATFORMS[triple] == "linux" else set())
    problems = []
    if not expected <= present or (MOLD_FLAG in present) != (MOLD_FLAG in expected):
        problems.append(
            f"{where}: development build lost the standard, got {effective}"
        )
    return problems


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
    developing = [
        inv
        for inv in invocations
        if inv.verb not in ("fmt", "clean")
        and not is_release_build(inv)
        and inv.tool != "whitaker"
    ]
    if not developing:
        return []
    checks = [index for index, line in enumerate(lines) if line.strip() == PREFLIGHT]
    if not checks:
        return [f"no {PREFLIGHT} before the development compile"]
    first = min(inv.index for inv in developing)
    if min(checks) > first:
        return [f"{PREFLIGHT} runs after the first development compile"]
    return []
