"""Reading a nextest duration as the runner reads one.

Separated from ``nextest_config`` so the grammar and the configuration
structure stay legible apart, and so neither module outgrows the
400-line limit ``AGENTS.md`` sets.

nextest deserializes every duration with ``humantime_serde``. That
grammar is wider than one number and a short unit, and a reader
narrower than it refuses configuration nextest loads, which makes the
contract fail on a correct file and blame the file for it.

The grammar below was measured against humantime 2.4.0, the version
nextest resolves, by compiling that parser and running the cases
through it, rather than inferred from prose. A value may carry a
fractional part with whitespace tolerated around the point, so ``1.5m``
and ``1 . 5 m`` are both ninety seconds; ``wk``, ``wks``, ``yr`` and
``yrs`` are accepted alongside the longer spellings; and the bare
string ``0`` is the one duration humantime reads without a unit.

One narrowness is deliberate and named here rather than left silent:
humantime also skips whitespace inside a number, so it reads ``1 5s``
as fifteen seconds. No configuration spells a number that way, and
admitting it would cost the value pattern its legibility, so this
reader refuses it.
"""

import re
import typing as typ

from nextest_errors import NextestConfigurationError

#: One value-and-unit pair. The fractional part is optional and
#: humantime tolerates whitespace around the point; a leading point, a
#: trailing point, a second point, a sign and a digit separator are all
#: refused there, and so are refused here.
_COMPONENT: typ.Final[re.Pattern[str]] = re.compile(
    r"(?P<value>\d+(?:\s*\.\s*\d+)?)\s*(?P<unit>[A-Za-zµ]+)\s*"
)

#: The one duration humantime reads without a unit. ``00``, ``0.0`` and
#: a trailing space all fail there, so the exception is this literal and
#: nothing wider.
_BARE_ZERO: typ.Final[str] = "0"

#: Every unit spelling ``humantime`` accepts, with its length in seconds.
#: Case matters: ``m`` is minutes and ``M`` is months, so the table is
#: consulted without folding case. A month is a twelfth of a Julian year
#: and a year is 365.25 days, which is how ``humantime`` defines them.
#: Spelt out in full rather than trimmed to the plausible spellings,
#: because refusing a unit nextest accepts fails a configuration the
#: runner is happy with.
_UNIT_SECONDS: typ.Final[dict[str, float]] = {
    "nanos": 1e-9,
    "nsec": 1e-9,
    "ns": 1e-9,
    "usec": 1e-6,
    "us": 1e-6,
    "µs": 1e-6,
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
    "wks": 604800.0,
    "wk": 604800.0,
    "w": 604800.0,
    "months": 2630016.0,
    "month": 2630016.0,
    "M": 2630016.0,
    "years": 31557600.0,
    "year": 31557600.0,
    "yrs": 31557600.0,
    "yr": 31557600.0,
    "y": 31557600.0,
}


def _component_at(duration: str, text: str, position: int) -> tuple[float, int]:
    """Return one component's length in seconds and where it ends.

    Parameters
    ----------
    duration
        The whole duration, carried for the error message so a failure
        names what was configured rather than the tail being read.
    text
        The duration with its surrounding whitespace removed.
    position
        Where in ``text`` this component starts.

    Returns
    -------
    tuple of (float, int)
        The component's length in seconds, and the offset at which the
        next component starts.

    Raises
    ------
    NextestConfigurationError
        If no component starts here, or its unit is not one humantime
        accepts.
    """
    component = _COMPONENT.match(text, position)
    if component is None:
        message = (
            f"unrecognized nextest duration {duration!r}; nextest reads "
            f"durations with humantime, which wants a sequence of numbers "
            f'each carrying a unit, such as "300s", "2h 37m" or "1.5m"'
        )
        raise NextestConfigurationError(message)
    unit = component["unit"]
    length = _UNIT_SECONDS.get(unit)
    if length is None:
        message = (
            f"nextest duration {duration!r} names the unit {unit!r}, which "
            f"humantime does not accept; note that 'm' is minutes and 'M' "
            f"is months"
        )
        raise NextestConfigurationError(message)
    # humantime tolerates whitespace around the fractional point, so the
    # matched value can read "1 . 5", which float cannot.
    return float("".join(component["value"].split())) * length, component.end()


def seconds(duration: str) -> float:
    """Convert a nextest duration to seconds.

    Parameters
    ----------
    duration
        A duration as nextest spells it, such as ``"300s"``, the
        multi-component ``"2h 37m"`` or the fractional ``"1.5m"``.

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
    >>> seconds("1.5m")
    90.0
    """
    text = duration.strip()
    if not text:
        message = (
            f"unrecognized nextest duration {duration!r}: it is empty, and "
            f"humantime reads no duration from nothing"
        )
        raise NextestConfigurationError(message)
    if text == _BARE_ZERO:
        return 0.0
    total = 0.0
    position = 0
    while position < len(text):
        length, position = _component_at(duration, text, position)
        total += length
    return total
