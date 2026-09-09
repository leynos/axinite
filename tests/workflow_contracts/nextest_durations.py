"""Reading a nextest duration as the runner reads one.

Separated from ``nextest_config`` so the grammar and the configuration
structure stay legible apart, and so neither module outgrows the
400-line limit ``AGENTS.md`` sets.

nextest deserializes every duration with ``humantime_serde``. That
grammar is wider than one number and a short unit, and a reader
narrower than it refuses configuration nextest loads, which makes the
contract fail on a correct file and blame the file for it.
"""

import re
import typing as typ

from nextest_errors import NextestConfigurationError

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
