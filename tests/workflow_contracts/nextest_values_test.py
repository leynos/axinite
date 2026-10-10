"""Contract pinning the values `.config/nextest.toml` actually sets.

Separate from ``timeout_ordering_test``, whose subject is the ordering
of the tiers, and from ``compile_contract_budget_test``, whose subject
is which tests a per-test allowance was sized for. Neither can see a
value being deleted or changed, because both compare one figure against
another and every comparison still holds afterwards.

``grace-period`` is the clearest case. All three profiles and both
compile-contract overrides set five seconds, and ``termination_allowance``
reads whatever it finds, falling back to nextest's ten-second default. So
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
from nextest_config import Profile, binaries_selected
from timeout_budgets import base_slow_timeout, global_timeout

#: The profiles the configuration is allowed to declare. Pinned as a set
#: rather than iterated, because every assertion below is parametrized
#: over every required profile: an unpinned profile carrying looser
#: budgets would be selectable by `--profile` and examined by nothing.
REQUIRED_PROFILES: typ.Final[frozenset[str]] = frozenset(
    {"default", "ci", "coverage"}
)

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

#: The whole-run budget every profile declares, in seconds. Read through
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

#: The binaries the overrides must name. The default profile runs
#: ``schema_helpers_ui`` and ``ci`` also runs ``trybuild``. Coverage
#: inherits the ci override but filters both binaries out.
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


def test_the_configuration_declares_the_profiles_this_contract_pins(
    nextest_profiles: dict[str, Profile],
) -> None:
    """A profile nobody pinned is a profile nobody bounded.

    Every assertion below is parametrized over every required profile,
    so a profile with unpinned budgets cannot be selected unnoticed.
    Compared both ways, so a renamed profile fails here rather than
    turning the parametrized assertions into lookups of a missing name.
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
    """Pin override counts and values, including coverage inheritance.

    ``compile_contract_budget_test`` asks whether every compile-contract
    binary a profile runs is allowed *at least* the longer budget, which
    is the right shape for that question and says nothing about the
    value: an override raised to an hour would satisfy it while sitting
    above the 30 m whole-run budget for every test it matched, so the
    run would end before the allowance could be used.

    Default and ci each declare one override naming the compile-contract
    binaries, allowing exactly 900 s. They also carry the Postgres group's
    override, which names no binary and sets no allowance (pinned by
    ``test_each_profile_pins_its_postgres_group_override``). Coverage declares none; it inherits the ci
    override and filters the binaries out. The counts are pinned because a
    second matching override would change the value in force by file order.
    """
    own = nextest_profiles[profile].overrides
    contract_overrides = [
        entry for entry in own if binaries_selected(entry.get("filter"))
    ]
    expected_count = 0 if profile == "coverage" else 1
    assert len(contract_overrides) == expected_count, (
        f"[profile.{profile}] declares {len(contract_overrides)} overrides "
        f"selecting compile-contract binaries; this contract pins "
        f"{expected_count}, because a second matching the same binaries "
        f"would decide the allowance in force by file order"
    )
    if not contract_overrides:
        return
    override = contract_overrides[0]
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


@pytest.mark.parametrize("profile", ["default", "ci"], ids=str)
def test_each_profile_pins_its_postgres_group_override(
    nextest_profiles: dict[str, Profile], profile: str
) -> None:
    """Pin the Postgres override to a group assignment and nothing else.

    Default and ci each carry one override that puts the Postgres-backed tests
    in the ``pg-embed`` group, which bounds how many clusters' databases run at
    once. It must select by test name, not by binary, and set no
    ``slow-timeout``: either would let it decide the allowance a compile
    contract runs under, which the other override pins.
    """
    others = [
        entry
        for entry in nextest_profiles[profile].overrides
        if not binaries_selected(entry.get("filter"))
    ]
    assert len(others) == 1, (
        f"[profile.{profile}] declares {len(others)} overrides that select "
        "no compile-contract binary; this contract pins the one Postgres group"
    )
    (override,) = others
    assert override.get("test-group") == "pg-embed", (
        f"[profile.{profile}]'s other override assigns "
        f"{override.get('test-group')!r}, not the pg-embed group"
    )
    assert "slow-timeout" not in override, (
        f"[profile.{profile}]'s Postgres override sets a slow-timeout, which "
        "would compete with the compile-contract allowance"
    )
