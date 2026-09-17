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


def matching_entries(directory: "Path", pattern: str) -> "list[Path]":
    """Return a directory's matching entries, sorted, or say which failed.

    The same hazard as :func:`directory_entries` and a sharper form of
    it. ``Path.glob`` does not raise on a directory that is absent or is
    not a directory at all: it yields nothing, exactly as a directory
    with no matches does. So the two cases are indistinguishable at the
    call site, the caller reads a tree with no sources, and every
    assertion over the result passes over an empty set while reporting
    success. The directory is therefore checked before the search rather
    than the search being wrapped, because wrapping it catches nothing.

    Sorted here rather than at each call site, because two sweeps
    disagreeing about order is a defect nobody would look for.

    Parameters
    ----------
    directory
        The directory to search.
    pattern
        A glob pattern, relative to the directory.

    Returns
    -------
    list of Path
        The matching entries, in sorted order.

    Raises
    ------
    SourceReadError
        If the directory cannot be searched.
    """
    if not directory.is_dir():
        message = (
            f"{directory} is not a directory, so the search for {pattern!r} "
            f"would return nothing and read as a tree with no matches"
        )
        raise SourceReadError(message, path=directory)
    try:
        return sorted(directory.glob(pattern))
    except OSError as error:
        message = f"{directory} could not be searched for {pattern!r}: {error}"
        raise SourceReadError(message, path=directory) from error
