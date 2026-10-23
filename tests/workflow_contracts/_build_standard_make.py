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
    REPOSITORY_ROOT,
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


def _parse_line(index: int, line: str) -> Invocation | None:
    """Parse one emitted line as an invocation, or return ``None``."""
    try:
        words = shlex.split(line)
    except ValueError:
        return None
    assignments: dict[str, str] = {}
    while words and _ASSIGNMENT.match(words[0]):
        name, _, value = words.pop(0).partition("=")
        assignments[name] = value
    return _command_of(words, assignments, index)


_ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")


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
    mentioning = [
        (index, line)
        for index, line in enumerate(lines)
        if "probe-cargo" in line or "probe-whitaker" in line
    ]
    parsed = [(line, _parse_line(index, line)) for index, line in mentioning]
    found = [inv for _, inv in parsed if inv is not None]
    unparsed = [line for line, inv in parsed if inv is None]
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
