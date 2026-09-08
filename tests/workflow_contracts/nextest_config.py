"""Parsing `.config/nextest.toml` into the profiles nextest reads.

Separated from ``timeout_budgets`` so the parsing and the arithmetic
stay legible apart, and so neither module outgrows the 400-line limit
``AGENTS.md`` sets.

The configuration is parsed with ``tomllib`` rather than matched as
text. A text match finds a key inside a comment, inside a ``filter``
string, or in a table nextest never consults, and reports a budget the
runner does not use.
"""

import re
import tomllib
import typing as typ

#: A duration as ``humantime`` spells it: one or more whole-number
#: components, each with a unit, optionally separated by whitespace.
#: nextest deserializes every duration with ``humantime_serde``, which
#: accepts ``"2h 37m"`` and ``"2h37m"`` as readily as ``"300s"``, and
#: rejects a fractional value such as ``"1.5s"`` outright. A reader
#: accepting one component only rejects configuration nextest accepts,
#: and the contract then fails on a file that is correct.
_DURATION: typ.Final[re.Pattern[str]] = re.compile(r"\A\s*(?:\d+\s*[A-Za-z]+\s*)+\Z")

#: One component of such a duration.
_COMPONENT: typ.Final[re.Pattern[str]] = re.compile(
    r"(?P<value>\d+)\s*(?P<unit>[A-Za-z]+)"
)

#: Every unit spelling ``humantime`` accepts, with its length in seconds.
#: Case matters: ``m`` is minutes and ``M`` is months, so the table is
#: consulted without folding case. A month is a twelfth of a Julian year
#: and a year is 365.25 days, which is how ``humantime`` defines them.
_UNIT_SECONDS: typ.Final[dict[str, float]] = {
    "nanos": 1e-9,
    "nsec": 1e-9,
    "ns": 1e-9,
    "usec": 1e-6,
    "us": 1e-6,
    "millis": 0.001,
    "msec": 0.001,
    "ms": 0.001,
    "seconds": 1.0,
    "second": 1.0,
    "secs": 1.0,
    "sec": 1.0,
    "s": 1.0,
    "minutes": 60.0,
    "minute": 60.0,
    "mins": 60.0,
    "min": 60.0,
    "m": 60.0,
    "hours": 3600.0,
    "hour": 3600.0,
    "hrs": 3600.0,
    "hr": 3600.0,
    "h": 3600.0,
    "days": 86400.0,
    "day": 86400.0,
    "d": 86400.0,
    "weeks": 604800.0,
    "week": 604800.0,
    "w": 604800.0,
    "months": 2630016.0,
    "month": 2630016.0,
    "M": 2630016.0,
    "years": 31557600.0,
    "year": 31557600.0,
    "y": 31557600.0,
}


class NextestConfigurationError(ValueError):
    """Raised when the configuration cannot be read as a set of budgets.

    Separate from a budget in the wrong order. A file that is not TOML,
    a profile declaring no ``slow-timeout``, or one whose
    ``global-timeout`` has been commented out, is a configuration this
    contract cannot reason about rather than one whose tiers are
    inverted.
    """


class UnboundedTestError(NextestConfigurationError):
    """Raised when a ``slow-timeout`` terminates no test.

    ``terminate-after`` is optional, and without it nextest marks a test
    slow and lets it run on, so the configuration parses, reads as
    deliberate, and bounds nothing. Reporting that as a period-long
    budget would put a number on the tier that is missing.
    """


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


def seconds(duration: str) -> float:
    """Convert a nextest duration to seconds.

    Parameters
    ----------
    duration
        A duration as nextest spells it, such as ``"300s"`` or the
        multi-component ``"2h 37m"``.

    Returns
    -------
    float
        The duration in seconds.

    Raises
    ------
    NextestConfigurationError
        If the text is not a duration nextest would accept.

    Examples
    --------
    >>> seconds("300s")
    300.0
    >>> seconds("2h 37m")
    9420.0
    """
    if _DURATION.match(duration) is None:
        message = (
            f"unrecognized nextest duration {duration!r}; nextest reads "
            f"durations with humantime, which wants whole-number components "
            f'each carrying a unit, such as "300s" or "2h 37m"'
        )
        raise NextestConfigurationError(message)
    total = 0.0
    for component in _COMPONENT.finditer(duration):
        unit = component["unit"]
        length = _UNIT_SECONDS.get(unit)
        if length is None:
            message = (
                f"nextest duration {duration!r} names the unit {unit!r}, which "
                f"humantime does not accept; note that 'm' is minutes and 'M' "
                f"is months"
            )
            raise NextestConfigurationError(message)
        total += float(component["value"]) * length
    return total


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
