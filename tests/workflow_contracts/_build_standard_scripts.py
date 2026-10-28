"""Judging the shell scripts that run Cargo outside Make and the workflows.

A script is a route like any other: ``scripts/coverage.sh`` measures coverage
and so must not take the development standard, and the scripts that build
release artefacts must not either. Both exclusions use the
mechanism the rest of the repository uses: ``RUSTFLAGS`` assigned, even to an
inherited empty value, displaces the configuration's ``rustflags`` tables.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import typing as typ

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping

RELEASE_BUILD = re.compile(
    r"\bcargo\s+(?:\+\S+\s+)?(?:component\s+)?(?:build|install)\b[^\n]*--release\b"
)
LLVM_COV = re.compile(r"\bcargo\s+(?:\+\S+\s+)?llvm-cov\s+(?!--version\b)")
RUSTFLAGS_PREFIX = re.compile(r"(?:^|[\s!(])RUSTFLAGS=")


def _code_lines(text: str) -> list[tuple[int, str]]:
    """Return ``(line number, text)`` for each non-comment line."""
    return [
        (number, line)
        for number, line in enumerate(text.splitlines(), start=1)
        if line.strip() and not line.lstrip().startswith("#")
    ]


def _release_violations(name: str, code: list[tuple[int, str]]) -> list[str]:
    """Report release builds that do not assign RUSTFLAGS on their own line."""
    return [
        f"{name}:{number}: a release build inherits the development "
        "standard; assign RUSTFLAGS on the command"
        for number, line in code
        if RELEASE_BUILD.search(line) and not RUSTFLAGS_PREFIX.search(line)
    ]


def _coverage_violations(name: str, code: list[tuple[int, str]]) -> list[str]:
    """Report a coverage script that runs before exporting RUSTFLAGS."""
    first = next((n for n, line in code if LLVM_COV.search(line)), None)
    if first is None:
        return []
    before = "\n".join(line for n, line in code if n < first)
    if re.search(r"^\s*export\s+RUSTFLAGS=", before, re.MULTILINE):
        return []
    return [f"{name}: coverage runs without exporting RUSTFLAGS first"]


def script_violations(name: str, text: str) -> list[str]:
    """Judge one script's release and coverage invocations.

    Parameters
    ----------
    name
        Script path, for messages.
    text
        The script's source.

    Returns
    -------
    list of str
        One message per release build that does not assign ``RUSTFLAGS`` on its
        own line, and per coverage script that does not export ``RUSTFLAGS``
        before its first ``cargo llvm-cov``.
    """
    code = _code_lines(text)
    return _release_violations(name, code) + _coverage_violations(name, code)


def all_script_violations(scripts: Mapping[str, str]) -> list[str]:
    """Judge every script in a mapping of path to source."""
    return [
        problem
        for name, text in scripts.items()
        for problem in script_violations(name, text)
    ]
