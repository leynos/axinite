"""Helpers shared by Makefile contract tests."""

from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class _MakeTestContext:
    """Group temporary paths and executable locations for a test case."""

    tmp_path: Path
    make_executable: str
    utility_bin: Path


def shell_quote(value: str) -> str:
    """Return the POSIX single-quoted shell representation of a value.

    Parameters
    ----------
    value : str
        Literal value to represent as one shell argument.

    Returns
    -------
    str
        POSIX shell text with embedded single quotes escaped.
    """
    return "'" + value.replace("'", "'\"'\"'") + "'"
