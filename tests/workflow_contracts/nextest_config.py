"""Parsing `.config/nextest.toml` into the profiles nextest reads.

Separated from ``timeout_budgets`` so the parsing and the arithmetic
stay legible apart, and so neither module outgrows the 400-line limit
``AGENTS.md`` sets.

The duration grammar lives in ``nextest_durations`` and the error
types in ``nextest_errors``; both are re-exported here, because a
reader of a budget wants one import rather than three.

The configuration is parsed with ``tomllib`` rather than matched as
text. A text match finds a key inside a comment, inside a ``filter``
string, or in a table nextest never consults, and reports a budget the
runner does not use.
"""

import re
import tomllib
import typing as typ

from nextest_durations import seconds
from nextest_errors import NextestConfigurationError, UnboundedTestError


class Profile(typ.NamedTuple):
    """One nextest profile, as the runner reads it.

    Attributes
    ----------
    name
        The profile's name.
    own
        The ``[profile.<name>]`` table itself, without its overrides.
        The base allowance and an override's are different claims: an
        override bounds the tests its filter matches and the profile's
        own bounds the rest, so a reading across both would let the base
        allowance be deleted unnoticed.
    overrides
        The profile's ``[[overrides]]`` entries, in file order.
    inherited
        ``[[profile.default.overrides]]``, in file order, for a profile
        other than ``default``. nextest consults the default profile's
        overrides for whichever profile is selected, after that
        profile's own, so a test can be governed by an override the
        selected profile never declares. Empty for ``default`` itself,
        whose own overrides are already in :attr:`overrides`.
    """

    name: str
    own: dict[str, object]
    overrides: tuple[dict[str, object], ...]
    inherited: tuple[dict[str, object], ...] = ()

    def tables(self) -> tuple[dict[str, object], ...]:
        """Return every table the profile reads a budget from.

        Returns
        -------
        tuple of dict
            The profile's own table, then each override, then each
            override inherited from ``default``.
        """
        return tuple(table for _, table in self.sources())

    def sources(self) -> tuple[tuple[str, dict[str, object]], ...]:
        """Return every table with the dotted path that declares it.

        A budget inherited from ``[[profile.default.overrides]]`` is
        reported against ``profile.default``, where it is written, so a
        failure sends the reader to the line that has to change rather
        than to the profile that inherits it.

        Returns
        -------
        tuple of (str, dict)
            The declaring path and the table, own first, then this
            profile's overrides, then the inherited ones.
        """
        own = f"profile.{self.name}"
        return (
            (own, self.own),
            *((own, table) for table in self.overrides),
            *(("profile.default", table) for table in self.inherited),
        )


def _table(value: object) -> dict[str, object]:
    """Return a parsed value as a table, or an empty one.

    Parameters
    ----------
    value
        Any value ``tomllib`` produced.

    Returns
    -------
    dict of str to object
        The table, or an empty one when the value is not a table.
    """
    match value:
        case dict():
            return dict(value)
        case _:
            return {}


def _entries(value: object) -> list[object]:
    """Return a parsed value as a list, or an empty one.

    Parameters
    ----------
    value
        Any value ``tomllib`` produced.

    Returns
    -------
    list of object
        The list, or an empty one when the value is not a list.
    """
    match value:
        case list():
            return list(value)
        case _:
            return []


def profiles(config_text: str) -> dict[str, Profile]:
    """Return each profile the configuration declares, keyed by name.

    Parameters
    ----------
    config_text
        A nextest configuration file's text.

    Returns
    -------
    dict of str to Profile
        Profile name to its table, its overrides, and the default
        profile's overrides that nextest consults for it.

    Raises
    ------
    NextestConfigurationError
        If the text is not valid TOML.
    """
    try:
        parsed = tomllib.loads(config_text)
    except tomllib.TOMLDecodeError as error:
        message = f"the nextest configuration is not valid TOML: {error}"
        raise NextestConfigurationError(message) from error
    declared = {
        str(name): _declared_profile(str(name), raw)
        for name, raw in _table(parsed.get("profile")).items()
    }
    return _with_default_overrides(declared)


def _slow_timeout(table: dict[str, object]) -> object:
    """Return one table's ``slow-timeout``, or None when it sets none.

    Parameters
    ----------
    table
        A profile's own table or one of its overrides.

    Returns
    -------
    object
        The value as parsed, or None.
    """
    return table.get("slow-timeout")


def _budget_of(path: str, value: object) -> float:
    """Return the per-test budget one ``slow-timeout`` declares.

    Parameters
    ----------
    path
        The dotted path of the declaring table, for the message.
    value
        The parsed value, a table or a bare duration.

    Returns
    -------
    float
        The budget in seconds.

    Raises
    ------
    UnboundedTestError
        If the value names no ``terminate-after``, in either spelling.
    NextestConfigurationError
        If the value is a table with no ``period``, or is neither a
        table nor a duration.
    """
    match value:
        case str():
            message = (
                f'{path}.slow-timeout = "{value}" sets a warning period with '
                f"no terminate-after, so nextest reports the test as slow and "
                f"never stops it"
            )
            raise UnboundedTestError(message)
        case dict():
            pass
        case _:
            message = f"{path}.slow-timeout is neither a table nor a duration"
            raise NextestConfigurationError(message)
    period = value.get("period")
    if not isinstance(period, str):
        message = f"{path}.slow-timeout names no period: {value!r}"
        raise NextestConfigurationError(message)
    multiplier = value.get("terminate-after")
    if multiplier is None:
        message = (
            f"{path}.slow-timeout sets no terminate-after, so nextest marks "
            f"the test slow and lets it run on; there is no per-test tier to "
            f"compare against"
        )
        raise UnboundedTestError(message)
    return seconds(period) * float(str(multiplier))


def _declared_profile(name: str, raw: object) -> Profile:
    """Return one ``[profile.<name>]`` table as a profile.

    Parameters
    ----------
    name
        The profile's name.
    raw
        The parsed value of its table.

    Returns
    -------
    Profile
        The profile, with nothing inherited yet.
    """
    table = _table(raw)
    return Profile(
        name=name,
        own={key: value for key, value in table.items() if key != "overrides"},
        overrides=tuple(_table(entry) for entry in _entries(table.get("overrides"))),
    )


def _with_default_overrides(declared: dict[str, Profile]) -> dict[str, Profile]:
    """Return each profile carrying the overrides it inherits.

    nextest consults ``[[profile.default.overrides]]`` for whichever
    profile is selected, so every profile but ``default`` itself takes
    them. ``default`` does not, because its own overrides are already
    recorded once.

    Parameters
    ----------
    declared
        Each profile as its own table declares it.

    Returns
    -------
    dict of str to Profile
        The same profiles, with ``inherited`` filled in.
    """
    inherited = declared["default"].overrides if "default" in declared else ()
    if not inherited:
        return declared
    return {
        name: profile if name == "default" else profile._replace(inherited=inherited)
        for name, profile in declared.items()
    }


#: A ``binary(...)`` term inside a nextest filterset.
_BINARY_TERM: typ.Final[re.Pattern[str]] = re.compile(r"binary\(\s*([^)\s]+)\s*\)")


def binaries_named(filterset: object) -> frozenset[str]:
    """Return the test binaries a filterset names.

    Parsed as terms rather than searched as text, so a filterset naming
    ``binary(a) | binary(b)`` reports both and one naming neither
    reports nothing. Only the names matter here: whether a term is
    negated is the caller's question, because ``not binary(x)`` in a
    ``default-filter`` excludes the binary while the same term in an
    override's filter selects it.

    Parameters
    ----------
    filterset
        A ``filter`` or ``default-filter`` value, which need not be a
        string.

    Returns
    -------
    frozenset of str
        Every binary name the filterset mentions.

    Examples
    --------
    >>> sorted(binaries_named("binary(trybuild) | binary(ui)"))
    ['trybuild', 'ui']
    >>> binaries_named(None)
    frozenset()
    """
    match filterset:
        case str():
            return frozenset(_BINARY_TERM.findall(filterset))
        case _:
            return frozenset()
