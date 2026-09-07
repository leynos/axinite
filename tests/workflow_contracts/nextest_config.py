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

_DURATION: typ.Final[re.Pattern[str]] = re.compile(
    r"^\s*(?P<value>\d+(?:\.\d+)?)\s*(?P<unit>ms|s|m|h)\s*$"
)

_UNIT_SECONDS: typ.Final[dict[str, float]] = {
    "ms": 0.001,
    "s": 1.0,
    "m": 60.0,
    "h": 3600.0,
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
    """

    name: str
    own: dict[str, object]
    overrides: tuple[dict[str, object], ...]

    def tables(self) -> tuple[dict[str, object], ...]:
        """Return every table the profile reads a budget from.

        Returns
        -------
        tuple of dict
            The profile's own table first, then each override.
        """
        return (self.own, *self.overrides)


def seconds(duration: str) -> float:
    """Convert a nextest duration to seconds.

    Parameters
    ----------
    duration
        A duration as nextest spells it, such as ``"300s"``.

    Returns
    -------
    float
        The duration in seconds.

    Raises
    ------
    NextestConfigurationError
        If the text is not a duration nextest would accept.
    """
    match = _DURATION.match(duration)
    if match is None:
        message = f"unrecognized nextest duration {duration!r}"
        raise NextestConfigurationError(message)
    return float(match["value"]) * _UNIT_SECONDS[match["unit"]]


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
    return dict(value) if isinstance(value, dict) else {}


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
    return list(value) if isinstance(value, list) else []


def profiles(config_text: str) -> dict[str, Profile]:
    """Return each profile the configuration declares, keyed by name.

    Parameters
    ----------
    config_text
        A nextest configuration file's text.

    Returns
    -------
    dict of str to Profile
        Profile name to its table and overrides.

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
    found: dict[str, Profile] = {}
    for name, raw in _table(parsed.get("profile")).items():
        table = _table(raw)
        overrides = tuple(_table(entry) for entry in _entries(table.get("overrides")))
        own = {key: value for key, value in table.items() if key != "overrides"}
        found[str(name)] = Profile(name=str(name), own=own, overrides=overrides)
    return found


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
