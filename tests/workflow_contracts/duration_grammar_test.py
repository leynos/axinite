"""The duration grammar, against the one humantime reads.

Split from ``timeout_reading_test`` so neither module outgrows the
400-line limit ``AGENTS.md`` sets. How a configuration's budgets are
read is asserted there; what a duration *is* is asserted here.

Every spelling was measured against humantime 2.3.0, which is what the
lockfile of the pinned cargo-nextest release resolves, by compiling that
parser and running the cases through it.
"""

import typing as typ

import pytest
from hypothesis import given
from hypothesis import strategies as st
from nextest_config import NextestConfigurationError, seconds
from nextest_units import (
    SUBSECOND_NANOSECONDS,
    UNIT_SECONDS,
    HumantimeOverflowError,
)


@pytest.mark.parametrize(
    ("duration", "expected"),
    [
        pytest.param("300s", 300.0, id="one-component"),
        pytest.param("2h 37m", 9420.0, id="two-components-spaced"),
        pytest.param("2h37m", 9420.0, id="two-components-joined"),
        pytest.param("1h 30m 15s", 5415.0, id="three-components"),
        pytest.param("15min", 900.0, id="a-long-unit-spelling"),
        pytest.param("500ms", 0.5, id="milliseconds"),
        pytest.param("1d", 86400.0, id="days"),
        pytest.param("1.5m", 90.0, id="a-fractional-value"),
        pytest.param("1 . 5 m", 90.0, id="a-fractional-value-spaced-around-the-point"),
        pytest.param("4.2s", 4.2, id="a-fractional-value-in-seconds"),
        pytest.param("2wk", 1209600.0, id="the-short-week-spelling"),
        pytest.param("2wks", 1209600.0, id="the-short-plural-week-spelling"),
        pytest.param("1yr", 31557600.0, id="the-short-year-spelling"),
        pytest.param("3yrs", 94672800.0, id="the-short-plural-year-spelling"),
        pytest.param("1\u00b5s", 1e-6, id="the-micro-sign-spelling"),
        pytest.param("1nanos", 1e-9, id="the-long-nanosecond-spelling"),
        pytest.param("1millis", 0.001, id="the-long-millisecond-spelling"),
        pytest.param("0", 0.0, id="the-bare-zero-humantime-reads-without-a-unit"),
        pytest.param("1 0s", 10.0, id="whitespace-inside-the-number"),
        pytest.param("1 2 . 3 4 s", 12.34, id="whitespace-throughout-the-number"),
        pytest.param("1.999999999s", 1.999999999, id="nanosecond-precision"),
        pytest.param("0.000001ms", 1e-9, id="a-fraction-that-lands-on-a-nanosecond"),
        pytest.param("0.25h", 900.0, id="a-fraction-of-an-hour-in-whole-seconds"),
        pytest.param("0.5m", 30.0, id="a-fraction-of-a-minute"),
        pytest.param("0.5y", 15778800.0, id="a-fraction-of-a-year"),
    ],
)
def test_the_duration_grammar_matches_the_one_nextest_reads(
    duration: str, expected: float
) -> None:
    """nextest deserializes durations with ``humantime``, not one unit.

    A reader accepting a single component rejects `2h 37m`, which
    nextest accepts, so the contract would fail on a configuration that
    is correct and the failure would name the reader's limitation as
    though it were the file's fault. Every spelling here was measured
    against humantime 2.3.0, which is what the lockfile of the pinned
    cargo-nextest release resolves, by running it through that parser
    rather than inferring it from prose: fractional values are accepted,
    whitespace is tolerated around the point and inside the number
    itself, the short week and year spellings are units, and the bare
    `0` is the one duration humantime reads without a unit.
    """
    assert seconds(duration) == pytest.approx(expected), (
        f"{duration!r} must read as {expected} seconds"
    )


@pytest.mark.parametrize(
    "duration",
    [
        pytest.param("300", id="no-unit"),
        pytest.param(".5s", id="a-value-that-is-only-a-fractional-part"),
        pytest.param("5.s", id="a-value-whose-fractional-part-is-missing"),
        pytest.param("1.5.5s", id="a-value-with-a-second-point"),
        pytest.param("-1s", id="a-signed-value"),
        pytest.param("300 fortnights", id="an-unknown-unit"),
        pytest.param("1S", id="a-unit-whose-case-is-wrong"),
        pytest.param("00", id="a-zero-that-is-not-the-bare-one"),
        pytest.param(" 0 ", id="a-bare-zero-carrying-whitespace"),
        pytest.param("0 ", id="a-bare-zero-with-a-trailing-space"),
        pytest.param("0.0000000002s", id="below-one-nanosecond"),
        pytest.param("0.0000000015s", id="a-fraction-of-a-nanosecond"),
        pytest.param("1.0ns", id="a-whole-fraction-of-a-nanosecond"),
        pytest.param("2.0ns", id="a-larger-whole-fraction-of-a-nanosecond"),
        pytest.param("0.000001h", id="an-hour-fraction-that-is-not-whole-seconds"),
        pytest.param(
            "1000000000000000000000ns",
            id="a-literal-past-the-u64-humantime-reads-it-into",
        ),
        pytest.param(
            "0.1000000000000000000s",
            id="a-fraction-whose-product-leaves-the-u64",
        ),
        pytest.param(
            "1.00000000000000000000s",
            id="a-fraction-whose-denominator-leaves-the-u64",
        ),
        pytest.param(
            "18446744073709551615s 1s",
            id="a-sum-past-the-u64-humantime-accumulates-into",
        ),
        pytest.param(
            "18446744073709551615s 1000ms",
            id="a-carry-that-completes-a-second-past-the-u64",
        ),
        pytest.param("", id="empty"),
    ],
)
def test_a_duration_nextest_would_refuse_is_refused_here(duration: str) -> None:
    """The grammar is matched, not merely widened.

    ``humantime`` admits a fractional part but nothing looser, so a
    reader that accepted more would put a number on a configuration
    nextest fails to load, and the contract would certify a file that
    cannot run. Each spelling here was refused by humantime 2.3.0, the
    version the pinned cargo-nextest release resolves, when the cases
    were run through it. The bare zero is the sharpest: humantime
    special-cases the exact text before reading a character, so a reader
    that stripped whitespace before comparing would accept `" 0 "`,
    which nextest rejects.

    One case leaves the parser by a different door. A thousand
    milliseconds on top of the largest whole second reach exactly a
    billion nanoseconds, which humantime's carry declines to move and
    ``Duration::new`` then moves regardless, panicking on the overflow.
    humantime returns no error for that text because it never returns
    at all, so nextest cannot load it either way, and a reader carrying
    only past a complete second would report a duration for it.
    """
    with pytest.raises(NextestConfigurationError):
        seconds(duration)


def test_a_refusal_names_the_arithmetic_that_produced_it() -> None:
    """The overflow that refused a component survives the translation.

    ``NextestConfigurationError`` says which configuration is at fault;
    ``HumantimeOverflowError`` says which of humantime's checked
    operations declined. The message names all three rules in one
    sentence, so on its own a traceback cannot separate a literal past
    the ``u64`` from a multiplication that left it from a division with
    a remainder. Suppressing the cause throws that away.
    """
    with pytest.raises(NextestConfigurationError) as refusal:
        seconds("1.0ns")
    assert isinstance(refusal.value.__cause__, HumantimeOverflowError), (
        "the parser failure is the cause of the refusal, not a detail to drop"
    )


#: Every unit spelling the reader knows, with its length in seconds.
#: Drawn from the reader's own tables rather than a second copy, because
#: the property below is about how components combine, and a unit the
#: reader does not know is a different question, asked by the refusal
#: cases above.
_SECONDS_OF: typ.Final[dict[str, float]] = {
    unit: nanos / 1e9 for unit, nanos in SUBSECOND_NANOSECONDS.items()
} | {unit: float(seconds) for unit, seconds in UNIT_SECONDS.items()}

#: The units any three-digit fraction can be written against. humantime
#: converts a fraction differently either side of the hour: below it the
#: fraction becomes whole nanoseconds, at it and above it whole seconds,
#: and a fraction of a nanosecond is refused outright. So `1.1M` is not
#: a duration, a tenth of a month being no whole number of seconds, and
#: neither is `1.0ns`. Between the microsecond and the minute the
#: nanosecond scale divides by a thousand whatever the numerator. The
#: two excluded ends are asserted by name in the cases above.
_UNITS = st.sampled_from(
    sorted(
        unit
        for unit, length in _SECONDS_OF.items()
        if 1e-6 <= length <= 60.0 and round(length * 1e9) % 1000 == 0
    )
)
_VALUES = st.integers(min_value=1, max_value=10_000)
_FRACTIONS = st.integers(min_value=0, max_value=999)
_SEPARATORS = st.sampled_from(["", " ", "  "])


@given(
    components=st.lists(st.tuples(_VALUES, _FRACTIONS, _UNITS), min_size=1, max_size=6),
    separators=st.lists(_SEPARATORS, min_size=6, max_size=6),
)
def test_a_sequence_of_components_sums_to_its_parts(
    components: list[tuple[int, int, str]], separators: list[str]
) -> None:
    """humantime sums a sequence, and the reader must sum the same one.

    The cases above pin the spellings a configuration is likely to use.
    This is the invariant underneath them: however many components a
    duration carries, whatever their units, and whether or not they are
    spaced apart, the reading is the sum of the components read
    separately. A reader that stopped at the first component, or that
    dropped one in the middle, would satisfy every fixed case whose
    total happened to survive and fail here.
    """
    written = "".join(
        f"{whole}.{fraction}{unit}{separator}"
        for (whole, fraction, unit), separator in zip(components, separators)
    )
    expected = sum(
        float(f"{whole}.{fraction}") * _SECONDS_OF[unit]
        for whole, fraction, unit in components
    )
    assert seconds(written) == pytest.approx(expected), (
        f"{written!r} must read as the sum of its components"
    )


def test_minutes_and_months_are_told_apart() -> None:
    """``m`` is minutes and ``M`` is months, and ``humantime`` is exact.

    Folding case here would read a thirty-minute budget as a
    two-and-a-half-year one, or the reverse, and either reading puts a
    plausible number on the wrong tier.
    """
    assert seconds("30m") == pytest.approx(1800.0)
    assert seconds("30M") == pytest.approx(30 * 2630016.0)
