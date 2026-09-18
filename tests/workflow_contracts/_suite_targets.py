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
import typing as typ
from collections.abc import Mapping

from _sources import read_text, read_toml
from _workflow_policy import REPOSITORY_ROOT

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

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
#: The build `test-workspace` has to perform before its suite. The metadata
#: and schema tests load the artefact it produces, so a recipe that runs the
#: suite first tests yesterday's WASM or fails on a clean checkout.
WASM_PREREQUISITE = "$(MAKE) build-github-tool-wasm"
#: The command that runs the workspace suite under the step's variables.
WORKSPACE_RECIPE = (
    "$(NEXTEST) run --workspace $(TEST_FEATURES) --profile $(NEXTEST_PROFILE)"
)
#: `test-workspace`'s whole recipe, in order. Asserted entire rather than
#: searched: a nested `$(MAKE) test-github-tool` would run the tool suite once
#: per leg again, which is the duplication this split removed, and it would
#: satisfy any check that merely refused a `--manifest-path` line.
WORKSPACE_RECIPE_LINES: tuple[str, ...] = (WASM_PREREQUISITE, WORKSPACE_RECIPE)
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
#: The Makefile that says what each target above actually runs.
MAKEFILE = REPOSITORY_ROOT / "Makefile"


def default_features_in(manifest: Mapping[str, object]) -> frozenset[str]:
    """Return the default feature set a parsed manifest declares.

    Cargo enables these on every command that does not pass
    `--no-default-features`, so a leg that names three of them and a leg that
    names none compile and run exactly the same thing. Reading them is what
    stops the contract comparing what a command says against what another
    command says, rather than what each one runs.

    Parameters
    ----------
    manifest
        A parsed `Cargo.toml`.

    Returns
    -------
    frozenset of str
        Every feature in the manifest's `default` list.
    """
    features = manifest.get("features")
    declared = features.get("default", []) if isinstance(features, Mapping) else []
    return frozenset(str(name) for name in declared)


def read_default_features(manifest: Path = ROOT_MANIFEST) -> frozenset[str]:
    """Read a manifest and return the default feature set it declares.

    This is the boundary: it names the file, it is the only thing here that
    touches one, and it reports a read or parse failure as a `SourceError`
    naming the path. Everything downstream takes the resulting set as an
    argument, so no query function hides a filesystem access behind a
    signature that says it answers a question about a command's flags.

    Parameters
    ----------
    manifest
        The manifest to read. It defaults to the root one; the parameter
        exists so the failure cases can be stated against a temporary file.

    Returns
    -------
    frozenset of str
        Every feature in the manifest's `default` list.

    Raises
    ------
    SourceError
        If the manifest is missing, unreadable or not valid TOML.
    """
    return default_features_in(read_toml(manifest))


def make_rule_in(makefile: str, name: str) -> tuple[tuple[str, ...], tuple[str, ...]]:
    """Return one rule's prerequisites and recipe lines from Makefile text.

    Pure in the text it is given, so a rule shape can be stated in a test
    without writing a file, and so the reading of the repository's own
    Makefile is one call rather than a hidden read inside every query.

    Parameters
    ----------
    makefile
        The Makefile's text.
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


def read_makefile(path: Path = MAKEFILE) -> str:
    """Read the Makefile these contracts judge.

    Parameters
    ----------
    path
        The file to read. It defaults to the repository's own; the parameter
        exists so the failure cases can be stated against a temporary file.

    Returns
    -------
    str
        The Makefile's text.

    Raises
    ------
    SourceError
        If the file is missing, unreadable or not valid UTF-8.
    """
    return read_text(path)
