"""Unit tests for the duplicate detection `suite_duplication_test.py` rests on.

The estate contract is satisfied by finding nothing, so the detector is driven
here against fixtures instead: one that contains the duplication the estate
used to pay for, and several that are the normal case and must never be
reported. Without the second kind, a detector that called everything a
duplicate would pass the first.

See `suite_key_test.py` for what a single command selects, and
`source_boundary_test.py` for the reading of the files these contracts judge.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import typing as typ

import yaml
from _suite_reader import duplicates_in, suite_runs_for


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

    def test_it_reports_a_coverage_lane_repeating_a_test_lane(
        self, defaults: frozenset[str]
    ) -> None:
        """The exact shape removed from `test.yml` must not pass unseen."""
        documents = {
            "one.yml": self._workflow("pull_request", self._TESTS, self._COVERAGE)
        }
        duplicated = duplicates_in(suite_runs_for(documents, "pull_request", defaults))
        assert len(duplicated) == 1, (
            f"a coverage lane repeating a test lane is one duplicate; got "
            f"{duplicated!r}"
        )
        found = sorted(str(run) for run in next(iter(duplicated.values())))
        assert found == ["one.yml:coverage", "one.yml:tests"], (
            f"the duplicate must name both lanes that cause it; got {found!r}"
        )

    def test_it_reports_one_lane_running_the_same_suite_per_leg(
        self, defaults: frozenset[str]
    ) -> None:
        """The GitHub tool crate's old shape: one command, three legs."""
        documents = {
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
        duplicated = duplicates_in(suite_runs_for(documents, "pull_request", defaults))
        assert len(duplicated) == 1, (
            "a command that ignores the matrix runs the identical suite once "
            "per leg, which is duplication inside a single job"
        )

    def test_a_guard_that_excludes_the_event_removes_the_duplicate(
        self, defaults: frozenset[str]
    ) -> None:
        """A job the trigger cannot dispatch costs nothing and is not a clash.

        This is the assertion that makes the fix visible: the same pair of
        lanes, with the guard `test.yml` now carries, is not a duplicate.
        """
        guarded = self._TESTS.replace(
            "    runs-on:", "    if: github.event_name != 'pull_request'\n    runs-on:"
        )
        documents = {"one.yml": self._workflow("pull_request", guarded, self._COVERAGE)}
        assert not duplicates_in(suite_runs_for(documents, "pull_request", defaults)), (
            "a job the trigger cannot dispatch costs nothing, so the pair is "
            "not a clash; this is the guard `test.yml` now carries"
        )

    def test_different_feature_sets_are_not_a_duplicate(
        self, defaults: frozenset[str]
    ) -> None:
        """Two lanes running different suites are the normal case."""
        other = self._COVERAGE.replace("--features test-helpers", "--all-features")
        documents = {"one.yml": self._workflow("pull_request", self._TESTS, other)}
        assert not duplicates_in(suite_runs_for(documents, "pull_request", defaults)), (
            "two lanes running different feature sets are two suites, which "
            "is the normal case and must never be reported as duplication"
        )

    def test_a_different_profile_is_not_a_duplicate(
        self, defaults: frozenset[str]
    ) -> None:
        """The same flags under a narrower profile run a different suite.

        This is what made the coverage lanes stop short of replacing the test
        legs: they ran the default profile, which drops the trybuild compile
        contracts, so standing the legs down would have left those unexecuted.
        """
        narrower = self._TESTS.replace(
            "cargo nextest run --workspace",
            "cargo nextest run --profile ci --workspace",
        )
        documents = {
            "one.yml": self._workflow("pull_request", narrower, self._COVERAGE)
        }
        assert not duplicates_in(suite_runs_for(documents, "pull_request", defaults)), (
            "the same flags under a narrower profile run a different set of "
            "tests, so the profile is part of what makes two runs the same"
        )

    def test_a_workflow_the_trigger_does_not_declare_contributes_nothing(
        self, defaults: frozenset[str]
    ) -> None:
        """`coverage.yml` runs on a push only; it cannot clash on a pull."""
        documents = {
            "a.yml": self._workflow("pull_request", self._TESTS),
            "b.yml": self._workflow("push", self._COVERAGE),
        }
        assert not duplicates_in(suite_runs_for(documents, "pull_request", defaults)), (
            "b.yml runs on a push only, so it cannot clash on a pull request"
        )
        assert not duplicates_in(suite_runs_for(documents, "push", defaults)), (
            "a.yml runs on a pull request only, so it cannot clash on a push"
        )
