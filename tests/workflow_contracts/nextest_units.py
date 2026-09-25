"""humantime's unit table and its checked arithmetic.

Separated from ``nextest_durations`` so the grammar and the arithmetic
stay legible apart, and so neither module outgrows the 400-line limit
``AGENTS.md`` sets.

Every rule here was read out of humantime 2.3.0's own parser rather than
from its documentation: the unit tables, the checked ``u64`` operations,
the refusal of any fraction of a nanosecond, and the line at the hour
where a fraction stops converting into nanoseconds and starts converting
into whole seconds.
"""

import typing as typ

#: How many nanoseconds humantime counts to a second, and to an hour.
#: The hour matters because it is where humantime changes how it reads a
#: fraction.
NANOSECONDS_PER_SECOND: typ.Final[int] = 1_000_000_000
SECONDS_PER_HOUR: typ.Final[int] = 3600

#: The largest value humantime will hold. Its parser works entirely in
#: checked ``u64`` arithmetic, so a literal, a product and a running
#: total are each bounded by this.
U64_MAX: typ.Final[int] = 2**64 - 1

#: The unit spellings humantime measures in nanoseconds, with their
#: length. Case is not folded anywhere in these tables: ``m`` is minutes
#: and ``M`` is months, so folding would read a thirty-minute budget as
#: a two-and-a-half-year one.
SUBSECOND_NANOSECONDS: typ.Final[dict[str, int]] = {
    "nanos": 1,
    "nsec": 1,
    "ns": 1,
    "usec": 1000,
    "us": 1000,
    "\u00b5s": 1000,
    "millis": 1000000,
    "msec": 1000000,
    "ms": 1000000,
}

#: The unit spellings humantime measures in whole seconds, with their
#: length. A month is a twelfth of a Julian year and a year is 365.25
#: days, which is how ``humantime`` defines them. Spelt out in full
#: rather than trimmed to the spellings this repository happens to use,
#: because refusing a unit nextest accepts fails a configuration the
#: runner is happy with.
UNIT_SECONDS: typ.Final[dict[str, int]] = {
    "seconds": 1,
    "second": 1,
    "secs": 1,
    "sec": 1,
    "s": 1,
    "minutes": 60,
    "minute": 60,
    "mins": 60,
    "min": 60,
    "m": 60,
    "hours": 3600,
    "hour": 3600,
    "hrs": 3600,
    "hr": 3600,
    "h": 3600,
    "days": 86400,
    "day": 86400,
    "d": 86400,
    "weeks": 604800,
    "week": 604800,
    "wks": 604800,
    "wk": 604800,
    "w": 604800,
    "months": 2630016,
    "month": 2630016,
    "M": 2630016,
    "years": 31557600,
    "year": 31557600,
    "yrs": 31557600,
    "yr": 31557600,
    "y": 31557600,
}

#: The spellings of the nanosecond, which is the one unit humantime
#: refuses a fraction of outright.
NANOSECOND_UNITS: typ.Final[frozenset[str]] = frozenset(
    unit for unit, length in SUBSECOND_NANOSECONDS.items() if length == 1
)


class HumantimeOverflowError(Exception):
    """Raised where humantime's checked ``u64`` arithmetic would fail.

    humantime reports every one of these as ``NumberOverflow``, whatever
    the cause: a literal past ``u64``, a multiplication that leaves it,
    or a division with a remainder. Catching one exception and reporting
    one fault keeps this reading's refusals aligned with the parser's.
    """


def checked(value: int) -> int:
    """Return a value humantime's ``u64`` arithmetic could hold.

    Parameters
    ----------
    value : int
        A result humantime computes with ``checked_mul`` or
        ``checked_add``.

    Returns
    -------
    int
        The value unchanged.

    Raises
    ------
    HumantimeOverflowError
        If it would not fit a ``u64``.
    """
    if value > U64_MAX:
        raise HumantimeOverflowError
    return value


def exact_division(numerator: int, denominator: int) -> int:
    """Return a quotient humantime's ``div`` would accept.

    humantime's ``div`` reports an overflow rather than truncating, so a
    remainder is a refusal, not a rounding.

    Parameters
    ----------
    numerator : int
        The dividend.
    denominator : int
        The divisor.

    Returns
    -------
    int
        The exact quotient.

    Raises
    ------
    HumantimeOverflowError
        If the division leaves a remainder.
    """
    if numerator % denominator:
        raise HumantimeOverflowError
    return numerator // denominator


def fraction_of(digits: str) -> tuple[int, int]:
    """Return a fractional part as humantime accumulates it.

    The denominator counts every digit while the numerator ignores
    leading zeros, both in a checked ``u64``. Twenty fractional digits
    therefore overflow the denominator and are refused, whatever the
    value they spell.

    Parameters
    ----------
    digits : str
        The digits after the point, whitespace already removed.

    Returns
    -------
    tuple of (int, int)
        The numerator and denominator.

    Raises
    ------
    HumantimeOverflowError
        If either leaves a ``u64``.
    """
    numerator = 0
    denominator = 1
    zeros = True
    for digit in digits:
        denominator = checked(denominator * 10)
        if digit == "0":
            if not zeros:
                numerator = checked(numerator * 10)
        else:
            zeros = False
            numerator = checked(numerator * 10 + int(digit))
    return numerator, denominator


def integer_parts(magnitude: int, unit: str) -> tuple[int, int]:
    """Return a component's whole part as seconds and nanoseconds.

    Parameters
    ----------
    magnitude : int
        The digits before the point.
    unit : str
        The unit spelling, which must be one humantime accepts.

    Returns
    -------
    tuple of (int, int)
        The whole seconds and the nanoseconds.

    Raises
    ------
    HumantimeOverflowError
        If the product leaves a ``u64``.
    """
    nanos_per = SUBSECOND_NANOSECONDS.get(unit)
    if nanos_per is not None:
        return 0, checked(magnitude * nanos_per)
    return checked(magnitude * UNIT_SECONDS[unit]), 0


def fractional_parts(numerator: int, denominator: int, unit: str) -> tuple[int, int]:
    """Return a component's fractional part as seconds and nanoseconds.

    The unit decides which of the two the fraction lands in, and this is
    the rule a reader working in nanoseconds alone gets wrong. humantime
    converts a fraction of an hour or anything longer into whole
    *seconds*, so ``"0.000001h"`` is refused although its value is a
    whole 3,600,000 ns, while ``"0.25h"`` is fifteen minutes. A fraction
    of a minute or anything shorter converts into whole nanoseconds. A
    fraction of a nanosecond is refused outright, so ``"1.0ns"`` will
    not load even though it spells a whole nanosecond.

    Parameters
    ----------
    numerator : int
        The fraction's numerator, as humantime accumulates it.
    denominator : int
        The fraction's denominator.
    unit : str
        The unit spelling, which must be one humantime accepts.

    Returns
    -------
    tuple of (int, int)
        The whole seconds and the nanoseconds.

    Raises
    ------
    HumantimeOverflowError
        Where humantime's checked arithmetic would fail, the refused
        fraction of a nanosecond included.
    """
    if unit in NANOSECOND_UNITS:
        raise HumantimeOverflowError
    nanos_per = SUBSECOND_NANOSECONDS.get(unit)
    if nanos_per is not None:
        return 0, exact_division(checked(numerator * nanos_per), denominator)
    seconds_per = UNIT_SECONDS[unit]
    if seconds_per >= SECONDS_PER_HOUR:
        return exact_division(checked(numerator * seconds_per), denominator), 0
    return 0, exact_division(
        checked(numerator * seconds_per * NANOSECONDS_PER_SECOND), denominator
    )


def component_parts(value: str, unit: str) -> tuple[int, int]:
    """Return one component as humantime's seconds and nanoseconds.

    Parameters
    ----------
    value : str
        The component's digits, whitespace already removed.
    unit : str
        The unit spelling, which must be one humantime accepts.

    Returns
    -------
    tuple of (int, int)
        The component's whole seconds and its nanoseconds.

    Raises
    ------
    HumantimeOverflowError
        Where humantime's checked arithmetic would fail.
    """
    whole, point, digits = value.partition(".")
    seconds_part, nanoseconds_part = integer_parts(checked(int(whole)), unit)
    if not point:
        return seconds_part, nanoseconds_part
    numerator, denominator = fraction_of(digits)
    fraction_seconds, fraction_nanoseconds = fractional_parts(
        numerator, denominator, unit
    )
    return seconds_part + fraction_seconds, nanoseconds_part + fraction_nanoseconds
