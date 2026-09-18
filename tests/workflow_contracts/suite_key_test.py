"""Unit tests for what a single command selects: features and profile.

The estate contracts in `suite_duplication_test.py` are satisfied by finding
nothing, so the key itself is tested here. A key that merged two different
selections would report duplication that is not there, and one that separated
two spellings of the same selection would pass over duplication that is; each
class below asserts both directions.

`feature_key` takes the manifest's default feature list as an argument rather
than reading it, so every case here states the defaults it is reasoning about.
The `defaults` fixture in `conftest.py` supplies the repository's own.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _suite_keys import feature_key, profile_of
from _suite_targets import DEFAULT_PROFILE


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
        self, left: str, right: str, defaults: frozenset[str]
    ) -> None:
        """Two spellings of one selection are one run, however written."""
        assert feature_key(left, defaults) == feature_key(right, defaults), (
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
    def test_it_keeps_different_selections_apart(
        self, left: str, right: str, defaults: frozenset[str]
    ) -> None:
        """A key that merged these would report a duplicate that is not one.

        This is the half that makes the contract narrow. Without it, a key
        that returned a constant would satisfy every equality above and
        condemn the whole estate as duplicated.
        """
        assert feature_key(left, defaults) != feature_key(right, defaults), (
            f"{left!r} and {right!r} select different features; a key that "
            "merges them reports a duplicate where there are two suites"
        )

    @pytest.mark.parametrize(
        ("left", "right"),
        [
            pytest.param(
                "--no-default-features --features libsql,postgres",
                "--no-default-features --features 'libsql postgres'",
                id="commas-or-whitespace",
            ),
            pytest.param(
                "--no-default-features --features libsql",
                "--no-default-features -F libsql",
                id="long-flag-or-short",
            ),
            pytest.param(
                "--no-default-features -F libsql",
                "--no-default-features -Flibsql",
                id="short-flag-separated-or-joined",
            ),
            pytest.param(
                "--no-default-features -F libsql",
                "--no-default-features -F=libsql",
                id="short-flag-separated-or-joined-with-equals",
            ),
            pytest.param(
                "--no-default-features --features libsql",
                "--no-default-features --features=libsql",
                id="long-flag-separated-or-joined-with-equals",
            ),
            pytest.param(
                "--no-default-features --features libsql,postgres",
                "--no-default-features --features=libsql,postgres",
                id="long-flag-joined-with-a-list",
            ),
        ],
    )
    def test_it_reads_every_spelling_cargo_accepts(
        self, left: str, right: str, defaults: frozenset[str]
    ) -> None:
        """One selection written six ways is one run.

        Cargo takes `-F` as well as `--features`, takes the value joined or
        separate, and accepts a whitespace-separated list in a single
        argument. A reader that saw only the long flag and only commas would
        return an empty selection for the rest, so two different narrow legs
        would key the same and be reported as a duplicate that is not one.
        """
        assert feature_key(left, defaults) == feature_key(right, defaults), (
            f"{left!r} and {right!r} are one selection written two ways; a "
            "reader that misses a spelling keys it as naming nothing"
        )

    def test_a_spelling_the_reader_misses_is_not_silently_empty(
        self, defaults: frozenset[str]
    ) -> None:
        """The narrow direction: the spellings still have to differ by content.

        Every equality above is satisfied by a reader that returns nothing for
        all of them, which is precisely the defect. This says the short flag
        carries its value rather than merely being tolerated.
        """
        assert feature_key("--no-default-features -F libsql", defaults) != feature_key(
            "--no-default-features -F postgres", defaults
        ), "the short flag must carry its value, not just be skipped over"

    def test_naming_a_default_feature_is_naming_nothing(
        self, defaults: frozenset[str]
    ) -> None:
        """A list of members of `default` selects what `default` selects.

        This is the defect the key was written without. `test.yml`'s leg
        named `all-features` passed three members of `default` and no
        `--no-default-features`, so Cargo built it exactly as it built the
        leg that passed no flags at all, and two paid legs ran one suite.
        """
        named = " ".join(f"--features {feature}" for feature in defaults)
        assert feature_key(f"{named} --features test-helpers", defaults) == feature_key(
            "--features test-helpers", defaults
        ), (
            f"naming {named!r} selects exactly what naming nothing selects, "
            "because every one of them is already a member of `default`"
        )

    def test_turning_the_defaults_off_keeps_two_narrow_legs_apart(
        self, defaults: frozenset[str]
    ) -> None:
        """Two `--no-default-features` legs differ by what they name.

        The direction that stops the resolution above becoming a blanket
        merge, and it has to compare two narrow legs to bite. Add the
        defaults to a command that has just turned them off and every narrow
        leg keys as the default set, because the features such a leg names
        are usually members of it: a libsql-only run and a postgres-only run
        would read as one run, and the contract would report a duplicate
        where there are two different suites.
        """
        assert feature_key(
            "--no-default-features --features libsql", defaults
        ) != feature_key("--no-default-features --features postgres", defaults), (
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


class TestTheMakeVariableSpelling:
    """A `make` step hands its selection over as one shell word.

    `TEST_FEATURES="--features x"` is a single token. A reader that did not
    unpack it would see no feature flags at all and key the command as the
    defaults, so a coverage lane driving `make` and a test lane driving Cargo
    would compare as different work when they run the same suite, and as the
    same work when they do not.
    """

    @pytest.mark.parametrize(
        "assignment",
        [
            pytest.param(
                "--no-default-features --features libsql", id="defaults-turned-off"
            ),
            pytest.param("--features test-helpers", id="defaults-left-on"),
            pytest.param("--all-features", id="all-features"),
            pytest.param(
                "--features libsql,postgres", id="a-list-inside-the-assignment"
            ),
        ],
    )
    def test_the_assignment_selects_what_the_flags_select(
        self, assignment: str, defaults: frozenset[str]
    ) -> None:
        """`TEST_FEATURES="X"` keys exactly as `X` does.

        This is the case that fails if the assignment stops being expanded:
        the whole token would carry no recognizable flag, so the command
        would key as the defaults whatever it named.
        """
        assert feature_key(f'TEST_FEATURES="{assignment}"', defaults) == feature_key(
            assignment, defaults
        ), (
            f"TEST_FEATURES={assignment!r} runs exactly {assignment!r}; a "
            "reader that leaves the assignment packed keys it as naming nothing"
        )

    def test_two_assignments_that_differ_still_differ(
        self, defaults: frozenset[str]
    ) -> None:
        """The narrow direction, without which the equalities prove nothing.

        A reader that dropped every `TEST_FEATURES=` token would satisfy none
        of the equalities above, but one that expanded the token and then
        discarded its contents would satisfy all of them. This says the
        contents reach the key.
        """
        assert feature_key(
            'TEST_FEATURES="--no-default-features --features libsql"', defaults
        ) != feature_key(
            'TEST_FEATURES="--no-default-features --features postgres"', defaults
        ), (
            "a libsql-only make step and a postgres-only one run two suites; "
            "a reader that expanded the assignment and dropped its contents "
            "would key them the same"
        )

    def test_the_assignment_carries_the_flags_past_the_target_name(
        self, defaults: frozenset[str]
    ) -> None:
        """The shape a workflow step actually writes, read whole.

        The estate writes the variable before the target and other arguments
        after it. Reading only a leading assignment, or only a trailing one,
        would key half the estate's steps as naming nothing.
        """
        assert feature_key(
            'NEXTEST_PROFILE=ci TEST_FEATURES="--all-features"', defaults
        ) == feature_key("--all-features", defaults)
