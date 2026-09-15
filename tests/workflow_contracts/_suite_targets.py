"""What this repository's test commands are, read from the Makefile.

A workflow step says `make test-workspace`, and only the Makefile says what
that runs. A Cargo command names some features, and only `Cargo.toml` says
which others come with them. Both readings live here so that the contracts in
`suite_duplication_test.py` compare what each lane executes rather than what
it says, and so that neither reading is restated in two places.

See `_suite_reader.py` for the workflow side of the same question.
"""

from __future__ import annotations

import re
import tomllib

from _workflow_policy import REPOSITORY_ROOT

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

#: The build `test-workspace` has to perform before that command. The metadata
#: and schema tests load the artefact it produces, so a recipe that runs the
#: suite first tests yesterday's WASM or fails on a clean checkout.
WASM_PREREQUISITE = "$(MAKE) build-github-tool-wasm"
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
#: A Make invocation of one of the targets above. The negative lookahead stops
#: `test` matching the first half of `test-workflow-contracts`, which is a
#: PyYAML parse rather than a suite.
MAKE_COMMAND: re.Pattern[str] = re.compile(
    r"\bmake\s+(?P<target>"
    + "|".join(sorted(MAKE_TARGETS, key=len, reverse=True))
    + r")\b(?!-)(?P<args>[^\n]*)"
)
#: Sentinels for the flags that select features without naming any. They are
#: part of the key so that `--all-features` and `--no-default-features
#: --features libsql` cannot collide with each other or with a feature list.
ALL_FEATURES = ":all-features"
NO_DEFAULT_FEATURES = ":no-default-features"
#: The manifest whose `default` feature list every command inherits unless it
#: passes `--no-default-features`.
ROOT_MANIFEST = REPOSITORY_ROOT / "Cargo.toml"


def default_features() -> frozenset[str]:
    """Return the root package's default feature set.

    Cargo enables these on every command that does not pass
    `--no-default-features`, so a leg that names three of them and a leg that
    names none compile and run exactly the same thing. Reading them here is
    what stops the contract comparing what a command says against what
    another command says, rather than what each one runs.

    Returns
    -------
    frozenset of str
        Every feature in the root manifest's `default` list.
    """
    manifest = tomllib.loads(ROOT_MANIFEST.read_text(encoding="utf-8"))
    declared = manifest.get("features", {}).get("default", [])
    return frozenset(str(name) for name in declared)


DEFAULT_FEATURES: frozenset[str] = default_features()


def make_rule(name: str) -> tuple[tuple[str, ...], tuple[str, ...]]:
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
