"""Contract for nested Cargo budgets and required contract targets.

Separate from ``timeout_ordering_test``, whose subject is the ordering
of the four tiers. This one is about which tests a per-test allowance
was sized for, which the ordering cannot see: every value can sit in the
right order while a binary that drives `rustc` runs under a bound chosen
for a unit test.

Run via ``make test-workflow-contracts``.
"""

import pytest
from _workflow_policy import REPOSITORY_ROOT
from compile_contract_inventory import (
    REQUIRED_COMPILE_CONTRACT_BINARIES,
    REQUIRED_NESTED_COMPILER_BINARIES,
    compile_contract_binaries,
)
from nextest_config import Profile, binaries_selected
from timeout_budgets import (
    NESTED_COMPILER_ALLOWANCE_SECONDS,
    binaries_short_of_allowance,
    excluded_from,
)

REQUIRED_COMPILE_CONTRACT_GROUP = "compile-contracts"

#: Which profile excludes each required contract binary, as
#: ``default-filter`` says. A reading that reported everything as
#: excluded would satisfy the allowance assertion vacuously, and a
#: reading that reported nothing as excluded would demand an allowance
#: of a binary the profile never runs.
REQUIRED_EXCLUSIONS: dict[tuple[str, str], bool] = {
    ("default", "trybuild"): True,
    ("default", "schema_helpers_ui"): False,
    ("ci", "trybuild"): False,
    ("ci", "schema_helpers_ui"): False,
    ("coverage", "trybuild"): True,
    ("coverage", "schema_helpers_ui"): True,
}


@pytest.mark.parametrize("profile", ["default", "ci", "coverage"], ids=str)
def test_every_nested_compiler_binary_is_allowed_the_longer_budget(
    nextest_profiles: dict[str, Profile], profile: str
) -> None:
    """Only a test that invokes nested Cargo needs the longer allowance.

    `trybuild` prepares its generated project, compiles fixtures and runs
    them inside the test process. `schema_helpers_ui` now compiles as an
    ordinary integration target before nextest starts, then only executes
    its accepted call sites under the base allowance.

    The nested-compiler inventory is source-discovered and pinned so a
    new trybuild harness cannot run under the ordinary allowance unnoticed.

    A profile that excludes the binary outright satisfies this too,
    since a binary that never runs needs no allowance. `default`
    excludes `trybuild` that way and runs `schema_helpers_ui`.
    """
    parsed = nextest_profiles[profile]
    binaries = compile_contract_binaries(REPOSITORY_ROOT / "tests")
    assert binaries == REQUIRED_NESTED_COMPILER_BINARIES, (
        "nested-compiler discovery must contain exactly the pinned "
        f"trybuild targets, got {sorted(binaries)}"
    )
    unbounded = binaries_short_of_allowance(parsed, binaries)
    assert not unbounded, (
        f"[profile.{profile}] runs these nested-compiler binaries without "
        f"an override allowing them {NESTED_COMPILER_ALLOWANCE_SECONDS:.0f}s: "
        f"{unbounded}"
    )


@pytest.mark.parametrize("profile", ["default", "ci", "coverage"], ids=str)
def test_every_compile_contract_binary_uses_the_serial_group(
    nextest_profiles: dict[str, Profile], profile: str
) -> None:
    """The serial group retains both required contract binaries.

    This inventory includes the schema-helper integration target even though
    it no longer invokes nested Cargo, so the scheduling contract stays fixed
    when compiler discovery changes.
    """
    binaries = REQUIRED_COMPILE_CONTRACT_BINARIES
    selected_profile = nextest_profiles[profile]
    overrides = (
        *selected_profile.overrides,
        *(table for _, table in selected_profile.inherited),
    )
    matching = [
        override
        for override in overrides
        if binaries_selected(override.get("filter")) == binaries
    ]
    assert matching, (
        f"[profile.{profile}] must inherit an override selecting every "
        f"required compile-contract binary {sorted(binaries)}"
    )
    misplaced = [
        override.get("test-group")
        for override in matching
        if override.get("test-group") != REQUIRED_COMPILE_CONTRACT_GROUP
    ]
    assert not misplaced, (
        f"[profile.{profile}] assigns required compile-contract binaries to "
        f"groups {misplaced}, not the serial group "
        f"{REQUIRED_COMPILE_CONTRACT_GROUP!r}"
    )


def test_nested_compiler_discovery_contains_only_trybuild() -> None:
    """The long-timeout inventory must not absorb ordinary test targets.

    Both required binaries remain in the separate contract inventory, but
    only ``trybuild`` constructs ``trybuild::TestCases`` and drives nested
    Cargo from the test process.
    """
    assert compile_contract_binaries(REPOSITORY_ROOT / "tests") == (
        REQUIRED_NESTED_COMPILER_BINARIES
    ), (
        "source discovery must find only the pinned nested-compiler target; "
        "the schema-helper contract is compiled with its ordinary target"
    )


def test_required_contract_inventory_keeps_both_targets() -> None:
    """Schema-helper execution stays required after leaving nested Cargo."""
    assert REQUIRED_COMPILE_CONTRACT_BINARIES == frozenset(
        {"trybuild", "schema_helpers_ui"}
    ), (
        "the required contract inventory must retain both binaries even though "
        "only trybuild invokes nested Cargo"
    )


def test_profile_exclusion_policy_covers_every_required_contract_binary() -> None:
    """Pin profile decisions for both required targets, including coverage."""
    profiles = {"default", "ci", "coverage"}
    expected = {
        (profile, binary)
        for profile in profiles
        for binary in REQUIRED_COMPILE_CONTRACT_BINARIES
    }
    assert set(REQUIRED_EXCLUSIONS) == expected, (
        "profile exclusion expectations must cover every required contract "
        "binary in default, ci and coverage"
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
