"""Property tests for Cargo invocation classification and preflight order.

Each expected result is derived from the invocation's raw arguments and line
positions rather than the routing helpers under test.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

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
VERBS = ("build", "check", "clippy", "test", "run", "install", "fmt", "clean")
ARGUMENT = st.sampled_from(("--release", "-r", "--all-targets", "--features", "x"))


@st.composite
def _invocations(draw: st.DrawFn) -> Invocation:
    """Build safe argument sequences, including paired profile and target flags."""
    tool = draw(st.sampled_from(TOOLS))
    verb = draw(st.sampled_from(VERBS))
    arguments = list(draw(st.lists(ARGUMENT, max_size=8)))
    if draw(st.booleans()):
        profile = draw(st.sampled_from(("dev", "test", "release", "bench")))
        arguments.extend(("--profile", profile))
    if draw(st.booleans()):
        arguments.extend(("--target", draw(st.sampled_from(tuple(PLATFORMS)))))
    if draw(st.booleans()):
        arguments.append("--cargo-profile")
    return Invocation(tool, verb, {}, tuple(arguments), draw(st.integers(0, 10)))


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


def _reference_preflight(
    lines: list[str], invocations: list[Invocation]
) -> bool:
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
    """Profile flags and target flags select the intended build route."""
    assert is_release_build(invocation) == _reference_release(invocation), (
        f"invocation={invocation!r}"
    )
    assert target_triples(invocation) == _reference_targets(invocation), (
        f"invocation={invocation!r}"
    )


@given(
    lines=st.lists(
        st.sampled_from((PREFLIGHT_COMMAND, "cargo test", "echo setup")), max_size=12
    ),
    invocations=st.lists(_invocations(), max_size=8),
)
def test_preflight_diagnostics_match_compile_order(
    lines: list[str], invocations: list[Invocation]
) -> None:
    """A preflight at or before the first dev compile is sufficient."""
    actual = bool(preflight_violations(lines, invocations))
    expected = _reference_preflight(lines, invocations)
    assert actual == expected, (
        f"lines={lines!r}, invocations={invocations!r}, "
        f"expected_violation={expected}, actual={actual}"
    )
