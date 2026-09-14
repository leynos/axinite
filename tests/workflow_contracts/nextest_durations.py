"""Reading a nextest duration as the runner reads one.

Separated from ``nextest_config`` so the grammar and the configuration
structure stay legible apart, and so neither module outgrows the
400-line limit ``AGENTS.md`` sets.

nextest deserializes every duration with ``humantime_serde``. That
grammar is wider than one number and a short unit, and a reader
narrower than it refuses configuration nextest loads, which makes the
contract fail on a correct file and blame the file for it.

The grammar below was measured against humantime 2.3.0, which is what
the lockfile of the pinned cargo-nextest release resolves (this
repository pins ``cargo-nextest@0.9.140``), by compiling that parser
and running the cases through it rather than inferring them from prose.
A value may carry a fractional part with whitespace tolerated around
the point, so ``1.5m`` and ``1 . 5 m`` are both ninety seconds;
whitespace inside the number is ignored too, so ``1 0s`` is ten seconds
and ``1 2 . 3 4 s`` is 12.34; ``wk``, ``wks``, ``yr`` and ``yrs`` are
accepted alongside the longer spellings; and the bare string ``0`` is
the one duration humantime reads without a unit.
"""

import re
import typing as typ

from nextest_errors import NextestConfigurationError
from nextest_units import (
    NANOSECONDS_PER_SECOND,
    SUBSECOND_NANOSECONDS,
    U64_MAX,
    UNIT_SECONDS,
    HumantimeOverflowError,
    component_parts,
)

#: Digits with whitespace tolerated between them. humantime's parser
#: skips whitespace while it accumulates a number, so ``1 0s`` is ten
#: seconds rather than a malformed duration, and the same holds either
#: side of the point: ``1 2 . 3 4 s`` is 12.34 seconds.
_SPACED_DIGITS: typ.Final[str] = r"\d(?:\s*\d)*"

#: One value-and-unit pair. The fractional part is optional and
#: humantime tolerates whitespace around the point; a leading point, a
#: trailing point, a second point, a sign and a digit separator are all
#: refused there, and so are refused here.
_COMPONENT: typ.Final[re.Pattern[str]] = re.compile(
    # The micro sign is written as an escape: the literal is visually
    # indistinguishable from the Greek small letter mu, which humantime
    # refuses, so the two must not be told apart by eye here.
    rf"(?P<value>{_SPACED_DIGITS}(?:\s*\.\s*{_SPACED_DIGITS})?)"
    r"\s*(?P<unit>[A-Za-z\u00b5]+)\s*"
)

#: The one duration humantime reads with no unit. Its parser
#: special-cases the exact text before reading a single character, so
#: the comparison is against the raw value rather than a stripped one:
#: ``" 0 "``, ``"0 "``, ``"00"`` and ``"0.0"`` are each refused, and a
#: reader that stripped first would accept a duration nextest rejects.
_BARE_ZERO: typ.Final[str] = "0"


def _component_at(duration: str, text: str, position: int) -> tuple[int, int, int]:
    """Return one component's seconds and nanoseconds, and where it ends.

    Parameters
    ----------
    duration : str
        The whole duration, carried for the message so a failure names
        what was configured rather than the tail being read.
    text : str
        The duration with its surrounding whitespace removed.
    position : int
        Where in ``text`` this component starts.

    Returns
    -------
    tuple of (int, int, int)
        The component's whole seconds, its nanoseconds, and the offset
        at which the next component starts.

    Raises
    ------
    NextestConfigurationError
        If no component starts here, if its unit is not one humantime
        accepts, or if its value is one humantime cannot represent.
    """
    component = _COMPONENT.match(text, position)
    if component is None:
        message = (
            f"unrecognized nextest duration {duration!r}; nextest reads "
            f"durations with humantime, which wants a sequence of numbers "
            f'each carrying a unit, such as "60s", "2h 37m" or "1.5m"'
        )
        raise NextestConfigurationError(message)
    unit = component["unit"]
    if unit not in SUBSECOND_NANOSECONDS and unit not in UNIT_SECONDS:
        message = (
            f"nextest duration {duration!r} names the unit {unit!r}, which "
            f"humantime does not accept; note that 'm' is minutes and 'M' "
            f"is months"
        )
        raise NextestConfigurationError(message)
    # humantime tolerates whitespace inside and around the number, so
    # the matched value can read "1 . 5"; the digits are joined before
    # the arithmetic reads them.
    value = "".join(component["value"].split())
    try:
        seconds_part, nanoseconds_part = component_parts(value, unit)
    except HumantimeOverflowError:
        message = (
            f"nextest duration {duration!r} carries a component humantime "
            f"cannot represent: its arithmetic is checked u64 throughout, it "
            f"converts a fraction of an hour or longer into whole seconds and "
            f"a shorter one into whole nanoseconds, and it refuses any "
            f"fraction of a nanosecond"
        )
        raise NextestConfigurationError(message) from None
    return seconds_part, nanoseconds_part, component.end()


def seconds(duration: str) -> float:
    """Convert a nextest duration to seconds.

    Parameters
    ----------
    duration : str
        A duration as nextest spells it, such as ``"60s"``, the
        multi-component ``"2h 37m"`` or the fractional ``"1.5m"``.

    Returns
    -------
    float
        The duration in seconds.

    Raises
    ------
    NextestConfigurationError
        If the text is not a duration nextest would accept.
    """
    if duration == _BARE_ZERO:
        return 0.0
    text = duration.strip()
    if not text:
        message = (
            f"unrecognized nextest duration {duration!r}: it is empty, and "
            f"humantime reads no duration from nothing"
        )
        raise NextestConfigurationError(message)
    total_seconds = 0
    total_nanoseconds = 0
    position = 0
    while position < len(text):
        component_seconds, component_nanoseconds, position = _component_at(
            duration, text, position
        )
        # humantime carries the running total as whole seconds beside a
        # nanosecond remainder, and both are checked, so a sum past that
        # ceiling will not load even though each component did.
        total_nanoseconds += component_nanoseconds
        if total_nanoseconds > NANOSECONDS_PER_SECOND:
            component_seconds += total_nanoseconds // NANOSECONDS_PER_SECOND
            total_nanoseconds %= NANOSECONDS_PER_SECOND
        total_seconds += component_seconds
        if total_seconds > U64_MAX:
            message = (
                f"nextest duration {duration!r} totals more seconds than the "
                f"u64 humantime accumulates them in"
            )
            raise NextestConfigurationError(message)
    return total_seconds + total_nanoseconds / NANOSECONDS_PER_SECOND
