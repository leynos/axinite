"""Bounded properties for invocation classification and preflight order.

Generated references are helper-level checks over supported synthetic
commands. Fixed command cases carry explicit expectations from the documented
routing rules. Preflight sequences are internally coherent; these properties
do not execute Make recipes or shell commands.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import pytest
from hypothesis import given
from hypothesis import strategies as st

from _build_standard import PLATFORMS
from _build_standard_make import Invocation
from _build_standard_routing import (
    is_release_build,
    preflight_violations,
    target_triples,
)

PREFLIGHT_COMMAND = "./scripts/check-build-tools.sh"
TOOLS = ("cargo", "nextest", "whitaker")
CARGO_VERBS = ("build", "check", "clippy", "test", "run", "install")
ARGUMENT = st.sampled_from(("--release", "-r", "--all-targets", "--features=x"))
DEFAULT_TARGETS = [triple for triple in PLATFORMS if PLATFORMS[triple] != "wasi"]


@st.composite
def _invocations(draw: st.DrawFn) -> Invocation:
    """Build supported tool/verb combinations and complete option values."""
    tool = draw(st.sampled_from(TOOLS))
    verb = _draw_verb(draw, tool)
    arguments = _base_arguments(draw, tool, verb)
    _append_profile_options(draw, tool, arguments)
    _append_target_option(draw, tool, arguments)
    return Invocation(tool, verb, {}, tuple(arguments), draw(st.integers(0, 10)))


def _draw_verb(draw: st.DrawFn, tool: str) -> str:
    if tool == "cargo":
        return draw(st.sampled_from(CARGO_VERBS))
    if tool == "nextest":
        return "run"
    return "dylint"


def _base_arguments(draw: st.DrawFn, tool: str, verb: str) -> list[str]:
    arguments = ["axinite-cli"] if verb == "install" else []
    if tool != "whitaker":
        arguments.extend(draw(st.lists(ARGUMENT, max_size=8)))
    return arguments


def _append_optional_option(
    draw: st.DrawFn,
    arguments: list[str],
    option: str,
    choices: tuple[str, ...],
) -> None:
    if draw(st.booleans()):
        arguments.extend((option, draw(st.sampled_from(choices))))


def _append_profile_options(draw: st.DrawFn, tool: str, arguments: list[str]) -> None:
    if tool == "cargo":
        _append_optional_option(
            draw, arguments, "--profile", ("dev", "test", "release", "bench")
        )
    if tool == "nextest":
        _append_optional_option(draw, arguments, "--profile", ("default", "ci"))
        _append_optional_option(
            draw, arguments, "--cargo-profile", ("release", "bench")
        )


def _append_target_option(draw: st.DrawFn, tool: str, arguments: list[str]) -> None:
    if tool != "whitaker" and draw(st.booleans()):
        arguments.extend(("--target", draw(st.sampled_from(tuple(PLATFORMS)))))
    if tool == "whitaker":
        arguments.append("--all")


def _reference_release(invocation: Invocation) -> bool:
    """Classify release commands from the documented Cargo/nextest arguments."""
    arguments = invocation.arguments
    if invocation.verb == "install":
        return True
    if invocation.tool == "nextest":
        return "--release" in arguments or "--cargo-profile" in arguments
    if "--profile" in arguments:
        profile_index = arguments.index("--profile")
        return arguments[profile_index + 1] not in ("dev", "test")
    return "--release" in arguments or "-r" in arguments


def _reference_targets(invocation: Invocation) -> list[str]:
    """Use an explicit target when present, otherwise every supported host."""
    if "--target" in invocation.arguments:
        return [invocation.arguments[invocation.arguments.index("--target") + 1]]
    return [triple for triple in PLATFORMS if PLATFORMS[triple] != "wasi"]


def _reference_preflight(lines: list[str], invocations: list[Invocation]) -> bool:
    """Report whether a development compile lacks an earlier preflight line."""
    compiling = [
        command
        for command in invocations
        if command.verb not in ("fmt", "clean")
        and command.tool != "whitaker"
        and not _reference_release(command)
    ]
    if not compiling:
        return False
    first_compile = min(command.index for command in compiling)
    checks = [
        index for index, line in enumerate(lines) if line.strip() == PREFLIGHT_COMMAND
    ]
    return not checks or min(checks) > first_compile


@given(_invocations())
def test_release_and_target_selection_follow_argument_meaning(
    invocation: Invocation,
) -> None:
    """The generated helper-level model agrees on supported invocations."""
    assert is_release_build(invocation) == _reference_release(invocation), (
        f"invocation={invocation!r}"
    )
    assert target_triples(invocation) == _reference_targets(invocation), (
        f"invocation={invocation!r}"
    )


@pytest.mark.parametrize(
    ("invocation", "expected_release", "expected_targets"),
    [
        pytest.param(
            Invocation("cargo", "build", {}, (), 0),
            False,
            DEFAULT_TARGETS,
            id="bare-cargo-build-is-development",
        ),
        pytest.param(
            Invocation("cargo", "build", {}, ("--release",), 0),
            True,
            DEFAULT_TARGETS,
            id="cargo-release-flag",
        ),
        pytest.param(
            Invocation("cargo", "build", {}, ("--profile", "bench"), 0),
            True,
            DEFAULT_TARGETS,
            id="cargo-custom-release-profile",
        ),
        pytest.param(
            Invocation("cargo", "test", {}, ("--profile", "test"), 0),
            False,
            DEFAULT_TARGETS,
            id="cargo-test-profile",
        ),
        pytest.param(
            Invocation("cargo", "install", {}, ("axinite-cli",), 0),
            True,
            DEFAULT_TARGETS,
            id="cargo-install",
        ),
        pytest.param(
            Invocation("nextest", "run", {}, ("--profile", "ci"), 0),
            False,
            DEFAULT_TARGETS,
            id="nextest-profile-is-not-cargo-profile",
        ),
        pytest.param(
            Invocation(
                "nextest",
                "run",
                {},
                ("--cargo-profile", "release", "--target", "x86_64-unknown-linux-gnu"),
                0,
            ),
            True,
            ["x86_64-unknown-linux-gnu"],
            id="nextest-cargo-release-and-explicit-target",
        ),
    ],
)
def test_supported_command_forms_match_explicit_routing_expectations(
    invocation: Invocation,
    expected_release: bool,
    expected_targets: list[str],
) -> None:
    """Specification examples anchor routing beyond the copied model."""
    assert is_release_build(invocation) is expected_release
    assert target_triples(invocation) == expected_targets


@st.composite
def _preflight_cases(draw: st.DrawFn) -> tuple[list[str], list[Invocation]]:
    """Generate a coherent sequence of preflight and command roles."""
    roles = draw(
        st.lists(
            st.sampled_from(
                ("preflight", "development", "release", "whitaker", "other")
            ),
            max_size=12,
        )
    )
    lines: list[str] = []
    invocations: list[Invocation] = []
    for index, role in enumerate(roles):
        if role == "preflight":
            lines.append(PREFLIGHT_COMMAND)
        elif role == "development":
            lines.append("cargo test")
            invocations.append(Invocation("cargo", "test", {}, (), index))
        elif role == "release":
            lines.append("cargo build --release")
            invocations.append(Invocation("cargo", "build", {}, ("--release",), index))
        elif role == "whitaker":
            lines.append('RUSTFLAGS="-D warnings" whitaker --all')
            invocations.append(Invocation("whitaker", "dylint", {}, ("--all",), index))
        else:
            lines.append("echo setup")
    return lines, invocations


@given(
    case=_preflight_cases(),
)
def test_preflight_diagnostics_match_compile_order(
    case: tuple[list[str], list[Invocation]],
) -> None:
    """Coherent synthetic command sequences check helper ordering only."""
    lines, invocations = case
    actual = bool(preflight_violations(lines, invocations))
    expected = _reference_preflight(lines, invocations)
    assert actual == expected, (
        f"lines={lines!r}, invocations={invocations!r}, "
        f"expected_violation={expected}, actual={actual}"
    )
