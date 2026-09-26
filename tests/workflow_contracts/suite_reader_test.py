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

    _ACTION = (
        "  ratchet:\n"
        "    runs-on: ubicloud-standard-4\n"
        "    steps:\n"
        "      - uses: leynos/shared-actions/.github/actions/generate-coverage@abc\n"
        "        env:\n"
        "          NEXTEST_PROFILE: {profile}\n"
        "        with:\n"
        "          features: test-helpers\n"
        "          use-cargo-nextest: 'true'\n"
    )

    def test_a_run_through_the_coverage_action_is_a_suite_run(
        self, defaults: frozenset[str]
    ) -> None:
        """The action runs the workspace suite with no `run:` line to read.

        A reading of scripts alone missed both lanes that moved onto the
        action, so a duplicate of either would pass unseen. The same suite
        under the same profile from a script is the duplicate that proves it.
        """
        ci_tests = self._TESTS.replace(
            "cargo nextest run --workspace", "cargo nextest run --profile ci --workspace"
        )
        documents = {
            "one.yml": self._workflow(
                "pull_request", ci_tests, self._ACTION.format(profile="ci")
            )
        }
        duplicated = duplicates_in(suite_runs_for(documents, "pull_request", defaults))
        found = sorted(str(run) for runs in duplicated.values() for run in runs)
        assert found == ["one.yml:ratchet", "one.yml:tests"], (
            f"the action's run must be seen and keyed like a script's; got {found!r}"
        )

    def test_the_action_reads_its_profile_from_the_step_env(
        self, defaults: frozenset[str]
    ) -> None:
        """Without `NEXTEST_PROFILE: ci` the action runs the default profile."""
        ci_tests = self._TESTS.replace(
            "cargo nextest run --workspace", "cargo nextest run --profile ci --workspace"
        )
        documents = {
            "one.yml": self._workflow(
                "pull_request", ci_tests, self._ACTION.format(profile="default")
            )
        }
        assert not duplicates_in(suite_runs_for(documents, "pull_request", defaults)), (
            "a default-profile action run is a different suite from a ci one"
        )

    def test_a_step_for_one_leg_runs_on_that_leg_alone(
        self, defaults: frozenset[str]
    ) -> None:
        """`coverage.yml` splits one suite between a script and the action.

        Read without the step conditions, the libsql leg would count the
        script's run and the action's, and report the leg against itself.
        """
        documents = {
            "one.yml": self._workflow(
                "pull_request",
                "  coverage:\n"
                "    runs-on: ubicloud-standard-4\n"
                "    strategy:\n"
                "      matrix:\n"
                "        include:\n"
                "          - name: default\n"
                "            flags: ''\n"
                "          - name: libsql-only\n"
                "            flags: --no-default-features\n"
                "    steps:\n"
                "      - if: matrix.name != 'libsql-only'\n"
                "        run: cargo nextest run --workspace --features test-helpers"
                " ${{ matrix.flags }}\n"
                "      - if: matrix.name == 'libsql-only'\n"
                "        uses: leynos/shared-actions/.github/actions/generate-coverage@a\n"
                "        with:\n"
                "          features: test-helpers\n"
                "          with-default-features: 'false'\n"
                "          use-cargo-nextest: 'true'\n",
            )
        }
        runs = suite_runs_for(documents, "pull_request", defaults)
        assert sorted(run.leg for run in runs) == ["default", "libsql-only"], (
            f"each leg runs the suite once; got {[str(run) for run in runs]}"
        )
        assert not duplicates_in(runs), "the two legs select different features"
