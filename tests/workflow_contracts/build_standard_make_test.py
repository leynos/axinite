"""Contracts for how the Makefile routes every Cargo invocation.

Reading Make text proves nothing about what a recipe runs, so these contracts
evaluate it: ``make --dry-run <target> CARGO=probe-cargo`` prints the lines the
recipe would execute, every line that mentions the probe is parsed as a Cargo
or Whitaker invocation, and none may be skipped. Each invocation is then judged
against the routing matrix:

* a development build keeps the standard on every platform, with the Linux-only
  linker flag on Linux alone;
* a release or packaging build (`install`, the release WASM build) assigns
  ``RUSTFLAGS`` so the standard does not leak into what ships;
* Whitaker carries a literal ``RUSTFLAGS="-D warnings"`` boundary and none of
  the standard;
* the build-tools check runs before any development compile.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import collections.abc as cabc
import re
import shutil
import subprocess
import typing as typ
from pathlib import Path

import pytest

import _build_standard_make
from _build_standard import REPOSITORY_ROOT, read_toml
from _build_standard_make import (
    Invocation,
    MakeDryRunError,
    dry_run,
    parse_invocations,
)
from _build_standard_routing import make_violations, preflight_violations

#: Every target that reaches Cargo or Whitaker, so a recipe that gains a Cargo
#: line is judged. The aggregates are listed too: they are what developers run.
TARGETS = (
    "build",
    "typecheck",
    "lint",
    "lint-clippy",
    "lint-whitaker",
    "lint-libsql-transport",
    "test",
    "test-cranelift-probe",
    "test-workspace",
    "test-github-tool",
    "test-libsql-transport",
    "test-cargo",
    "test-matrix",
    "test-matrix-cargo",
    "install",
    "build-github-tool-wasm",
    "all",
)
#: How many invocations each target must emit at least, so an extractor that
#: silently skips a branch fails here rather than passing on an empty list.
MINIMUM_INVOCATIONS = {
    "build": 1,
    "typecheck": 5,
    "lint-clippy": 5,
    "lint-whitaker": 2,
    "lint-libsql-transport": 1,
    "test-cranelift-probe": 1,
    "test-workspace": 2,
    "test-github-tool": 1,
    "test-libsql-transport": 4,
    "test-cargo": 3,
    "test-matrix": 5,
    "test-matrix-cargo": 5,
    "install": 1,
    "build-github-tool-wasm": 1,
}


@pytest.fixture(scope="module")
def config() -> dict[str, typ.Any]:
    """Return the parsed Cargo configuration."""
    return read_toml(".cargo/config.toml")


@pytest.fixture(scope="module")
def emitted() -> dict[str, tuple[list[str], list[Invocation]]]:
    """Evaluate every target once: its lines and the invocations in them."""
    result: dict[str, tuple[list[str], list[Invocation]]] = {}
    for target in TARGETS:
        lines = dry_run(target)
        invocations, unparsed = parse_invocations(lines)
        assert not unparsed, (
            f"{target} emitted a probe line that did not parse: {unparsed}"
        )
        result[target] = (lines, invocations)
    return result


@pytest.mark.parametrize(
    "target", sorted(MINIMUM_INVOCATIONS), ids=sorted(MINIMUM_INVOCATIONS)
)
def test_every_cargo_line_of_a_target_is_extracted(
    target: str, emitted: dict[str, tuple[list[str], list[Invocation]]]
) -> None:
    """A target yields at least the invocations its recipe is known to hold."""
    _, invocations = emitted[target]
    assert len(invocations) >= MINIMUM_INVOCATIONS[target], (
        f"make {target} yielded {len(invocations)} invocations; the extractor skipped some"
    )


@pytest.mark.parametrize("target", TARGETS, ids=TARGETS)
def test_every_invocation_follows_the_routing_matrix(
    target: str,
    emitted: dict[str, tuple[list[str], list[Invocation]]],
    config: dict[str, typ.Any],
) -> None:
    """Development keeps the standard; release and Whitaker exclude it."""
    _, invocations = emitted[target]
    problems = [
        message
        for invocation in invocations
        for message in make_violations(
            invocation,
            config,
            label=f"make {target}: {invocation.tool} {invocation.verb}",
        )
    ]
    assert problems == [], f"make {target} violates the routing matrix: {problems}"


@pytest.mark.parametrize("target", TARGETS, ids=TARGETS)
def test_the_build_tools_check_precedes_the_first_development_compile(
    target: str, emitted: dict[str, tuple[list[str], list[Invocation]]]
) -> None:
    """A missing tool reports an installation hint before Cargo runs."""
    lines, invocations = emitted[target]
    violations = preflight_violations(lines, invocations)
    assert violations == [], (
        f"make {target} has build-tools preflight violations: {violations}; "
        f"recipe lines: {lines}"
    )


def test_release_routes_were_seen_and_assign_rustflags(
    emitted: dict[str, tuple[list[str], list[Invocation]]],
) -> None:
    """`install` and the WASM release build both carry the release assignment."""
    release = [
        invocation
        for target in ("install", "build-github-tool-wasm")
        for invocation in emitted[target][1]
    ]
    assert len(release) == 2, f"expected both release invocations, got: {release}"
    missing_assignments = [
        invocation
        for invocation in release
        if "RUSTFLAGS" not in invocation.assignments
    ]
    assert not missing_assignments, (
        f"release invocations missing RUSTFLAGS: {missing_assignments}"
    )


def test_whitaker_has_a_literal_boundary(
    emitted: dict[str, tuple[list[str], list[Invocation]]],
) -> None:
    """Both Whitaker commands assign exactly `-D warnings`.

    The literal displaces the configuration's rustflags tables, and nothing
    else in the configuration reaches Whitaker's installer-managed toolchain.
    """
    whitaker = [i for i in emitted["lint-whitaker"][1] if i.tool == "whitaker"]
    assert len(whitaker) == 2, f"expected both Whitaker invocations, got: {whitaker}"
    flags = {invocation.assignments.get("RUSTFLAGS") for invocation in whitaker}
    assert flags == {"-D warnings"}, f"unexpected Whitaker RUSTFLAGS: {flags}"


@pytest.mark.parametrize(
    "cargo", ("probe-cargo", "/opt/with space/cargo"), ids=("bare", "spaced")
)
def test_the_executable_is_substituted_on_every_cargo_line(cargo: str) -> None:
    """No recipe hard-codes `cargo`: each line runs the injected executable."""
    for target in (
        "typecheck",
        "lint-clippy",
        "test-workspace",
        "test-cranelift-probe",
        "build",
        "install",
    ):
        lines = dry_run(target, cargo=cargo)
        hard_coded = [line for line in lines if re.search(r"(^|\s)cargo\s", line)]
        assert hard_coded == [], f"make {target} runs a literal cargo: {hard_coded}"


@pytest.mark.parametrize(
    "failure",
    (
        subprocess.CalledProcessError(
            2, ["make"], stderr="No rule to make target 'missing-target'."
        ),
        OSError("make is unavailable"),
    ),
    ids=("make-failed", "make-unavailable"),
)
def test_dry_run_classifies_make_failures(
    failure: Exception, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A failed dry run names its target and preserves process diagnostics."""

    def fail_run(*args: object, **kwargs: object) -> subprocess.CompletedProcess[str]:
        del args, kwargs
        raise failure

    monkeypatch.setattr(_build_standard_make.subprocess, "run", fail_run)

    with pytest.raises(MakeDryRunError, match="missing-target") as caught:
        dry_run("missing-target")

    assert caught.value.target == "missing-target"
    assert caught.value.root == REPOSITORY_ROOT
    if isinstance(failure, subprocess.CalledProcessError):
        assert caught.value.returncode == 2
        assert caught.value.stderr == "No rule to make target 'missing-target'."
    else:
        assert caught.value.returncode is None


# -- mutations ----------------------------------------------------------------


def _mutated_dry_run(
    tmp_path: Path, target: str, edit: cabc.Callable[[str], str]
) -> list[str]:
    """Return a target's dry run against a Makefile copy that `edit` altered."""
    text = (REPOSITORY_ROOT / "Makefile").read_text(encoding="utf-8")
    changed = edit(text)
    assert changed != text, "the mutation did not apply"
    (tmp_path / "Makefile").write_text(changed, encoding="utf-8")
    return dry_run(target, root=tmp_path)


def _judge(
    tmp_path: Path,
    target: str,
    edit: cabc.Callable[[str], str],
    config: dict[str, typ.Any],
) -> list[str]:
    """Return every violation a mutated Makefile's target raises."""
    lines = _mutated_dry_run(tmp_path, target, edit)
    invocations, unparsed = parse_invocations(lines)
    problems = [f"unparsed: {line}" for line in unparsed]
    for invocation in invocations:
        problems.extend(make_violations(invocation, config, label=target))
    problems.extend(preflight_violations(lines, invocations))
    return problems


MUTATIONS: dict[str, tuple[str, cabc.Callable[[str], str]]] = {
    "typecheck-assigns-warnings-only": (
        "typecheck",
        lambda t: t.replace(
            "\t$(CARGO_COMMAND) check --all --benches --tests --examples $(TEST_FEATURES)\n",
            '\tRUSTFLAGS="-D warnings" $(CARGO_COMMAND) check --all --benches --tests --examples $(TEST_FEATURES)\n',
            1,
        ),
    ),
    "typecheck-loses-the-preflight": (
        "typecheck",
        lambda t: t.replace("typecheck: check-build-tools\n", "typecheck:\n", 1),
    ),
    "clippy-loses-the-preflight": (
        "lint-clippy",
        lambda t: t.replace("lint-clippy: check-build-tools\n", "lint-clippy:\n", 1),
    ),
    "test-loses-the-preflight": (
        "test-workspace",
        lambda t: t.replace(
            "test-workspace: check-build-tools\n", "test-workspace:\n", 1
        ),
    ),
    "probe-loses-the-preflight": (
        "test-cranelift-probe",
        lambda t: t.replace(
            "test-cranelift-probe: check-build-tools\n",
            "test-cranelift-probe:\n",
            1,
        ),
    ),
    "build-loses-the-preflight": (
        "build",
        lambda t: t.replace("build: check-build-tools\n", "build:\n", 1),
    ),
    "whitaker-inherits-the-frontend": (
        "lint-whitaker",
        lambda t: t.replace(
            'RUSTFLAGS="-D warnings" $(WHITAKER) --all --',
            'RUSTFLAGS="-D warnings -Zthreads=8" $(WHITAKER) --all --',
            1,
        ),
    ),
    "whitaker-inherits-mold": (
        "lint-whitaker",
        lambda t: t.replace(
            'RUSTFLAGS="-D warnings" $(WHITAKER) --all --',
            'RUSTFLAGS="-D warnings -Clink-arg=-fuse-ld=mold" $(WHITAKER) --all --',
            1,
        ),
    ),
    "whitaker-has-no-boundary": (
        "lint-whitaker",
        lambda t: t.replace(
            '\tRUSTFLAGS="-D warnings" $(WHITAKER) --all -- --all-targets',
            "\t$(WHITAKER) --all -- --all-targets",
            1,
        ),
    ),
    "install-inherits-the-standard": (
        "install",
        lambda t: t.replace(
            "\t$(RELEASE_RUSTFLAGS) $(CARGO_COMMAND) install",
            "\t$(CARGO_COMMAND) install",
            1,
        ),
    ),
    "wasm-release-inherits-the-standard": (
        "build-github-tool-wasm",
        lambda t: t.replace(
            "\t$(RELEASE_RUSTFLAGS) $(CARGO_COMMAND) build --manifest-path",
            "\t$(CARGO_COMMAND) build --manifest-path",
            1,
        ),
    ),
    "release-restates-the-standard": (
        "install",
        lambda t: t.replace(
            'RELEASE_RUSTFLAGS = RUSTFLAGS="$${RUSTFLAGS-}"',
            'RELEASE_RUSTFLAGS = RUSTFLAGS="-Zthreads=8"',
            1,
        ),
    ),
}


@pytest.mark.parametrize("name", sorted(MUTATIONS), ids=sorted(MUTATIONS))
def test_each_mutation_is_detected(
    name: str, tmp_path: Path, config: dict[str, typ.Any]
) -> None:
    """Breaking a route one way at a time must be reported."""
    target, edit = MUTATIONS[name]
    assert shutil.which("make"), "make is required"
    assert _judge(tmp_path, target, edit, config), (
        f"the mutation {name!r} passed the Make contract"
    )


def test_the_unmutated_copy_is_clean(
    tmp_path: Path, config: dict[str, typ.Any]
) -> None:
    """The mutation harness reports nothing against an unchanged Makefile."""
    text = (REPOSITORY_ROOT / "Makefile").read_text(encoding="utf-8")
    (tmp_path / "Makefile").write_text(text, encoding="utf-8")
    for target in ("typecheck", "lint-whitaker", "install", "build"):
        lines = dry_run(target, root=tmp_path)
        invocations, unparsed = parse_invocations(lines)
        assert not unparsed, f"make {target} emitted unparsed lines: {unparsed}"
        assert invocations, (
            f"make {target} emitted no Cargo or Whitaker commands: {lines}"
        )
        preflight_problems = preflight_violations(lines, invocations)
        assert preflight_problems == [], (
            f"make {target} preflight violations: {preflight_problems}"
        )
        routing_problems = [
            m for i in invocations for m in make_violations(i, config, label=target)
        ]
        assert routing_problems == [], (
            f"make {target} routing violations: {routing_problems}"
        )


def test_the_mutations_executed() -> None:
    """Prove the parametrized cases exist, so none can be discovered away."""
    assert len(MUTATIONS) == 12, (
        f"expected 12 Makefile mutations, got {len(MUTATIONS)}: {sorted(MUTATIONS)}"
    )
