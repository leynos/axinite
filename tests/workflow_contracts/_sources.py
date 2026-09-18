"""Reading the files these contracts judge, with the failure named.

Everything else in this directory is a pure function of text that has already
been read. This module is the one place that touches the filesystem, so a
manifest that is missing, a Makefile that cannot be decoded, or a workflow
that is not YAML raises one exception type that names the path, rather than an
`OSError`, a `TOMLDecodeError` or a `YAMLError` escaping from inside something
whose signature says it answers a question about a feature list.

The distinction matters because of where these failures land. A read at import
time turns a bad file into a collection error, and a contract directory that
fails to collect reports no failures at all, which reads exactly like a clean
run. Reading at the boundary, from a fixture, makes the same fault fail the
contracts that depend on it, by name and with the path in the message.
"""

from __future__ import annotations

import tomllib
import typing as typ

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Mapping
    from pathlib import Path


class SourceError(Exception):
    """A file these contracts read could not be read or parsed.

    Attributes
    ----------
    path
        The file that failed, so the message names it rather than leaving the
        reader to guess which of several sources was at fault.
    """

    def __init__(self, path: Path, reason: str) -> None:
        self.path = path
        super().__init__(f"{path}: {reason}")


def read_text(path: Path) -> str:
    """Return a file's text, or raise `SourceError` naming it.

    Parameters
    ----------
    path
        The file to read.

    Returns
    -------
    str
        Its contents, decoded as UTF-8.

    Raises
    ------
    SourceError
        If the file is missing, unreadable, or not valid UTF-8.
    """
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise SourceError(
            path, f"cannot be read ({error.strerror or error})"
        ) from error
    except UnicodeDecodeError as error:
        raise SourceError(path, f"is not valid UTF-8 ({error})") from error


def read_toml(path: Path) -> Mapping[str, object]:
    """Return a parsed TOML document, or raise `SourceError` naming it.

    Parameters
    ----------
    path
        The TOML file to read, such as a Cargo manifest.

    Returns
    -------
    Mapping
        The parsed document.

    Raises
    ------
    SourceError
        If the file cannot be read, or is not valid TOML.
    """
    text = read_text(path)
    try:
        return tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise SourceError(path, f"is not valid TOML ({error})") from error
