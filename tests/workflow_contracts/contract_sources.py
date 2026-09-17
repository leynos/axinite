"""Reading the files these contracts are about, fallibly and visibly.

The readings above this module are queries: they take a directory or a
path and return lanes, profiles or binary names. Acquisition is
fallible in ways a query is not, and a tuple return type says nothing
about it. A missing directory, an unreadable file or one that is not
UTF-8 would otherwise surface as a bare ``OSError`` or a
``UnicodeDecodeError`` naming an errno and a byte offset, with nothing
saying which contract was reading what, or that the failure was in
acquisition at all.

So the two filesystem calls these contracts make are wrapped here and
report a domain error carrying the path, with the cause chained.
``SourceReadError`` keeps ``OSError`` as its base, because these are
operating-system failures and a caller already catching ``OSError``
should keep catching them.
"""

import typing as typ
from fnmatch import fnmatch

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path


class SourceReadError(OSError):
    """Raised when a file or directory these contracts read cannot be.

    Attributes
    ----------
    path
        The file or directory the fault is about.
    """

    def __init__(self, message: str, *, path: "Path") -> None:
        """Record the message and the path it is about.

        Parameters
        ----------
        message
            What went wrong, for a person reading the failure.
        path
            The file or directory that could not be read.
        """
        super().__init__(message)
        self.path = path


def read_source(path: "Path") -> str:
    """Return one file's text, or say which file could not be read.

    Parameters
    ----------
    path
        The file to read.

    Returns
    -------
    str
        The file's contents, decoded as UTF-8.

    Raises
    ------
    SourceReadError
        If the file cannot be read or is not UTF-8.
    """
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        message = f"{path} could not be read: {error}"
        raise SourceReadError(message, path=path) from error
    except UnicodeDecodeError as error:
        message = (
            f"{path} is not UTF-8, so this contract cannot read what it says: {error}"
        )
        raise SourceReadError(message, path=path) from error


def directory_entries(directory: "Path") -> "list[Path]":
    """Return a directory's entries, or say which directory is missing.

    A directory that cannot be listed is the dangerous case rather than
    a loud one: the sweeps above take whatever this returns, so an empty
    result reads as a tree with no workflows and every assertion over it
    passes over nothing.

    Parameters
    ----------
    directory
        The directory to list.

    Returns
    -------
    list of Path
        Its entries, in whatever order the filesystem gives them.

    Raises
    ------
    SourceReadError
        If the directory cannot be listed.
    """
    try:
        return list(directory.iterdir())
    except OSError as error:
        message = f"{directory} could not be listed: {error}"
        raise SourceReadError(message, path=directory) from error


def _named_like(directory: "Path", pattern: str) -> "list[Path]":
    """Return one directory's entries whose names match a pattern.

    The listing goes through :func:`directory_entries`, so a directory
    that cannot be read raises here rather than yielding nothing.

    Parameters
    ----------
    directory
        The directory to list.
    pattern
        A glob pattern with no separator, matched against each name.

    Returns
    -------
    list of Path
        The matching entries, in the order the filesystem gave them.

    Raises
    ------
    SourceReadError
        If the directory cannot be listed.
    """
    return [
        entry for entry in directory_entries(directory) if fnmatch(entry.name, pattern)
    ]


def _nested_like(directory: "Path", head: str, tail: str) -> "list[Path]":
    """Return entries one level down whose parents match ``head``.

    Parameters
    ----------
    directory
        The directory to walk.
    head
        The pattern each subdirectory's name must match.
    tail
        The pattern each entry inside one must match.

    Returns
    -------
    list of Path
        The matching entries.

    Raises
    ------
    SourceReadError
        If any directory involved cannot be listed.
    """
    found: list[Path] = []
    for entry in _named_like(directory, head):
        if entry.is_dir():
            found.extend(_named_like(entry, tail))
    return found


def matching_entries(directory: "Path", pattern: str) -> "list[Path]":
    """Return a directory's matching entries, sorted, or say which failed.

    The same hazard as :func:`directory_entries` and a sharper form of
    it, twice over.

    ``Path.glob`` does not raise on a directory that is absent or is not
    a directory at all: it yields nothing, exactly as a directory with
    no matches does. Worse, from Python 3.13 it also suppresses the
    errors raised while scanning, so an existing directory that cannot
    be read passes ``is_dir`` and still yields nothing. A pre-check
    therefore closes only half the hole, and wrapping the call closes
    none of it: there is no exception left to catch.

    So the directories are enumerated rather than globbed. Every listing
    goes through :func:`directory_entries`, which propagates what
    ``iterdir`` raises, and the names are matched afterwards. An
    unreadable directory now fails loudly at the listing instead of
    reporting a tree with no sources, which is the answer that would
    have let compile-contract discovery certify an empty set.

    The pattern is matched rather than interpreted. One separator is
    supported, which is what this repository's sweeps use; anything
    deeper is refused rather than silently under-matched, because a
    reader that cannot walk a pattern must not report on it.

    Sorted here rather than at each call site, because two sweeps
    disagreeing about order is a defect nobody would look for.

    Parameters
    ----------
    directory
        The directory to search.
    pattern
        A glob pattern relative to the directory, with at most one
        ``/`` separator.

    Returns
    -------
    list of Path
        The matching entries, in sorted order.

    Raises
    ------
    SourceReadError
        If any directory involved cannot be listed, or the pattern
        nests deeper than this reader walks.
    """
    head, separator, tail = pattern.partition("/")
    if not separator:
        return sorted(_named_like(directory, head))
    if "/" in tail:
        message = (
            f"the pattern {pattern!r} nests deeper than this reader walks, so "
            f"it cannot say what {directory} holds"
        )
        raise SourceReadError(message, path=directory)
    return sorted(_nested_like(directory, head, tail))
