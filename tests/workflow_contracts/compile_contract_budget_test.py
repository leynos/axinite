"""Contract for what the compile-contract binaries are allowed.

Separate from ``timeout_ordering_test``, whose subject is the ordering
of the four tiers. This one is about which tests a per-test allowance
was sized for, which the ordering cannot see: every value can sit in the
right order while a binary that drives `rustc` runs under a bound chosen
for a unit test.

Run via ``make test-workflow-contracts``.
"""

import pytest
from _workflow_policy import REPOSITORY_ROOT
from nextest_config import Profile, profiles
from timeout_budgets import (
    COMPILE_CONTRACT_ALLOWANCE_SECONDS,
    NEXTEST_CONFIG,
    binaries_short_of_allowance,
    compile_contract_binaries,
    excluded_from,
)

#: Every compile-contract binary in the tree, pinned by name. The
#: assertion above reads this set from the sources, and a reading that
#: stopped recognizing one would leave that assertion passing over a
#: smaller set, which is indistinguishable from the tree having fewer.
#: `trybuild` is `tests/trybuild.rs` and `schema_helpers_ui` is
#: `tests/schema_helpers_ui/main.rs`, so the two Cargo target forms are
#: both represented and neither can be dropped unnoticed.
REQUIRED_COMPILE_CONTRACT_BINARIES = frozenset({"trybuild", "schema_helpers_ui"})

#: Which profile excludes which of them outright, as
#: ``default-filter`` says. A reading that reported everything as
#: excluded would satisfy the allowance assertion vacuously, and a
#: reading that reported nothing as excluded would demand an allowance
#: of a binary the profile never runs.
REQUIRED_EXCLUSIONS: dict[tuple[str, str], bool] = {
    ("default", "trybuild"): True,
    ("default", "schema_helpers_ui"): False,
    ("ci", "trybuild"): False,
    ("ci", "schema_helpers_ui"): False,
}


@pytest.fixture(scope="module")
def nextest_profiles() -> dict[str, Profile]:
    """Return each nextest profile the configuration declares.

    Returns
    -------
    dict[str, Profile]
        Profile name to its table and overrides.
    """
    return profiles(NEXTEST_CONFIG.read_text(encoding="utf-8"))


@pytest.mark.parametrize("profile", ["default", "ci"], ids=str)
def test_every_compile_contract_binary_is_allowed_the_longer_budget(
    nextest_profiles: dict[str, Profile], profile: str
) -> None:
    """A binary that drives `rustc` does not fit the base allowance.

    These spawn a fresh compiler per case against the full crate, so a
    whole binary is minutes rather than seconds. The base allowance is
    sized for the ordinary tests, and a compile-contract binary running
    under it is bounded by a number chosen for something else.

    The binaries are discovered from the sources rather than listed
    here, because a new one appearing is exactly the failure this
    guards against. `trybuild` was named in the configuration and
    `schema_helpers_ui` was not, and the second ran under the 300 s base
    allowance, measuring 244 s, 249 s and 257 s on run 34159479674
    before a slower runner ended it at 300 s on run 34271865377. The
    contract then read as satisfied, because every value it compared
    was in the right order; nothing said which tests the base allowance
    was sized for.

    A profile that excludes the binary outright satisfies this too,
    since a binary that never runs needs no allowance. `default`
    excludes `trybuild` that way and runs `schema_helpers_ui`.
    """
    parsed = nextest_profiles[profile]
    binaries = compile_contract_binaries(REPOSITORY_ROOT / "tests")
    assert binaries, (
        "no test source calls trybuild::TestCases; either the compile "
        "contracts moved or this reading stopped recognizing them, and "
        "either way the assertion below would pass over nothing"
    )
    unbounded = binaries_short_of_allowance(parsed, binaries)
    assert not unbounded, (
        f"[profile.{profile}] runs these compile-contract binaries without "
        f"an override allowing them {COMPILE_CONTRACT_ALLOWANCE_SECONDS:.0f}s, "
        f"so each is bounded by the base allowance sized for the ordinary "
        f"tests: {unbounded}"
    )


def test_both_compile_contract_binaries_are_found() -> None:
    """The discovery must not quietly shrink.

    The assertion above is a sweep over whatever this reading returns,
    so a reading that stopped recognizing a binary would pass over the
    remainder and the loss would look exactly like success. Cargo names
    a test target after `tests/<name>.rs` or after the directory in
    `tests/<name>/main.rs`, and the tree has one of each, so pinning the
    set holds both forms.
    """
    assert compile_contract_binaries(REPOSITORY_ROOT / "tests") == (
        REQUIRED_COMPILE_CONTRACT_BINARIES
    ), (
        "the compile-contract binaries are not the ones this contract pins; "
        "a new one needs an entry here and an override allowing it the "
        "longer budget, and a removed one needs both taken out"
    )


@pytest.mark.parametrize(
    ("profile", "binary", "expected"),
    [
        pytest.param(profile, binary, expected, id=f"{profile}-{binary}")
        for (profile, binary), expected in REQUIRED_EXCLUSIONS.items()
    ],
)
def test_the_exclusion_reading_says_what_the_filter_says(
    nextest_profiles: dict[str, Profile], profile: str, binary: str, expected: bool
) -> None:
    """A binary a profile never runs needs no allowance, and no other.

    The allowance assertion skips an excluded binary, so a reading that
    reported every binary as excluded would satisfy it over nothing,
    and one that reported none as excluded would demand an allowance of
    `trybuild` under a profile whose `default-filter` removes it. Both
    readings agree with a correct one on the assertion's verdict for
    this tree, which is why they are pinned here instead.
    """
    assert excluded_from(nextest_profiles[profile], binary) is expected, (
        f"[profile.{profile}]'s default-filter must "
        f"{'exclude' if expected else 'run'} binary({binary})"
    )
