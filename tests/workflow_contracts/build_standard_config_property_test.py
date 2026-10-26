"""Property tests for Cargo rustflags normalization and source precedence.

The oracle models Cargo's single-source selection directly, independently of
`_build_standard`'s table reader and matching helpers.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

from hypothesis import given
from hypothesis import strategies as st

from _build_standard import PLATFORMS, effective_rustflags, normalize

FLAG = st.text(max_size=18)
SOURCE = st.one_of(FLAG, st.lists(FLAG, max_size=12))
TARGET_KEYS = tuple(PLATFORMS) + ('cfg(target_os = "linux")',)


def _reference_normalize(flags: list[str]) -> list[str]:
    """Normalize tokens with an independent pending-control state machine."""
    normalized: list[str] = []
    pending: str | None = None
    for flag in flags:
        if not flag:
            continue
        if pending is not None:
            normalized.append(pending + flag)
            pending = None
        elif flag in ("-C", "-Z"):
            pending = flag
        else:
            normalized.append(flag)
    if pending is not None:
        normalized.append(pending)
    return normalized


def _source_tokens(source: object) -> list[str]:
    """Read a generated table value using the input contract, not helpers."""
    if isinstance(source, str):
        return _reference_normalize(source.split())
    if isinstance(source, list):
        return _reference_normalize([str(token) for token in source])
    return []


def _target_applies(key: object, triple: str) -> bool:
    """Match an exact target or the Linux cfg target independently."""
    if key == triple:
        return True
    if key != 'cfg(target_os = "linux")':
        return False
    return PLATFORMS[triple] == "linux"


def _matching_target_sources(config: dict[str, object], triple: str) -> list[object]:
    """Collect rustflags from every matching Cargo target table."""
    targets = config.get("target", {})
    if not isinstance(targets, dict):
        return []

    matching: list[object] = []
    for key, table in targets.items():
        if not _target_applies(key, triple):
            continue
        if not isinstance(table, dict):
            continue
        if "rustflags" not in table:
            continue
        matching.append(table["rustflags"])
    return matching


def _build_source_tokens(config: dict[str, object]) -> list[str]:
    """Read build rustflags only after no target table matched."""
    build = config.get("build", {})
    if not isinstance(build, dict):
        return []
    if "rustflags" not in build:
        return []
    return _source_tokens(build["rustflags"])


def _reference_effective(
    config: dict[str, object], triple: str, assigned: str | None
) -> list[str]:
    """Apply an assigned environment or Cargo's target-then-build precedence."""
    if assigned is not None:
        return _reference_normalize(assigned.split())

    matching = _matching_target_sources(config, triple)
    if matching:
        return [token for source in matching for token in _source_tokens(source)]
    return _build_source_tokens(config)


@given(st.lists(FLAG, max_size=40))
def test_normalize_matches_the_pending_control_model(flags: list[str]) -> None:
    """Arbitrary empty and split compiler tokens normalize by one rule."""
    assert normalize(flags) == _reference_normalize(flags), f"flags={flags!r}"


@given(
    triple=st.sampled_from(tuple(PLATFORMS)),
    build=SOURCE,
    targets=st.dictionaries(
        st.sampled_from(TARGET_KEYS), SOURCE, max_size=len(TARGET_KEYS)
    ),
    assigned=st.one_of(st.none(), FLAG),
)
def test_effective_flags_match_cargo_source_precedence(
    triple: str,
    build: object,
    targets: dict[str, object],
    assigned: str | None,
) -> None:
    """Assigned flags replace config; matching target tables replace build."""
    config: dict[str, object] = {
        "build": {"rustflags": build},
        "target": {key: {"rustflags": value} for key, value in targets.items()},
    }
    actual = effective_rustflags(config, triple, assigned)
    expected = _reference_effective(config, triple, assigned)
    assert actual == expected, (
        f"triple={triple}, assigned={assigned!r}, config={config!r}, "
        f"expected={expected!r}, actual={actual!r}"
    )
