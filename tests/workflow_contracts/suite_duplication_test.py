"""Contracts against running one test suite twice for the same commit.

Nothing fails when a suite runs twice. Both runs pass, both report, and the
only evidence is the bill and the wait. Axinite paid that twice over: on a
pull request `test.yml`'s libsql-only leg compiled and ran the workspace under
exactly the flags `codescene-coverage.yml` was already running it under, and
on a push to `main` all three `test.yml` legs repeated what `coverage.yml` had
just done with instrumentation. The GitHub tool crate's suite was worse: it
rode along inside `make test`, so it ran once per leg, three times a trigger.

The rule enforced here is therefore about work rather than about job names: on
a trigger a developer waits for, no two lanes may run the same suite over the
same feature set under the same nextest profile. Feature sets are compared as
sets, so a leg rewritten from `--features a,b` to `--features a --features b`
is still the same run, and `--all-features` stays distinct from a list that
happens to name every feature today, because tomorrow it will not. The profile
is part of the identity because it selects which tests run at all: the default
profile drops the trybuild compile contracts, so a default-profile lane does
not stand in for a `ci` one, however identical the flags.

Only `pull_request` and `push` are in scope. Cron work runs GitHub-hosted,
where this repository pays nothing and nobody is waiting, so duplication there
is not what this contract is for.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import shlex
import typing as typ
from dataclasses import dataclass

import pytest
import yaml
from _workflow_policy import (
    DIST_GENERATED,
    REPOSITORY_ROOT,
    Job,
    jobs_of,
    load,
    matrix_legs,
    runs_on_event,
    step_text,
    triggers,
    workflow_paths,
)

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator
    from pathlib import Path

#: The triggers a developer waits on, and the only ones that reach a paid
#: runner. A scheduled duplicate is free and blocks nobody.
PAID_EVENTS: tuple[str, ...] = ("pull_request", "push")

#: The scope a run covers. Two runs only collide when they cover the same
#: scope: the workspace suite and an out-of-workspace crate's suite share a
#: command shape and nothing else.
WORKSPACE = "workspace"
GITHUB_TOOL_MANIFEST = "tools-src/github/Cargo.toml"
GITHUB_TOOL_SCOPE = f"crate:{GITHUB_TOOL_MANIFEST}"

#: What each Make target runs, and whether the features it runs under come
#: from the step's `TEST_FEATURES`. A workflow step says `make test-workspace`
#: and only the Makefile says what that is; the mapping is asserted against it
#: below rather than assumed. `make test` is here because it is what the
#: `tests` legs used to call, and a contract that could not read it would have
#: reported the duplication it caused as nothing at all.
MAKE_TARGETS: dict[str, tuple[tuple[str, bool], ...]] = {
    "test": ((WORKSPACE, True), (GITHUB_TOOL_SCOPE, False)),
    "test-workspace": ((WORKSPACE, True),),
    "test-github-tool": ((GITHUB_TOOL_SCOPE, False),),
}

#: The command each Make target must still run for the mapping above to hold.
WORKSPACE_RECIPE = "$(NEXTEST) run --workspace $(TEST_FEATURES)"
GITHUB_TOOL_RECIPE = "$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)"

#: The variable a workflow step uses to hand feature flags to a Make target.
#: The flags arrive as one shell word, `TEST_FEATURES="--features x"`, so the
#: selection is inside a token rather than beside it.
FEATURE_VARIABLE = "TEST_FEATURES"

#: The variable a workflow step uses to choose the nextest profile for a Make
#: target, and the profile the Makefile falls back to without it.
PROFILE_VARIABLE = "NEXTEST_PROFILE"
DEFAULT_PROFILE = "default"
MAKEFILE_PROFILE_DEFAULT = f"{PROFILE_VARIABLE} ?= {DEFAULT_PROFILE}"

#: Cargo commands that run a suite. The instrumented form counts:
#: `cargo llvm-cov nextest` compiles and runs the same tests as
#: `cargo nextest run`, which is the whole reason a coverage lane can stand in
#: for a test lane. `cargo llvm-cov clean` and `cargo llvm-cov report` are
#: excluded by requiring `nextest` in the same command.
CARGO_COMMANDS: tuple[re.Pattern[str], ...] = (
    re.compile(r"\bcargo\s+(?:\+\S+\s+)?llvm-cov\s+nextest\b(?P<args>[^\n]*)"),
    re.compile(r"\bcargo\s+(?:\+\S+\s+)?nextest\s+run\b(?P<args>[^\n]*)"),
    re.compile(r"\bcargo\s+(?:\+\S+\s+)?test\b(?P<args>[^\n]*)"),
)

#: A Make invocation of one of the targets above. The negative lookahead stops
#: `test` matching the first half of `test-workflow-contracts`, which is a
#: PyYAML parse rather than a suite.
MAKE_COMMAND: re.Pattern[str] = re.compile(
    r"\bmake\s+(?P<target>" + "|".join(sorted(MAKE_TARGETS, key=len, reverse=True))
    + r")\b(?!-)(?P<args>[^\n]*)"
)

#: Where a Cargo command names the crate it runs against.
MANIFEST_RE: re.Pattern[str] = re.compile(
    r"--manifest-path[= ](?P<path>\S+)"
)

#: Sentinels for the flags that select features without naming any. They are
#: part of the key so that `--all-features` and `--no-default-features
#: --features libsql` cannot collide with each other or with a feature list.
ALL_FEATURES = ":all-features"
NO_DEFAULT_FEATURES = ":no-default-features"

#: `${{ matrix.<key> }}`, which is how a leg's flags reach the command.
MATRIX_REFERENCE_RE: re.Pattern[str] = re.compile(
    r"\$\{\{\s*matrix\.(?P<key>[A-Za-z0-9_-]+)\s*\}\}"
)


@dataclass(frozen=True)
class SuiteRun:
    """One execution of the workspace suite, with the features it selects.

    Attributes
    ----------
    job
        The job that runs it.
    leg
        The matrix leg's name, or an empty string when the job has no matrix.
    scope
        What the run covers: the workspace, or one out-of-workspace crate.
    profile
        The nextest profile, which decides which of the suite's tests run.
    features
        The feature selection, as a set, so that two spellings of one
        selection compare equal.
    """

    job: Job
    leg: str
    scope: str
    profile: str
    features: frozenset[str]

    @property
    def key(self) -> tuple[str, str, frozenset[str]]:
        """Return what makes two runs the same work."""
        return self.scope, self.profile, self.features

    def __str__(self) -> str:
        """Identify the run in assertion output."""
        return f"{self.job} ({self.leg})" if self.leg else str(self.job)


def _feature_key(args: str) -> frozenset[str]:
    """Return the feature selection a command's arguments make.

    Parameters
    ----------
    args
        The command's arguments, with every matrix reference already
        substituted.

    Returns
    -------
    frozenset of str
        Each named feature, plus a sentinel for `--all-features` and for
        `--no-default-features`. Everything else is ignored: a profile, an
        output path or a test filter changes how the run is reported, not
        which tests it compiles and executes.
    """
    selected: set[str] = set()
    tokens = shlex.split(args, comments=False, posix=True)
    # A `make` step passes the selection as one assignment word. Unpacking it
    # here, rather than at the call site, means the two command shapes are
    # keyed the same way and a coverage lane can be compared with a test lane.
    tokens = [
        part
        for token in tokens
        for part in (
            shlex.split(token.partition("=")[2])
            if token.startswith(f"{FEATURE_VARIABLE}=")
            else [token]
        )
    ]
    for index, token in enumerate(tokens):
        if token == "--all-features":
            selected.add(ALL_FEATURES)
        elif token == "--no-default-features":
            selected.add(NO_DEFAULT_FEATURES)
        elif token == "--features" and index + 1 < len(tokens):
            selected.update(tokens[index + 1].split(","))
        elif token.startswith("--features="):
            selected.update(token.partition("=")[2].split(","))
    return frozenset(name for name in selected if name)


def _profile(args: str) -> str:
    """Return the nextest profile a command's arguments select.

    Parameters
    ----------
    args
        The command's arguments, with every matrix reference already
        substituted. Both spellings are read: `--profile ci` on a Cargo
        command, and `NEXTEST_PROFILE=ci` on a Make target.

    Returns
    -------
    str
        The profile name, or the nextest default when the command names none.
    """
    tokens = shlex.split(args, comments=False, posix=True)
    for index, token in enumerate(tokens):
        if token == "--profile" and index + 1 < len(tokens):
            return tokens[index + 1]
        if token.startswith("--profile="):
            return token.partition("=")[2]
        if token.startswith(f"{PROFILE_VARIABLE}="):
            return token.partition("=")[2]
    return DEFAULT_PROFILE


def _substitute(text: str, leg: dict[str, str]) -> str:
    """Return a step's script with this leg's matrix values in place.

    An unresolved reference is left as it stands rather than dropped, so a
    typo in a matrix key shows up in the key instead of quietly widening two
    different runs into one.
    """
    return MATRIX_REFERENCE_RE.sub(
        lambda match: leg.get(match["key"], match[0]), text
    )


def _cargo_runs(script: str) -> Iterator[tuple[str, str, frozenset[str]]]:
    """Yield the scope and features of each Cargo suite command in a script.

    A command that names neither a manifest nor `--workspace` is skipped. It
    is a filtered or single-crate run, such as the WIT instantiation test, and
    counting it as the workspace suite would report a clash with a lane that
    runs thousands of tests it does not.
    """
    for pattern in CARGO_COMMANDS:
        for match in pattern.finditer(script):
            args = match["args"]
            manifest = MANIFEST_RE.search(args)
            if manifest is not None:
                yield f"crate:{manifest['path']}", _profile(args), _feature_key(args)
            elif "--workspace" in args:
                yield WORKSPACE, _profile(args), _feature_key(args)


def _make_runs(script: str) -> Iterator[tuple[str, str, frozenset[str]]]:
    """Yield the scope, profile and features of each Make target in a script.

    Only the workspace half reads the step's variables. `make test-github-tool`
    runs plain `cargo test` over one crate: it takes no features and no
    nextest profile, so giving it the step's would invent a difference between
    two identical runs.
    """
    for match in MAKE_COMMAND.finditer(script):
        for scope, takes_features in MAKE_TARGETS[match["target"]]:
            if takes_features:
                yield scope, _profile(match["args"]), _feature_key(match["args"])
            else:
                yield scope, DEFAULT_PROFILE, frozenset()


def _suite_runs_in(job: Job, event: str) -> Iterator[SuiteRun]:
    """Yield every suite run a job performs on an event."""
    for leg in matrix_legs(job, event):
        for step in job.steps:
            script = _substitute(step_text(step), leg)
            for scope, profile, features in (
                *_cargo_runs(script),
                *_make_runs(script),
            ):
                yield SuiteRun(
                    job=job,
                    leg=leg.get("name", ""),
                    scope=scope,
                    profile=profile,
                    features=features,
                )


def suite_runs_for(
    documents: dict[str, dict[str, object]], event: str
) -> list[SuiteRun]:
    """Return every workspace-suite run an event dispatches.

    Parameters
    ----------
    documents
        Parsed workflows, keyed by file name.
    event
        The trigger to resolve against: a workflow that does not declare it,
        and a job whose guard excludes it, contribute nothing.

    Returns
    -------
    list of SuiteRun
        One entry per leg per suite command, in workflow order.
    """
    found: list[SuiteRun] = []
    for name, document in documents.items():
        if event not in triggers(document):
            continue
        for job in jobs_of(name, document):
            if not runs_on_event(job, event):
                continue
            found.extend(_suite_runs_in(job, event))
    return found


def duplicates_in(
    runs: list[SuiteRun],
) -> dict[tuple[str, str, frozenset[str]], list[SuiteRun]]:
    """Return the work more than one run covers."""
    by_key: dict[tuple[str, str, frozenset[str]], list[SuiteRun]] = {}
    for run in runs:
        by_key.setdefault(run.key, []).append(run)
    return {key: found for key, found in by_key.items() if len(found) > 1}


def _estate() -> dict[str, dict[str, object]]:
    """Return every workflow this contract judges, parsed and keyed by name."""
    return {
        path.name: load(path)
        for path in workflow_paths()
        if path.name != DIST_GENERATED
    }


ESTATE = _estate()


def test_the_scan_finds_the_suite_at_all() -> None:
    """Guard against a selector that silently matches nothing.

    Every assertion below is satisfied by finding no runs, so the scan's own
    reach is asserted first. Two is the floor a working estate cannot go
    under: a pull request runs the suite, and so does a push.
    """
    for event in PAID_EVENTS:
        assert len(suite_runs_for(ESTATE, event)) >= 2, (
            f"no workspace suite runs were found for {event}; the contract "
            "below would pass with the suite deleted"
        )


def _make_rule(name: str) -> tuple[tuple[str, ...], tuple[str, ...]]:
    """Return one Makefile rule's prerequisites and recipe lines.

    Parameters
    ----------
    name
        The target to read.

    Returns
    -------
    tuple of tuple of str
        The prerequisites, then the recipe lines with their leading tab
        stripped.

    Raises
    ------
    AssertionError
        If the Makefile no longer declares the target. Every mapping in this
        module rests on it, so its absence has to stop the run rather than
        quietly answer "no commands".
    """
    makefile = (REPOSITORY_ROOT / "Makefile").read_text(encoding="utf-8")
    rule = re.search(
        rf"^{re.escape(name)}:(?P<prerequisites>[^\n]*)\n(?P<recipe>(?:\t[^\n]*\n)*)",
        makefile,
        re.MULTILINE,
    )
    assert rule is not None, f"the Makefile no longer defines the {name!r} target"
    return (
        tuple(rule["prerequisites"].split()),
        tuple(line.lstrip("\t") for line in rule["recipe"].splitlines()),
    )


def test_make_test_is_both_halves_and_nothing_else() -> None:
    """`make test` must remain exactly the two lanes CI runs separately.

    A developer runs one target; CI runs the halves on different lanes and
    different triggers. If the whole stopped being the sum of the parts, the
    local command and the gate would test different things, and this module
    would still read `make test` as covering both.
    """
    prerequisites, recipe = _make_rule("test")
    assert set(prerequisites) == {"test-workspace", "test-github-tool"}, (
        f"`make test` builds {prerequisites}, which is no longer the two "
        "lanes CI runs; a developer and the gate would test different things"
    )
    assert not recipe, (
        f"`make test` has grown a recipe of its own ({recipe}), so it runs "
        "work neither lane runs and nothing in CI covers it"
    )


def test_the_make_targets_still_run_what_this_module_reads_them_as() -> None:
    """Tie each target name in a workflow to what the Makefile makes it do.

    A workflow step says `make test-workspace`. If that target stopped running
    the workspace suite, or started running the GitHub tool crate again, every
    judgement here about what a lane executes would be wrong while still
    reading correctly.
    """
    _, workspace = _make_rule("test-workspace")
    assert any(line.startswith(WORKSPACE_RECIPE) for line in workspace), (
        f"`make test-workspace` no longer runs {WORKSPACE_RECIPE!r}, so the "
        "workflow steps that call it do not run what this contract reads "
        f"them as running. Its recipe is {workspace}."
    )
    assert not any("--manifest-path" in line for line in workspace), (
        "`make test-workspace` builds or tests an out-of-workspace crate, "
        "which is the duplication this split removed: the GitHub tool crate "
        "has its own lane, and running it here runs it once per leg again"
    )
    _, tool = _make_rule("test-github-tool")
    assert tool == (GITHUB_TOOL_RECIPE,), (
        f"`make test-github-tool` runs {tool}, not {(GITHUB_TOOL_RECIPE,)}; "
        "the scope and the empty feature set this module gives it both come "
        "from that one command"
    )
    makefile = (REPOSITORY_ROOT / "Makefile").read_text(encoding="utf-8")
    assert MAKEFILE_PROFILE_DEFAULT in makefile, (
        f"the Makefile no longer falls back to the {DEFAULT_PROFILE!r} nextest "
        "profile, so a step that names no profile does not run the tests this "
        "module reads it as running"
    )
    assert f"GITHUB_TOOL_MANIFEST := {GITHUB_TOOL_MANIFEST}\n" in makefile, (
        f"the GitHub tool manifest is no longer {GITHUB_TOOL_MANIFEST}, so "
        "its lane and a Cargo command naming the same crate would be read as "
        "two different scopes and never compared"
    )


#: Every suite this repository has, and the scope each covers. The workspace
#: leaves two crates out, and `--workspace` cannot reach either, so each needs
#: a lane of its own or it is not tested at all.
EXPECTED_SCOPES: frozenset[str] = frozenset(
    {WORKSPACE, GITHUB_TOOL_SCOPE, "crate:channels-src/telegram/Cargo.toml"}
)


@pytest.mark.parametrize("event", PAID_EVENTS)
def test_every_suite_still_runs_on_every_paid_trigger(event: str) -> None:
    """The other half of de-duplication: nothing may go missing instead.

    Removing a duplicate lane and removing the only lane look identical in a
    diff and identical in a green run. This names the suites, so a lane
    deleted rather than de-duplicated fails here, and a suite added without a
    lane on one trigger fails here too.
    """
    found = {run.scope for run in suite_runs_for(ESTATE, event)}
    assert found == EXPECTED_SCOPES, (
        f"on {event} the suites that run are {sorted(found)}, not "
        f"{sorted(EXPECTED_SCOPES)}. A suite with no lane on a trigger is not "
        "tested there, whatever the gate reports."
    )


#: The profile that runs every test the suite has. Anything narrower leaves
#: the trybuild compile contracts unexecuted.
FULL_PROFILE = "ci"


@pytest.mark.parametrize("event", PAID_EVENTS)
def test_every_trigger_runs_the_workspace_suite_in_full(event: str) -> None:
    """A lane that replaces another must not be narrower than it was.

    De-duplication removes the second run of a suite, so whichever run is left
    has to be the whole of it. The default nextest profile drops the trybuild
    compile contracts, and a lane running it looks in every other respect like
    the lane it replaced.
    """
    profiles = {
        run.profile for run in suite_runs_for(ESTATE, event) if run.scope == WORKSPACE
    }
    assert profiles == {FULL_PROFILE}, (
        f"on {event} the workspace suite runs under {sorted(profiles)}, not "
        f"only {FULL_PROFILE!r}. A lane on a narrower profile runs fewer "
        "tests than the lane it stands in for, and nothing else on this "
        "trigger makes up the difference."
    )


@pytest.mark.parametrize("event", PAID_EVENTS)
def test_no_trigger_runs_the_same_suite_twice(event: str) -> None:
    """The contract itself: one feature selection, one run, per trigger."""
    duplicated = duplicates_in(suite_runs_for(ESTATE, event))
    assert not duplicated, "\n".join(
        f"on {event}, {scope} under {sorted(features) or 'no features'} and "
        f"the {profile} profile is run by "
        + ", ".join(str(run) for run in runs)
        for (scope, profile, features), runs in duplicated.items()
    )


class TestFeatureKey:
    """The key has to see through spelling without merging real differences."""

    @pytest.mark.parametrize(
        ("left", "right"),
        [
            ("--features a,b --workspace", "--features a --features b"),
            (
                "--no-default-features --features libsql --features test-helpers",
                "--features test-helpers --no-default-features --features libsql",
            ),
            (
                "--features test-helpers --workspace --lcov --output-path x",
                "--features test-helpers --profile ci",
            ),
        ],
        ids=["comma-or-repeated", "order", "reporting-flags"],
    )
    def test_it_ignores_what_does_not_change_the_run(
        self, left: str, right: str
    ) -> None:
        """Two spellings of one selection are one run, however written."""
        assert _feature_key(left) == _feature_key(right)

    @pytest.mark.parametrize(
        ("left", "right"),
        [
            ("--all-features", "--features a,b"),
            ("--features libsql", "--no-default-features --features libsql"),
            ("--features a,b", "--features a"),
        ],
        ids=["all-features-is-not-a-list", "default-features", "subset"],
    )
    def test_it_keeps_different_selections_apart(self, left: str, right: str) -> None:
        """A key that merged these would report a duplicate that is not one.

        This is the half that makes the contract narrow. Without it, a key
        that returned a constant would satisfy every equality above and
        condemn the whole estate as duplicated.
        """
        assert _feature_key(left) != _feature_key(right)


class TestProfile:
    """The profile decides which tests run, so it decides identity."""

    @pytest.mark.parametrize(
        ("args", "expected"),
        [
            ("--workspace --lcav", DEFAULT_PROFILE),
            ("--workspace --profile ci", "ci"),
            ("--workspace --profile=ci", "ci"),
            ("NEXTEST_PROFILE=ci TEST_FEATURES=\"--features x\"", "ci"),
            ("TEST_FEATURES=\"--features x\"", DEFAULT_PROFILE),
        ],
        ids=["absent", "cargo", "cargo-equals", "make", "make-absent"],
    )
    def test_it_reads_every_spelling(self, args: str, expected: str) -> None:
        """A profile the reader misses defaults, and defaults compare equal.

        That is the direction that matters: two lanes would then look like one
        run when the tests they execute differ by the whole trybuild set.
        """
        assert _profile(args) == expected


class TestDuplicateDetection:
    """The detector must fire on the shape it was written for, and only that."""

    @staticmethod
    def _workflow(event: str, *jobs_yaml: str) -> dict[str, object]:
        """Return a parsed one-workflow fixture triggered by one event."""
        body = "\n".join(jobs_yaml)
        return typ.cast(
            "dict[str, object]",
            yaml.safe_load(f'"on":\n  {event}:\n    branches: [main]\njobs:\n{body}'),
        )

    _TESTS = (
        "  tests:\n"
        "    runs-on: ubicloud-standard-4\n"
        "    steps:\n"
        "      - run: cargo nextest run --workspace --features test-helpers\n"
    )
    _COVERAGE = (
        "  coverage:\n"
        "    runs-on: ubicloud-standard-4\n"
        "    steps:\n"
        "      - run: cargo llvm-cov nextest --features test-helpers"
        " --workspace --lcov\n"
    )

    def test_it_reports_a_coverage_lane_repeating_a_test_lane(self) -> None:
        """The exact shape removed from `test.yml` must not pass unseen."""
        estate = {"one.yml": self._workflow("pull_request", self._TESTS, self._COVERAGE)}
        duplicated = duplicates_in(suite_runs_for(estate, "pull_request"))
        assert len(duplicated) == 1
        assert sorted(str(run) for run in next(iter(duplicated.values()))) == [
            "one.yml:coverage",
            "one.yml:tests",
        ]

    def test_it_reports_one_lane_running_the_same_suite_per_leg(self) -> None:
        """The GitHub tool crate's old shape: one command, three legs."""
        estate = {
            "one.yml": self._workflow(
                "pull_request",
                "  tests:\n"
                "    runs-on: ubicloud-standard-4\n"
                "    strategy:\n"
                "      matrix:\n"
                "        include:\n"
                "          - name: a\n"
                "            flags: --features x\n"
                "          - name: b\n"
                "            flags: --features y\n"
                "    steps:\n"
                "      - run: cargo nextest run --workspace --features test-helpers\n",
            )
        }
        duplicated = duplicates_in(suite_runs_for(estate, "pull_request"))
        assert len(duplicated) == 1, (
            "a command that ignores the matrix runs the identical suite once "
            "per leg, which is duplication inside a single job"
        )

    def test_a_guard_that_excludes_the_event_removes_the_duplicate(self) -> None:
        """A job the trigger cannot dispatch costs nothing and is not a clash.

        This is the assertion that makes the fix visible: the same pair of
        lanes, with the guard `test.yml` now carries, is not a duplicate.
        """
        guarded = self._TESTS.replace(
            "    runs-on:", "    if: github.event_name != 'pull_request'\n    runs-on:"
        )
        estate = {"one.yml": self._workflow("pull_request", guarded, self._COVERAGE)}
        assert not duplicates_in(suite_runs_for(estate, "pull_request"))

    def test_different_feature_sets_are_not_a_duplicate(self) -> None:
        """Two lanes running different suites are the normal case."""
        other = self._COVERAGE.replace("--features test-helpers", "--all-features")
        estate = {"one.yml": self._workflow("pull_request", self._TESTS, other)}
        assert not duplicates_in(suite_runs_for(estate, "pull_request"))

    def test_a_different_profile_is_not_a_duplicate(self) -> None:
        """The same flags under a narrower profile run a different suite.

        This is what made the coverage lanes stop short of replacing the test
        legs: they ran the default profile, which drops the trybuild compile
        contracts, so standing the legs down would have left those unexecuted.
        """
        narrower = self._TESTS.replace(
            "cargo nextest run --workspace", "cargo nextest run --profile ci --workspace"
        )
        estate = {"one.yml": self._workflow("pull_request", narrower, self._COVERAGE)}
        assert not duplicates_in(suite_runs_for(estate, "pull_request"))

    def test_a_workflow_the_trigger_does_not_declare_contributes_nothing(self) -> None:
        """`coverage.yml` runs on a push only; it cannot clash on a pull."""
        estate = {
            "a.yml": self._workflow("pull_request", self._TESTS),
            "b.yml": self._workflow("push", self._COVERAGE),
        }
        assert not duplicates_in(suite_runs_for(estate, "pull_request"))
        assert not duplicates_in(suite_runs_for(estate, "push"))
