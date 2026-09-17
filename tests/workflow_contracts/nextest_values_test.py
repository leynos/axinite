"""Contract pinning the values `.config/nextest.toml` actually sets.

Separate from ``timeout_ordering_test``, whose subject is the ordering
of the tiers, and from ``compile_contract_budget_test``, whose subject
is which tests a per-test allowance was sized for. Neither can see a
value being deleted or changed, because both compare one figure against
another and every comparison still holds afterwards.

``grace-period`` is the clearest case. Both profiles and both overrides
set five seconds, and ``termination_allowance`` reads whatever it finds,
falling back to nextest's ten-second default when it finds nothing. So
deleting every ``grace-period`` in the file *raises* the computed
requirement and leaves the ordering assertions passing, while the wait
between `SIGTERM` and `SIGKILL` silently doubles. The override's 900 s
is the next: ``binaries_short_of_allowance`` accepts anything at or
above the allowance, so an override raised to an hour would pass while
sitting above the 30 m whole-run budget for every test it matched. And
the whole-run budget itself is only ever compared with the largest
per-test allowance, so the documented thirty minutes could become forty
without a failure.

Pinned by value, therefore, not by relation: these are the figures
``docs/developers-guide.md`` records, and a change to any of them is a
change to that table.

Run via ``make test-workflow-contracts``.
"""

import typing as typ

import pytest
from nextest_config import Profile, binaries_selected, profiles
from timeout_budgets import NEXTEST_CONFIG, base_slow_timeout, global_timeout

#: The profiles the configuration is allowed to declare. Pinned as a set
#: rather than iterated, because every assertion below is parametrized
#: over these two names: a third profile carrying looser budgets would
#: be selectable by `--profile` and examined by nothing here.
REQUIRED_PROFILES: typ.Final[frozenset[str]] = frozenset({"default", "ci"})

#: Each profile's own ``slow-timeout``, field by field, as the
#: developers' guide records it. Compared as a whole table so a field
#: added to it fails too; an unrecognized field is not inert, because
#: nextest ignores an unknown key with a warning rather than refusing
#: the file.
REQUIRED_BASE_SLOW_TIMEOUT: typ.Final[dict[str, str]] = {
    "period": "300s",
    "terminate-after": "1",
    "grace-period": "5s",
}

#: The whole-run budget both profiles declare, in seconds. Read through
#: the duration grammar rather than matched as text, so the assertion is
#: about the thirty minutes the guide documents and not about which of
#: the spellings of thirty minutes the file happens to use.
REQUIRED_GLOBAL_TIMEOUT_SECONDS: typ.Final[float] = 30 * 60.0

#: The compile-contract override's ``slow-timeout``, field by field.
#: ``compile_contract_budget_test`` asserts the allowance is *at least*
#: 900 s, which is the right shape for that question and leaves an
#: override raised past the whole-run budget passing.
REQUIRED_OVERRIDE_SLOW_TIMEOUT: typ.Final[dict[str, str]] = {
    "period": "900s",
    "terminate-after": "1",
    "grace-period": "5s",
}

#: The binaries that override must name. Both profiles run
#: ``schema_helpers_ui`` and ``ci`` also runs ``trybuild``, and the
#: override carries both in each profile so that neither depends on
#: which profile a lane selects.
REQUIRED_OVERRIDE_BINARIES: typ.Final[frozenset[str]] = frozenset(
    {"trybuild", "schema_helpers_ui"}
)


def _fields(table: object) -> dict[str, str]:
    """Return a parsed table's entries as text, keyed by name.

    Parameters
    ----------
    table
        A parsed ``slow-timeout`` value, which need not be a table.

    Returns
    -------
    dict of str to str
        Every entry as text, empty when the value is not a table.
    """
    match table:
        case dict():
            return {str(key): str(value) for key, value in table.items()}
        case _:
            return {}


@pytest.fixture(scope="module")
def nextest_profiles() -> dict[str, Profile]:
    """Return each nextest profile the configuration declares.

    Returns
    -------
    dict[str, Profile]
        Profile name to its table and overrides.
    """
    return profiles(NEXTEST_CONFIG.read_text(encoding="utf-8"))


def test_the_configuration_declares_the_profiles_this_contract_pins(
    nextest_profiles: dict[str, Profile],
) -> None:
    """A profile nobody pinned is a profile nobody bounded.

    Every assertion below is parametrized over ``default`` and ``ci``,
    so a third profile would be selectable by ``--profile`` and read by
    none of them. Compared both ways, so a renamed profile fails here
    rather than turning the parametrized assertions into lookups of a
    name that no longer exists.
    """
    assert set(nextest_profiles) == set(REQUIRED_PROFILES), (
        f"the configuration declares {sorted(nextest_profiles)}, not "
        f"{sorted(REQUIRED_PROFILES)}; a profile with no entry here is a "
        f"profile whose budgets nobody has pinned, and `--profile` can select "
        f"it"
    )


@pytest.mark.parametrize("profile", sorted(REQUIRED_PROFILES), ids=str)
def test_each_profile_pins_its_base_slow_timeout_field_by_field(
    nextest_profiles: dict[str, Profile], profile: str
) -> None:
    """The base allowance is three values, and only two were asserted.

    ``timeout_ordering_test`` reads ``period`` and ``terminate-after``
    because those two make up the per-test budget it compares. The
    third, ``grace-period``, is what nextest waits between `SIGTERM` and
    `SIGKILL`, and nothing compared it: deleting it from every table
    leaves that test passing, leaves ``termination_allowance`` computing
    a *larger* requirement from nextest's ten-second default, and
    doubles the wait a terminated test actually gets.

    Compared as a whole table rather than field by field, so a field
    added here fails as well as one removed. An unrecognized field is
    not inert: nextest ignores an unknown configuration key with a
    warning and runs anyway, so a misspelling is a silently unset value.
    """
    base = base_slow_timeout(nextest_profiles[profile])
    assert base == REQUIRED_BASE_SLOW_TIMEOUT, (
        f"[profile.{profile}]'s base slow-timeout is {base}, not "
        f"{REQUIRED_BASE_SLOW_TIMEOUT}; these are the figures the "
        f"developers' guide records, and changing one changes that table"
    )


@pytest.mark.parametrize("profile", sorted(REQUIRED_PROFILES), ids=str)
def test_each_profile_pins_the_documented_whole_run_budget(
    nextest_profiles: dict[str, Profile], profile: str
) -> None:
    """Thirty minutes is a documented figure, not merely a large one.

    The ordering assertion requires the whole-run budget to sit above
    the largest per-test allowance, which 40 m and 60 m satisfy as well
    as 30 m does. The guide's table says thirty minutes and says why:
    above the largest per-test allowance in either profile, and twice
    the longest instrumented run measured. A budget that drifted from
    it would leave the guide describing a value the runner does not use.
    """
    budget = global_timeout(nextest_profiles[profile])
    assert budget == REQUIRED_GLOBAL_TIMEOUT_SECONDS, (
        f"[profile.{profile}]'s global-timeout is {budget:.0f}s, not the "
        f"{REQUIRED_GLOBAL_TIMEOUT_SECONDS:.0f}s the developers' guide "
        f"records"
    )


@pytest.mark.parametrize("profile", sorted(REQUIRED_PROFILES), ids=str)
def test_each_profile_pins_its_compile_contract_override(
    nextest_profiles: dict[str, Profile], profile: str
) -> None:
    """One override, naming both binaries, allowing exactly 900 s.

    ``compile_contract_budget_test`` asks whether every compile-contract
    binary a profile runs is allowed *at least* the longer budget, which
    is the right shape for that question and says nothing about the
    value: an override raised to an hour would satisfy it while sitting
    above the 30 m whole-run budget for every test it matched, so the
    run would end before the allowance could be used.

    The count is pinned too. A second override matching the same
    binaries would be consulted ahead of or behind this one depending on
    file order, and the allowance in force would then depend on a line
    nobody had compared.
    """
    own = nextest_profiles[profile].overrides
    assert len(own) == 1, (
        f"[profile.{profile}] declares {len(own)} overrides; this contract "
        f"pins one, because a second matching the same binaries would decide "
        f"the allowance in force by file order"
    )
    override = own[0]
    selected = binaries_selected(override.get("filter"))
    assert selected == REQUIRED_OVERRIDE_BINARIES, (
        f"[profile.{profile}]'s override selects binaries {sorted(selected)}, "
        f"not {sorted(REQUIRED_OVERRIDE_BINARIES)}; a binary dropped from the "
        f"filterset, or negated inside it, falls back to the base allowance "
        f"sized for the ordinary tests"
    )
    fields = _fields(override.get("slow-timeout"))
    assert fields == REQUIRED_OVERRIDE_SLOW_TIMEOUT, (
        f"[profile.{profile}]'s override slow-timeout is {fields}, not "
        f"{REQUIRED_OVERRIDE_SLOW_TIMEOUT}; the guide records 900 s, and a "
        f"larger one would sit above the whole-run budget that contains it"
    )
