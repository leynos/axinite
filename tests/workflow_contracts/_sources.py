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

import errno
import stat
import tomllib
import typing as typ

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    import collections.abc as cabc
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


def _classified_as(
    path: Path, predicate: cabc.Callable[[int], bool], description: str
) -> bool:
    """Return whether a path is of the kind `predicate` tests for.

    `Path.is_dir` and `Path.is_file` look like questions rather than
    filesystem calls, and they answer `False` for an entry they could not
    `stat`. That makes an unreadable path indistinguishable from one that is
    simply absent, which is the same silent under-reading `contract_sources`
    documents about `Path.glob`: a contract asking whether a manifest exists
    would be told it does not, and would report a configuration fault about a
    file it never managed to look at.

    The `stat` is taken directly rather than through
    `Path.is_dir(follow_symlinks=...)`, which only arrived in Python 3.13.
    The call raises on every supported version, which is the point of asking
    this way.

    Parameters
    ----------
    path
        The entry to classify.
    predicate
        A `stat` mode test, such as `stat.S_ISDIR`.
    description
        What the predicate tests for, named in the failure so a reader learns
        which question went unanswered.

    Returns
    -------
    bool
        Whether the entry is of that kind. False for an entry that is absent,
        or whose parent is not a directory.

    Raises
    ------
    SourceError
        If the entry exists but cannot be classified.
    """
    try:
        return predicate(path.stat().st_mode)
    except OSError as error:
        if error.errno in (errno.ENOENT, errno.ENOTDIR):
            return False
        raise SourceError(
            path,
            f"cannot be classified as {description} or not "
            f"({error.strerror or error})",
        ) from error


def is_directory(path: Path) -> bool:
    """Return whether a path is a directory, or say it could not be told.

    The directory test is a filesystem call, not a question, and it answers
    `False` for a path it could not `stat`. A reading that took that answer
    would report a declared directory as missing when it exists and simply
    could not be looked at, which sends a reader after a rename that never
    happened.

    Parameters
    ----------
    path
        The entry to classify.

    Returns
    -------
    bool
        Whether the entry is a directory. False for an entry that is absent
        or whose parent is not a directory.

    Raises
    ------
    SourceError
        If the entry exists but cannot be classified.
    """
    return _classified_as(path, stat.S_ISDIR, "a directory")


def is_regular_file(path: Path) -> bool:
    """Return whether a path is a regular file, or say it could not be told.

    The same hazard as :func:`is_directory`, and the one a manifest check
    would otherwise hit: `Path.is_file` answers `False` for a file that is
    there but could not be `stat`, so a contract would report a manifest
    missing from a directory that holds it.

    Parameters
    ----------
    path
        The entry to classify.

    Returns
    -------
    bool
        Whether the entry is a regular file. False for an entry that is
        absent or whose parent is not a directory.

    Raises
    ------
    SourceError
        If the entry exists but cannot be classified.
    """
    return _classified_as(path, stat.S_ISREG, "a regular file")
