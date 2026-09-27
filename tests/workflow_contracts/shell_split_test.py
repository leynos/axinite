"""Unit tests for where one shell command ends and the next begins.

The estate's own steps run one suite command per line today, so a reader that
never split anything would pass every contract in this directory. The splitter
is therefore driven here directly, against the shapes a step is free to grow
into, and the duplicate-key consequence is asserted through `cargo_runs` so
the defect is stated where it costs something rather than only where it is
visible.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from _shell import split_commands
from _suite_reader import cargo_runs, make_runs
from _suite_targets import GITHUB_TOOL_SCOPE, WORKSPACE


@pytest.mark.parametrize(
    ("script", "expected"),
    [
        pytest.param("cargo test", ["cargo test"], id="one-command"),
        pytest.param(
            "cargo test && cargo build",
            ["cargo test", "cargo build"],
            id="and-and",
        ),
        pytest.param(
            "cargo test || echo failed",
            ["cargo test", "echo failed"],
            id="or-or",
        ),
        pytest.param(
            "cargo test; cargo build",
            ["cargo test", "cargo build"],
            id="semicolon",
        ),
        pytest.param(
            "cargo test | tee log",
            ["cargo test", "tee log"],
            id="pipe",
        ),
        pytest.param(
            "cargo test\ncargo build",
            ["cargo test", "cargo build"],
            id="newline",
        ),
        pytest.param(
            "set -euo pipefail\ncargo test --workspace && cargo build",
            ["set -euo pipefail", "cargo test --workspace", "cargo build"],
            id="a-real-step-preamble",
        ),
    ],
)
def test_a_script_splits_where_a_command_ends(script: str, expected: list[str]) -> None:
    """Each operator ends the command before it.

    A reader that stopped only at a newline gives the first command every
    argument of the second, which is the whole defect: the key it produces
    describes a run nothing performs.
    """
    assert split_commands(script) == expected, (
        f"{script!r} split to {split_commands(script)!r}, not {expected!r}"
    )


@pytest.mark.parametrize(
    ("script", "expected"),
    [
        pytest.param(
            "cargo test -E 'test(a|b)'",
            ["cargo test -E 'test(a|b)'"],
            id="a-pipe-in-single-quotes",
        ),
        pytest.param(
            'cargo test -E "test(a|b)"',
            ['cargo test -E "test(a|b)"'],
            id="a-pipe-in-double-quotes",
        ),
        pytest.param(
            "echo 'a; b' && cargo test",
            ["echo 'a; b'", "cargo test"],
            id="a-semicolon-in-quotes-then-a-real-operator",
        ),
        pytest.param(
            'cargo test --features "a b" && cargo build',
            ['cargo test --features "a b"', "cargo build"],
            id="quoted-value-then-a-real-operator",
        ),
    ],
)
def test_a_separator_inside_quotes_is_an_ordinary_character(
    script: str, expected: list[str]
) -> None:
    """The narrow direction: over-splitting truncates a command's arguments.

    A splitter that ignored quoting would cut `--features "a b"` in half, and
    the command would key as selecting nothing. That is the opposite error
    from the one above and just as wrong, so both are stated.
    """
    assert split_commands(script) == expected, (
        f"{script!r} split to {split_commands(script)!r}, not {expected!r}; a "
        "separator inside quotes must not end the command"
    )


def test_a_second_command_does_not_lend_the_first_its_flags(
    defaults: frozenset[str],
) -> None:
    """The consequence, asserted where it costs something.

    This is the shape the splitter exists for. Read as one command, the
    workspace run picks up `--features harness` and keys as a selection
    nothing runs; the tool-crate run then disappears entirely, because the
    manifest path is read as part of the first command's arguments.
    """
    script = (
        "cargo nextest run --workspace --profile ci && "
        "cargo test --manifest-path tools-src/github/Cargo.toml"
    )
    found = sorted(
        (scope, profile) for scope, profile, _ in cargo_runs(script, defaults)
    )
    assert found == [(GITHUB_TOOL_SCOPE, "default"), (WORKSPACE, "ci")], (
        f"the two commands are two runs; got {found}"
    )
    workspace = next(
        features
        for scope, _, features in cargo_runs(script, defaults)
        if scope == WORKSPACE
    )
    assert workspace == defaults, (
        "the workspace command names no features of its own, so it runs the "
        f"manifest's defaults; got {sorted(workspace)}"
    )


def test_a_make_target_after_an_operator_is_still_found(
    defaults: frozenset[str],
) -> None:
    """The Make reader splits on the same boundaries as the Cargo one.

    `make_runs` matched over the whole block too, so a target after an
    operator took the preceding command's arguments as its own variables.
    """
    script = 'echo building && make test-workspace TEST_FEATURES="--all-features"'
    found = sorted(make_runs(script, defaults))
    assert len(found) == 1, f"one Make target runs one suite; got {found}"
    scope, _, features = found[0]
    assert scope == WORKSPACE, (
        f"the Make target runs the workspace suite; the reader read {scope!r}"
    )
    assert ":all-features" in features, (
        "the target's own variable assignment must reach the key; got "
        f"{sorted(features)}"
    )
