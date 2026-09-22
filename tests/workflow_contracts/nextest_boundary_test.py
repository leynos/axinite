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
a deliberately misspelled copy, which must be rejected.

The run happens in a copy of ``fixtures/nextest_boundary``, a crate
whose only contents are three empty test binaries. ``show-config``
resolves a profile's overrides against the binaries the package
declares and refuses a filterset naming one that does not exist, so the
fixture declares ``trybuild`` and ``schema_helpers_ui`` under those
names. Running it against the real crate instead would build the whole
test suite, which is minutes; the fixture builds in under a second.

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

import os
import re
import shutil
import subprocess
import tomllib
import typing as typ
from pathlib import Path

import pytest
from _workflow_policy import REPOSITORY_ROOT, load, workflow_paths
from contract_sources import read_source
from timeout_budgets import NEXTEST_CONFIG

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Sequence

#: The crate the configuration is loaded in. Three empty test binaries,
#: two of them named after the compile-contract binaries the real
#: overrides select, so the real filtersets resolve.
FIXTURE = REPOSITORY_ROOT / "tests" / "workflow_contracts" / "fixtures"
FIXTURE_CRATE = FIXTURE / "nextest_boundary"

#: What nextest prints, and does not fail on, when a key is not one it
#: knows. The run proceeds with that setting at its default, which is
#: why this contract treats the warning as a failure.
IGNORED_KEYS_WARNING: typ.Final[str] = "ignoring unknown configuration keys"

#: How long the whole subprocess may take, as a guard against a hung
#: `cargo` rather than as a budget. The fixture builds in about a second
#: and the terminated run ends in about one more; a loaded host makes
#: both slower, and nothing here is timed.
SUBPROCESS_TIMEOUT_SECONDS: typ.Final[float] = 600.0


#: The workflow that runs the contract suite, and so the one that must
#: install the runner these assertions hand the file to.
CONTRACT_SUITE_WORKFLOW: typ.Final[str] = "code_style.yml"

#: Stands for a lane that defers to ``CARGO_NEXTEST_VERSION`` and
#: declares it at neither scope. Reported rather than skipped: the
#: expression resolves to the empty string and the lane installs
#: whatever the tool action then picks.
_UNDECLARED: typ.Final[str] = "<undeclared>"


#: A ``cargo-nextest@<version>`` reference that pins a literal version.
#: Matched against the workflow's raw text rather than its parsed
#: document, because one lane pins the version inside a
#: ``setup-commands:`` block handed to a reusable workflow, where no
#: key names it and no ``env`` declares it. The character class stops
#: before ``$`` so an expression is not mistaken for a version.
_NEXTEST_LITERAL: typ.Final[re.Pattern[str]] = re.compile(
    r"cargo-nextest@([^\s$'\"]+)"
)

#: A ``cargo-nextest@`` reference that defers to ``CARGO_NEXTEST_VERSION``.
#: The variable may be declared at workflow or job scope, so the
#: reference is recognized here and resolved against both below.
_NEXTEST_DEFERRED: typ.Final[re.Pattern[str]] = re.compile(
    r"cargo-nextest@\$\{\{\s*env\.CARGO_NEXTEST_VERSION\s*\}\}"
)

#: Any ``cargo-nextest@`` reference at all, literal or deferred. What
#: makes a workflow one that installs the runner.
_NEXTEST_INSTALL: typ.Final[str] = "cargo-nextest@"


def _declared_env_versions(document: object) -> set[str]:
    """Return the ``CARGO_NEXTEST_VERSION`` values a workflow declares.

    Both scopes are read. Four lanes here declare it at workflow level
    and one inside its job, and a reading confined to the workflow block
    saw the job-scoped one as declaring nothing while its steps still
    installed the runner.

    Parameters
    ----------
    document
        A parsed workflow document.

    Returns
    -------
    set of str
        Every value declared, at either scope. Empty when none is.
    """
    if not isinstance(document, dict):
        return set()
    blocks = [document.get("env")]
    jobs = document.get("jobs")
    if isinstance(jobs, dict):
        blocks.extend(
            job.get("env") for job in jobs.values() if isinstance(job, dict)
        )
    return {
        str(block["CARGO_NEXTEST_VERSION"])
        for block in blocks
        if isinstance(block, dict) and "CARGO_NEXTEST_VERSION" in block
    }


def _installed_nextest_versions() -> dict[str, set[str]]:
    """Return the cargo-nextest versions each workflow installs.

    Every workflow that installs the runner is reported, not only those
    declaring ``CARGO_NEXTEST_VERSION``. Four lanes install it through
    the shared tool action and defer to that variable;
    ``mutation-testing.yml`` pins the version literally inside the
    ``setup-commands:`` it hands to the reusable mutation workflow. A
    reading confined to the ``env`` block saw four of the five, so the
    literal could drift to another release and the agreement assertion
    would still pass over the four that agreed with each other.

    A reference to the workflow's own ``env`` resolves to that block's
    value, so a lane deferring to a variable it never sets is reported
    as installing an unresolved version rather than as installing
    nothing.

    Returns
    -------
    dict of str to set of str
        Workflow file name to the versions it installs, for those that
        install the runner at all.
    """
    installed: dict[str, set[str]] = {}
    for path in workflow_paths():
        text = read_source(path)
        if _NEXTEST_INSTALL not in text:
            continue
        versions = set(_NEXTEST_LITERAL.findall(text))
        if _NEXTEST_DEFERRED.search(text):
            declared = _declared_env_versions(load(path))
            versions |= declared or {_UNDECLARED}
        installed[path.name] = versions
    return installed


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
    installed = _installed_nextest_versions()
    assert CONTRACT_SUITE_WORKFLOW in installed, (
        f"{CONTRACT_SUITE_WORKFLOW} runs the contract suite and installs no "
        f"cargo-nextest, so the runner these assertions use is whatever "
        f"happens to be on the image"
    )
    unresolved = {
        name: versions
        for name, versions in installed.items()
        if _UNDECLARED in versions
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


def _tool_is_present(command: str) -> bool:
    """Return whether a command resolves on `PATH`.

    Parameters
    ----------
    command
        The executable's name.

    Returns
    -------
    bool
        True when it resolves.
    """
    return shutil.which(command) is not None


def _run(
    arguments: "Sequence[str]", crate: Path, target: Path
) -> subprocess.CompletedProcess[str]:
    """Run a cargo command in the copied crate, writing under `target`.

    Both build directories are set explicitly rather than inherited.
    A developer's shell may point them at a shared tree, and a fixture
    that is meant to be built and discarded should not write there.

    Parameters
    ----------
    arguments
        The command and its arguments.
    crate
        The copied fixture crate to run in.
    target
        Where the fixture's build output goes.

    Returns
    -------
    subprocess.CompletedProcess
        The finished process, with its output captured as text.
    """
    environment = dict(os.environ)
    environment["CARGO_TARGET_DIR"] = str(target)
    environment["CARGO_BUILD_BUILD_DIR"] = str(target / "build")
    return subprocess.run(  # noqa: S603 - fixed argument vector, no shell
        list(arguments),
        cwd=crate,
        env=environment,
        capture_output=True,
        text=True,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
        check=False,
    )


class Fixture(typ.NamedTuple):
    """Where the copied fixture crate lives and where it builds.

    Attributes
    ----------
    crate
        The copy of ``fixtures/nextest_boundary`` this module runs in.
    target
        The build directory its output goes to.
    """

    crate: Path
    target: Path


@pytest.fixture(scope="module")
def fixture_crate(tmp_path_factory: pytest.TempPathFactory) -> Fixture:
    """Return the fixture crate copied outside the repository.

    Copied rather than built in place, because cargo discovers
    ``.cargo/config.toml`` by walking up the directory tree and does not
    stop at a workspace root. This repository's names the mold linker,
    which only the build lanes install, so a fixture built inside the
    tree fails to link in the lane that runs this suite and the failure
    reads as nextest refusing the configuration. Outside the tree there
    is no such file to find, which is what makes the fixture's verdict
    about the configuration and nothing else.

    Returns
    -------
    Fixture
        The copied crate and its build directory, made once per module.
    """
    root = tmp_path_factory.mktemp("nextest-boundary")
    crate = root / "crate"
    shutil.copytree(FIXTURE_CRATE, crate)
    return Fixture(crate=crate, target=root / "target")


@pytest.fixture(scope="module", autouse=True)
def _require_the_toolchain() -> None:
    """Fail rather than skip when cargo or nextest is missing.

    A skipped contract in CI is a contract that is not running, and this
    one exists because the reimplementation cannot answer the question
    by itself. ``make test`` already requires both commands.
    """
    missing = [
        tool for tool in ("cargo", "cargo-nextest") if not _tool_is_present(tool)
    ]
    assert not missing, (
        f"this contract loads the configuration with nextest itself and needs "
        f"{missing} on PATH; `make test` requires them too"
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
