"""Contract test for the development build footprint rule.

The Concordat ``rust-dev-build-footprint`` rule needs every default development
profile to disable incremental compilation and debug information. The root
workspace and each standalone workspace under ``channels-src``, ``tools-src``
and ``fuzz`` carry their own ``[profile.dev]``, so an edit to any of them must
not quietly re-enable either.

The nightly pin is past 2026-07-31, so ``.cargo/config.toml`` also carries the
native ``[unstable] embed-metadata = false`` key (DF-003).

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
MANIFESTS = sorted(
    [
        ROOT / "Cargo.toml",
        ROOT / "fuzz" / "Cargo.toml",
        *(ROOT / "channels-src").glob("*/Cargo.toml"),
        *(ROOT / "tools-src").glob("*/Cargo.toml"),
    ]
)


def _is_zero_debug(debug: object) -> bool:
    """Accept the exact integer ``0`` or the boolean ``false``.

    Python equality would also accept ``0.0``, which Cargo rejects.
    """
    is_zero_integer = (
        isinstance(debug, int) and not isinstance(debug, bool) and debug == 0
    )
    return debug is False or is_zero_integer


def test_the_manifest_list_is_not_empty() -> None:
    """A moved directory must not leave the contract reading nothing."""
    assert len(MANIFESTS) >= 16, f"expected at least 16 manifests, found {MANIFESTS}"


@pytest.mark.parametrize("manifest_path", MANIFESTS, ids=lambda p: str(p.relative_to(ROOT)))
def test_development_profile_keeps_the_build_footprint_settings(
    manifest_path: Path,
) -> None:
    """Each workspace root sets ``incremental = false`` and ``debug = 0``."""
    manifest = tomllib.loads(manifest_path.read_text("utf-8"))
    dev = manifest.get("profile", {}).get("dev", {})
    assert dev.get("incremental") is False, (
        f"{manifest_path.relative_to(ROOT)}: profile.dev must set incremental = false"
    )
    assert _is_zero_debug(dev.get("debug")), (
        f"{manifest_path.relative_to(ROOT)}: profile.dev must set debug = 0 or false"
    )


def test_root_dependency_profile_disables_debug_info() -> None:
    """The root dependency profile sets ``debug = 0`` and keeps split debuginfo."""
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text("utf-8"))
    dependencies = (
        manifest.get("profile", {}).get("dev", {}).get("package", {}).get("*", {})
    )
    assert _is_zero_debug(dependencies.get("debug")), (
        'profile.dev.package."*" must set debug = 0 or false'
    )
    assert dependencies.get("split-debuginfo") == "unpacked", (
        'profile.dev.package."*" must keep split-debuginfo = "unpacked"'
    )


def test_cargo_config_disables_metadata_embedding_natively() -> None:
    """``.cargo/config.toml`` sets the native ``[unstable] embed-metadata = false``.

    The pinned dated nightly is past 2026-07-31, so the native key replaces the
    older ``no-embed-metadata`` spelling. Raw rustc metadata flags are refused
    because they broke build scripts.
    """
    config_path = ROOT / ".cargo" / "config.toml"
    config = tomllib.loads(config_path.read_text("utf-8"))
    assert config.get("unstable", {}).get("embed-metadata") is False, (
        ".cargo/config.toml must set [unstable] embed-metadata = false"
    )
    flags = [
        *config.get("build", {}).get("rustflags", []),
        *(
            flag
            for table in config.get("target", {}).values()
            for flag in table.get("rustflags", [])
        ),
    ]
    assert not any("embed-metadata" in str(flag) for flag in flags), (
        "no raw rustc embed-metadata flag may appear in any rustflags source"
    )
