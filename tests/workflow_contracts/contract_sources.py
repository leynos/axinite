"""Reading the files these contracts are about, fallibly and visibly.

The readings above this module are queries: they take a directory or a
path and return lanes, profiles or binary names. Acquisition is
fallible in ways a query is not, and a tuple return type says nothing
about it. A missing directory, an unreadable file or one that is not
UTF-8 would otherwise surface as a bare ``OSError`` or a
``UnicodeDecodeError`` naming an errno and a byte offset, with nothing
saying which contract was reading what, or that the failure was in
acquisition at all.

So the filesystem calls these contracts make are wrapped here and
report a domain error carrying the path, with the cause chained. There
are three: reading a file, listing a directory, and asking whether an
entry is a directory. The third was added late, because `Path.is_dir`
looks like a question rather than a filesystem call and answers False
for an entry it could not stat, which is the same silent under-reading
`matching_entries` documents about `Path.glob`.

``SourceReadError`` keeps ``OSError`` as its base, because these are
operating-system failures and a caller already catching ``OSError``
should keep catching them.
"""

import errno
import stat
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
    """Return one directory's entries whose names match a separator-free pattern."""
    # Listed through `directory_entries`, so a directory that cannot be
    # read raises `SourceReadError` here rather than yielding nothing.
    return [
        entry for entry in directory_entries(directory) if fnmatch(entry.name, pattern)
    ]


def _is_directory(entry: "Path") -> bool:
    """Return whether an entry is a directory, refusing an answer it cannot give."""
    # `Path.is_dir` swallows every `OSError` and answers False, so an
    # entry this process cannot stat is indistinguishable from a plain
    # file. That is the same fault `matching_entries` documents about
    # `Path.glob`, one level down: a `tests` directory readable but not
    # executable lists its children and then fails to stat any of them,
    # so the nested sweep reports no compile-contract binaries and every
    # assertion over that empty set passes.
    #
    # `ENOENT` and `ENOTDIR` are answers rather than failures. A
    # dangling symlink is not a directory, and neither is a path whose
    # parent component turns out to be a file; both are skipped as
    # before. Anything else means the question was not answered, and an
    # unanswered question is reported rather than guessed.
    #
    # `Path.stat` rather than `Path.is_dir`, and not the keyword
    # `is_dir(follow_symlinks=...)`, which arrived in Python 3.13. The
    # stat call raises on every supported version, which is the whole
    # point of asking this way.
    try:
        return stat.S_ISDIR(entry.stat().st_mode)
    except OSError as error:
        if error.errno in (errno.ENOENT, errno.ENOTDIR):
            return False
        message = (
            f"{entry} could not be classified as a directory or not: {error}; "
            f"the nested sweep would otherwise skip it silently and report a "
            f"tree with nothing in it"
        )
        raise SourceReadError(message, path=entry) from error


def _nested_like(directory: "Path", head: str, tail: str) -> "list[Path]":
    """Return entries one level down whose parent directories match ``head``."""
    # Every filesystem call on this path is fallible and visible: the
    # two listings through `directory_entries`, and the directory test
    # through `_is_directory`.
    found: list[Path] = []
    for entry in _named_like(directory, head):
        if _is_directory(entry):
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
