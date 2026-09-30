"""Contract putting `.config/nextest.toml` to nextest itself.

Every other contract over that file reads it with `tomllib` and a
reimplementation of humantime's grammar. That is the right way to ask
what the file says, and it cannot ask whether the runner agrees. A key
this repository's reader accepts and nextest does not is the gap, and
nextest does not close it loudly: an unknown configuration key is a
*warning*, after which the run proceeds with that setting unset. A
misspelled ``grace_period`` therefore parses, pins, orders and passes
everywhere while the grace period in force is nextest's default.

So the file is handed to `cargo nextest` and the verdict is taken from
it. Two things are asserted: that nextest loads the real file for both
profiles without error, and that it reports no ignored key while doing
so. The second is the one that catches the misspelling, and it is
proved here rather than merely stated: the same reading is run against
a deliberately misspelled copy, which must warn while still loading.

The run happens in a copy of ``fixtures/nextest_boundary``. It declares
the two compile-contract binaries under the same names as the real crate,
and their tests share a marker file to detect cross-process overlap.
``show-config`` resolves a profile's overrides against the binaries the
package declares and refuses a filterset naming one that does not exist.

It is copied out of the tree rather than built in place because cargo
finds ``.cargo/config.toml`` by walking up the directory tree and does
not stop at a workspace root. This repository's names the mold linker,
which only the build lanes install, so a fixture built in place fails
to link in the lane that runs this suite, and the failure reads as
nextest refusing the configuration.

The third binary sleeps, and it is what makes the termination
assertion behavioural rather than another reading. nextest runs it
under a ``slow-timeout`` built from the real configuration's own
``terminate-after`` and ``grace-period`` with a one-second period
substituted, and the test is terminated. A real file that lost
``terminate-after`` would produce a configuration under which nextest
reports the test slow and lets it run to completion, and this assertion
is what fails then.

Requires `cargo` and `cargo-nextest`, which ``make test`` already
requires. Run via ``make test-workflow-contracts``.
"""

import tomllib
import typing as typ
from pathlib import Path

import pytest
from contract_sources import read_source
from nextest_versions import UNDECLARED, installed_versions_in, read_workflow_texts
from nextest_boundary_support import (
    Fixture,
    _require_the_toolchain,
    _run,
    fixture_crate,
)
from timeout_budgets import NEXTEST_CONFIG

#: What nextest prints, and does not fail on, when a key is not one it
#: knows. The run proceeds with that setting at its default, which is
#: why this contract treats the warning as a failure.
IGNORED_KEYS_WARNING: typ.Final[str] = "ignoring unknown configuration keys"

#: The workflow that runs the contract suite, and so the one that must
#: install the runner these assertions hand the file to.
CONTRACT_SUITE_WORKFLOW: typ.Final[str] = "code_style.yml"


def test_every_lane_installs_the_same_nextest() -> None:
    """A contract must answer for the runner the suite actually uses.

    These assertions take nextest's verdict on `.config/nextest.toml`,
    so the release they take it from has to be the release the coverage
    and test lanes run under. Two versions in the tree would leave the
    contract certifying a file for a runner nothing else loads, and the
    divergence would be invisible: both lanes would be green.

    The lane running the suite is named, not merely counted, because a
    tree where every version agreed by having none at all would satisfy
    a bare agreement check.
    """
    installed = installed_versions_in(read_workflow_texts())
    assert CONTRACT_SUITE_WORKFLOW in installed, (
        f"{CONTRACT_SUITE_WORKFLOW} runs the contract suite and installs no "
        f"cargo-nextest, so the runner these assertions use is whatever "
        f"happens to be on the image"
    )
    unresolved = {
        name: versions
        for name, versions in installed.items()
        if UNDECLARED in versions
    }
    assert not unresolved, (
        f"a lane defers to a cargo-nextest version it never declares "
        f"({unresolved}); the reference resolves to the empty string and the "
        f"lane installs whatever the tool action then picks"
    )
    versions = {version for versions in installed.values() for version in versions}
    assert len(versions) == 1, (
        f"the workflows install more than one cargo-nextest version "
        f"({installed}); the contract suite would then take its verdict from "
        f"a release the coverage, test or mutation lanes do not run"
    )


@pytest.mark.parametrize("profile", ["default", "ci"], ids=str)
def test_nextest_loads_the_real_configuration(
    fixture_crate: Fixture, profile: str
) -> None:
    """The runner accepts the file, for the profile a lane may select.

    A duration this repository's grammar accepts and humantime does not
    fails here, and so does a filterset naming a binary that no longer
    exists: nextest resolves overrides against the package's test
    binaries, which is why the fixture declares the two the real
    overrides name.
    """
    done = _run(
        (
            "cargo",
            "nextest",
            "show-config",
            "test-groups",
            "--config-file",
            str(NEXTEST_CONFIG),
            "--profile",
            profile,
        ),
        fixture_crate.crate,
        fixture_crate.target,
    )
    assert done.returncode == 0, (
        f"nextest refused {NEXTEST_CONFIG} under --profile {profile}; every "
        f"other contract over this file reads it with tomllib and cannot see "
        f"this:\n{done.stderr}"
    )


@pytest.mark.parametrize("profile", ["default", "ci"], ids=str)
def test_nextest_ignores_no_key_in_the_real_configuration(
    fixture_crate: Fixture, profile: str
) -> None:
    """A key nextest does not know is a setting that is not set.

    This is the failure the reading cannot reach. nextest warns and
    runs, so a misspelled `grace_period` leaves a file that parses,
    pins, orders and passes while the grace period in force is
    nextest's ten-second default. The exit status says nothing, so the
    warning is what is read.
    """
    done = _run(
        (
            "cargo",
            "nextest",
            "show-config",
            "test-groups",
            "--config-file",
            str(NEXTEST_CONFIG),
            "--profile",
            profile,
        ),
        fixture_crate.crate,
        fixture_crate.target,
    )
    assert IGNORED_KEYS_WARNING not in done.stderr, (
        f"nextest is ignoring keys in {NEXTEST_CONFIG} under --profile "
        f"{profile}, so those settings are at their defaults whatever the "
        f"file says:\n{done.stderr}"
    )


def test_the_ignored_key_reading_would_report_a_misspelling(
    fixture_crate: Fixture, tmp_path: Path
) -> None:
    """The assertion above passes on a file with nothing wrong with it.

    That is indistinguishable from a reading that reports nothing at
    all, and the exit status is 0 either way. So the same reading is
    run against the real file with every `grace-period` misspelled: it
    must report the warning, and nextest must still exit 0, which is
    the whole reason the warning rather than the status is read.
    """
    misspelled = tmp_path / "misspelled-nextest.toml"
    original = read_source(NEXTEST_CONFIG)
    mutated = original.replace("grace-period", "grace_period")
    assert mutated != original, (
        "the real configuration names no grace-period, so this proof would "
        "run against an unmutated copy and report nothing"
    )
    misspelled.write_text(mutated, encoding="utf-8")
    done = _run(
        (
            "cargo",
            "nextest",
            "show-config",
            "test-groups",
            "--config-file",
            str(misspelled),
            "--profile",
            "ci",
        ),
        fixture_crate.crate,
        fixture_crate.target,
    )
    assert done.returncode == 0, (
        f"nextest refused the misspelled copy outright, so the warning is no "
        f"longer what distinguishes it:\n{done.stderr}"
    )
    assert IGNORED_KEYS_WARNING in done.stderr, (
        f"nextest reported no ignored key for a file whose every "
        f"grace-period is misspelled, so the assertion above discriminates "
        f"nothing:\n{done.stderr}"
    )


def _short_slow_timeout() -> str:
    """Return the real base allowance with a one-second period.

    Built from the real configuration rather than written out, so the
    run below exercises the fields that file sets. A file that lost
    ``terminate-after`` yields a configuration under which nextest
    reports a test slow and lets it finish, and the assertion that
    consumes this is what fails then.

    Read through :func:`read_source` rather than with ``read_text``,
    so a configuration that cannot be read reports which file and why
    instead of raising a bare ``OSError`` from inside a helper whose
    name and return type promise a value. That is the same acquisition
    boundary the sweeps in this suite go through, and this module was
    reaching past it.

    Returns
    -------
    str
        A ``[profile.default]`` table as TOML text.

    Raises
    ------
    AssertionError
        If the real configuration declares no base ``slow-timeout``.
    SourceReadError
        If the configuration cannot be read or is not UTF-8.
    """
    parsed = tomllib.loads(read_source(NEXTEST_CONFIG))
    base = parsed["profile"]["default"]["slow-timeout"]
    assert isinstance(base, dict), (
        f"[profile.default].slow-timeout is {base!r}, not a table; this proof "
        f"substitutes its period and keeps its other fields"
    )
    fields = {**base, "period": "1s"}
    rendered = ", ".join(
        f'{key} = "{value}"' if isinstance(value, str) else f"{key} = {value}"
        for key, value in fields.items()
    )
    return f"[profile.default]\nslow-timeout = {{ {rendered} }}\n"


def test_nextest_terminates_a_test_under_these_fields(
    fixture_crate: Fixture, tmp_path: Path
) -> None:
    """The fields do not merely load; they stop a test.

    Every other assertion is about what a file says. This one runs a
    test that sleeps for thirty seconds under the real configuration's
    own ``terminate-after`` and ``grace-period``, with the period cut to
    a second, and requires nextest to terminate it.

    The distinction matters because the failure it guards against is
    silent in the other direction: without ``terminate-after`` nextest
    marks a test slow, prints a warning every period, and lets it run to
    completion, so the run passes and the suite is unbounded.
    """
    configuration = tmp_path / "short-nextest.toml"
    configuration.write_text(_short_slow_timeout(), encoding="utf-8")
    done = _run(
        (
            "cargo",
            "nextest",
            "run",
            "--config-file",
            str(configuration),
            # Named rather than left to the environment. `_run` passes the
            # inherited environment through, and nextest reads
            # `NEXTEST_PROFILE` when no profile is named: a developer or a
            # lane with that set to `ci` would select a profile this
            # temporary configuration does not declare, nextest would exit
            # before running anything, and the assertions below would read
            # a non-zero status and a missing timeout as the run having
            # failed for the reason they are about.
            "--profile",
            "default",
            "-E",
            "binary(slow)",
        ),
        fixture_crate.crate,
        fixture_crate.target,
    )
    assert done.returncode != 0, (
        f"nextest completed a thirty-second test under a one-second "
        f"allowance, so the fields the real configuration sets do not stop a "
        f"test:\n{done.stderr}"
    )
    assert "TIMEOUT" in done.stderr, (
        f"the run failed for some reason other than the timeout, so this "
        f"proves nothing about the allowance:\n{done.stderr}"
    )
