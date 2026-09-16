"""Unit tests for the reading `suite_duplication_test.py` rests on.

The estate contracts are satisfied by finding nothing, so the reader has to be
tested on its own: a key that merged two different selections would report
duplication that is not there, and one that separated two spellings of the
same selection would pass over duplication that is. Each class below asserts
both directions for one part of the key.
"""

from __future__ import annotations

import tomllib
import typing as typ

import pytest
import yaml
from _suite_keys import feature_key, profile_of
from _suite_reader import duplicates_in, suite_runs_for
from _suite_targets import DEFAULT_FEATURES, DEFAULT_PROFILE, ROOT_MANIFEST


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
        assert feature_key(left) == feature_key(right), (
            f"{left!r} and {right!r} select the same features, so they are "
            "one run; a key that separates them passes over a duplicate"
        )

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
        assert feature_key(left) != feature_key(right), (
            f"{left!r} and {right!r} select different features; a key that "
            "merges them reports a duplicate where there are two suites"
        )

    def test_the_default_set_comes_from_the_manifest(self) -> None:
        """Read the defaults rather than restating them.

        A reader that returned an empty set would restore the old behaviour
        silently: every equality below would still hold for a command that
        names nothing, and the only evidence would be a duplicate the
        contract failed to report.
        """
        declared = tomllib.loads(ROOT_MANIFEST.read_text(encoding="utf-8"))
        assert DEFAULT_FEATURES == frozenset(declared["features"]["default"]), (
            f"{DEFAULT_FEATURES!r} is not the manifest's default list; a "
            "restated set drifts from the one Cargo actually enables"
        )
        assert DEFAULT_FEATURES, "the root manifest declares no default features"

    def test_naming_a_default_feature_is_naming_nothing(self) -> None:
        """A list of members of `default` selects what `default` selects.

        This is the defect the key was written without. `test.yml`'s leg
        named `all-features` passed three members of `default` and no
        `--no-default-features`, so Cargo built it exactly as it built the
        leg that passed no flags at all, and two paid legs ran one suite.
        """
        named = " ".join(f"--features {feature}" for feature in DEFAULT_FEATURES)
        assert feature_key(f"{named} --features test-helpers") == feature_key(
            "--features test-helpers"
        ), (
            f"naming {named!r} selects exactly what naming nothing selects, "
            "because every one of them is already a member of `default`"
        )

    def test_turning_the_defaults_off_keeps_two_narrow_legs_apart(self) -> None:
        """Two `--no-default-features` legs differ by what they name.

        The direction that stops the resolution above becoming a blanket
        merge, and it has to compare two narrow legs to bite. Add the
        defaults to a command that has just turned them off and every narrow
        leg keys as the default set, because the features such a leg names
        are usually members of it: a libsql-only run and a postgres-only run
        would read as one run, and the contract would report a duplicate
        where there are two different suites.
        """
        assert feature_key("--no-default-features --features libsql") != feature_key(
            "--no-default-features --features postgres"
        ), (
            "a libsql-only run and a postgres-only run are two suites; "
            "folding the defaults into either would read them as one"
        )


class TestProfile:
    """The profile decides which tests run, so it decides identity."""

    @pytest.mark.parametrize(
        ("args", "expected"),
        [
            ("--workspace --lcav", DEFAULT_PROFILE),
            ("--workspace --profile ci", "ci"),
            ("--workspace --profile=ci", "ci"),
            ('NEXTEST_PROFILE=ci TEST_FEATURES="--features x"', "ci"),
            ('TEST_FEATURES="--features x"', DEFAULT_PROFILE),
        ],
        ids=["absent", "cargo", "cargo-equals", "make", "make-absent"],
    )
    def test_it_reads_every_spelling(self, args: str, expected: str) -> None:
        """A profile the reader misses defaults, and defaults compare equal.

        That is the direction that matters: two lanes would then look like one
        run when the tests they execute differ by the whole trybuild set.
        """
        assert profile_of(args) == expected, (
            f"{args!r} selects the {expected!r} profile; a profile the reader "
            "misses defaults, and two defaults compare equal"
        )


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
        estate = {
            "one.yml": self._workflow("pull_request", self._TESTS, self._COVERAGE)
        }
        duplicated = duplicates_in(suite_runs_for(estate, "pull_request"))
        assert len(duplicated) == 1, (
            f"a coverage lane repeating a test lane is one duplicate; got "
            f"{duplicated!r}"
        )
        found = sorted(str(run) for run in next(iter(duplicated.values())))
        assert found == ["one.yml:coverage", "one.yml:tests"], (
            f"the duplicate must name both lanes that cause it; got {found!r}"
        )

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
        assert not duplicates_in(suite_runs_for(estate, "pull_request")), (
            "a job the trigger cannot dispatch costs nothing, so the pair is "
            "not a clash; this is the guard `test.yml` now carries"
        )

    def test_different_feature_sets_are_not_a_duplicate(self) -> None:
        """Two lanes running different suites are the normal case."""
        other = self._COVERAGE.replace("--features test-helpers", "--all-features")
        estate = {"one.yml": self._workflow("pull_request", self._TESTS, other)}
        assert not duplicates_in(suite_runs_for(estate, "pull_request")), (
            "two lanes running different feature sets are two suites, which "
            "is the normal case and must never be reported as duplication"
        )

    def test_a_different_profile_is_not_a_duplicate(self) -> None:
        """The same flags under a narrower profile run a different suite.

        This is what made the coverage lanes stop short of replacing the test
        legs: they ran the default profile, which drops the trybuild compile
        contracts, so standing the legs down would have left those unexecuted.
        """
        narrower = self._TESTS.replace(
            "cargo nextest run --workspace",
            "cargo nextest run --profile ci --workspace",
        )
        estate = {"one.yml": self._workflow("pull_request", narrower, self._COVERAGE)}
        assert not duplicates_in(suite_runs_for(estate, "pull_request")), (
            "the same flags under a narrower profile run a different set of "
            "tests, so the profile is part of what makes two runs the same"
        )

    def test_a_workflow_the_trigger_does_not_declare_contributes_nothing(self) -> None:
        """`coverage.yml` runs on a push only; it cannot clash on a pull."""
        estate = {
            "a.yml": self._workflow("pull_request", self._TESTS),
            "b.yml": self._workflow("push", self._COVERAGE),
        }
        assert not duplicates_in(suite_runs_for(estate, "pull_request")), (
            "b.yml runs on a push only, so it cannot clash on a pull request"
        )
        assert not duplicates_in(suite_runs_for(estate, "push")), (
            "a.yml runs on a pull request only, so it cannot clash on a push"
        )
