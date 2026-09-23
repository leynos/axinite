"""Helpers shared by Makefile contract tests."""


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
