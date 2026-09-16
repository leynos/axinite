"""The duration grammar, against the one humantime reads.

Split from ``timeout_reading_test`` so neither module outgrows the
400-line limit ``AGENTS.md`` sets. How a configuration's budgets are
read is asserted there; what a duration *is* is asserted here.

Every spelling was measured against humantime 2.3.0, which is what the
lockfile of the pinned cargo-nextest release resolves, by compiling that
parser and running the cases through it.
"""

import re
import typing as typ

import pytest
from hypothesis import given
from hypothesis import strategies as st
from nextest_config import NextestConfigurationError, seconds
from nextest_durations import _WHITESPACE_CHARS
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
        pytest.param(
            "18446744073709551615s 999999999ns",
            18446744073709551615 + 0.999999999,
            id="the-largest-duration-humantime-holds",
        ),
        pytest.param("0.5s 0.5s", 1.0, id="two-half-seconds-that-carry-to-one"),
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
            "18446744073709551615s 500ms 500ms",
            id="a-carry-that-completes-a-second-past-the-u64",
        ),
        pytest.param("\u0663\u0660\u0660s", id="a-run-of-unicode-digits"),
        pytest.param("3\u0660\u0660s", id="a-unicode-digit-after-an-ascii-one"),
        pytest.param(
            "18446744073709551615ns 18446744073709551615ns",
            id="a-nanosecond-sum-past-the-u64-before-it-carries",
        ),
        pytest.param("1\u001cs", id="a-file-separator-between-a-digit-and-its-unit"),
        pytest.param("\u001c45m", id="a-file-separator-before-the-number"),
        pytest.param("45m\u001f", id="a-unit-separator-after-the-unit"),
        pytest.param("1\u001d0s", id="a-group-separator-inside-the-number"),
        pytest.param("", id="empty"),
    ],
)
def test_a_duration_nextest_would_refuse_is_refused_here(duration: str) -> None:
    r"""The grammar is matched, not merely widened.

    ``humantime`` admits a fractional part but nothing looser, so a
    reader that accepted more would put a number on a configuration
    nextest fails to load, and the contract would certify a file that
    cannot run. Each spelling here was refused by humantime 2.3.0, the
    version the pinned cargo-nextest release resolves, when the cases
    were run through it. The bare zero is the sharpest: humantime
    special-cases the exact text before reading a character, so a reader
    that stripped whitespace before comparing would accept `" 0 "`,
    which nextest rejects.

    One case leaves the parser by a different door. Two half-seconds on
    top of the largest whole second reach exactly a billion nanoseconds,
    which humantime's carry declines to move and ``Duration::new`` then
    moves regardless, panicking on the overflow. humantime returns no
    error for that text because it never returns at all, so nextest
    cannot load it either way, and a reader carrying only past a
    complete second would report a duration for it. One nanosecond
    short of that carry is the largest duration humantime does hold,
    and it sits in the acceptance cases as the other half of the pair.

    The two runs of Unicode digits are the reader's own width rather
    than the parser's. Python's ``\d`` matches every Unicode decimal
    digit and ``int`` reads them, so both spellings were three hundred
    seconds here; humantime compares against ``'0'..='9'`` and refuses
    them, reporting "expected number at 0" for the run that opens with
    one and "invalid character at 1" for the run that does not. The
    mixed spelling is the sharper of the two, because a reader that
    checked only its first character would still accept it.

    The nanosecond accumulator has a ceiling of its own, and it is not
    the seconds' one. humantime's ``add_current`` opens with
    ``(out.subsec_nanos() as u64).add(nsec)?``, before any carry, so the
    remainder held so far plus this component's nanoseconds must fit a
    ``u64`` by itself: two values of ``u64::MAX`` nanoseconds carry the
    first to 18,446,744,073 seconds and then overflow on the second. The
    duration they name, about 36.9 billion seconds, is far below the
    seconds ceiling, so a reader summing into Python's unbounded integer
    and checking only the seconds afterwards finds nothing wrong and
    reports a duration nextest will not start under.

    The four C0 separators are the mirror image of those digits, and
    they cost nothing to write and everything to miss. Python's ``\s``
    matches U+001C to U+001F and Rust's ``char::is_whitespace`` does
    not, so a reader spelling its whitespace ``\s`` skips one wherever
    it skips a space. All four were read as durations here until this
    was written: `1\x1cs` as one second, `\x1c45m` and `45m\x1f` as
    forty-five minutes, `1\x1d0s` as ten. The separator between two
    digits is the one nobody would notice in a file, which is why it is
    a case of its own.
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


def test_the_whitespace_class_is_rusts_and_not_pythons() -> None:
    r"""Pin the class in both directions, over the whole of Unicode.

    Rust's ``char::is_whitespace`` is the Unicode White_Space property.
    Python's ``\s`` is that property plus U+001C to U+001F, the file,
    group, record and unit separators, and ``str.strip`` and
    ``str.split`` carry the same excess. The refusal cases above catch
    the excess for four inputs; this catches it for the class.

    Both directions are asserted. A class that had lost a genuine space
    would make this reader refuse configurations nextest loads, which is
    the opposite failure and equally wrong, and no refusal case would
    show it. Pinning both means a change to either language's notion of
    whitespace fails here rather than in a runner months later.
    """
    ours = set(_WHITESPACE_CHARS)
    pythons = {chr(cp) for cp in range(0x110000) if re.match(r"\s", chr(cp))}
    separators = {"\u001c", "\u001d", "\u001e", "\u001f"}
    assert pythons - ours == separators, (
        f"Python's whitespace exceeds this reader's by something other than "
        f"the four C0 separators: {sorted(pythons - ours - separators)!r}"
    )
    assert not ours - pythons, (
        f"this reader treats as whitespace something Python does not: "
        f"{sorted(ours - pythons)!r}"
    )


@pytest.mark.parametrize(
    "spelling",
    [
        pytest.param("1 0s", id="a-space"),
        pytest.param("1\u00a00s", id="a-no-break-space"),
        pytest.param("1\u20080s", id="a-punctuation-space"),
    ],
)
def test_the_digit_join_drops_every_whitespace_the_class_allows(
    spelling: str,
) -> None:
    """Exercise the join through the widest whitespace the class allows.

    The digits of a spaced number are joined after the pattern has
    matched, so while the pattern refuses a separator the join can never
    meet one, and no input through `seconds` can tell a correct join
    from ``str.split``. The join is spelled out anyway, because a later
    widening of the pattern would turn a refusal into a silently
    different number, and a line nothing can reach is a line nothing
    proves. These three spellings do reach it.
    """
    assert seconds(spelling) == pytest.approx(10.0)
