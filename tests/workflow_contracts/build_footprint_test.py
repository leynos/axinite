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

import os
import re
import shutil
import subprocess
import tomllib
import typing as typ
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


@pytest.mark.parametrize(
    "manifest_path", MANIFESTS, ids=lambda p: str(p.relative_to(ROOT))
)
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


# --- Effective Cargo behaviour -------------------------------------------------
#
# The checks above read files. These run the pinned Cargo on a one-file crate
# that carries this repository's own ``[profile.dev]`` table and Cargo
# configuration, and read the rustc command lines Cargo prints with ``-v``, so
# the settings are proved at the Cargo boundary and not only on the page.

FIXTURE_LIB: typ.Final[str] = "pub fn footprint() {}\n"
RUSTC_LINE: typ.Final[re.Pattern[str]] = re.compile(
    r"Running `.*--crate-name footprint .*`"
)
WRAPPER_VARIABLES: typ.Final[tuple[str, ...]] = (
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "RUSTFLAGS",
    "CARGO_INCREMENTAL",
    "CARGO_PROFILE_DEV_DEBUG",
    "CARGO_PROFILE_DEV_INCREMENTAL",
)


def _dev_profile_text() -> str:
    """Return the repository's ``[profile.dev]`` table text, header included."""
    manifest = (ROOT / "Cargo.toml").read_text("utf-8")
    match = re.search(r"^\[profile\.dev\]\n(?:(?!\[).*\n?)*", manifest, re.MULTILINE)
    assert match is not None, "Cargo.toml must carry a [profile.dev] table"
    return match.group(0)


def _compile_fixture(tmp_path: Path, extra_env: dict[str, str] | None = None) -> str:
    """Build the fixture crate and return the rustc command line Cargo ran.

    The fixture holds the repository's Cargo configuration and ``[profile.dev]``
    table, and builds under the repository's pinned channel with every
    wrapper and profile override removed from the environment, so only the
    repository's own settings and the override under test apply.
    """
    channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text("utf-8"))[
        "toolchain"
    ]["channel"]
    crate = tmp_path / "footprint"
    (crate / "src").mkdir(parents=True)
    (crate / ".cargo").mkdir()
    (crate / "Cargo.toml").write_text(
        '[package]\nname = "footprint"\nversion = "0.0.0"\nedition = "2024"\n\n'
        + _dev_profile_text(),
        "utf-8",
    )
    (crate / "src" / "lib.rs").write_text(FIXTURE_LIB, "utf-8")
    shutil.copy(ROOT / ".cargo" / "config.toml", crate / ".cargo" / "config.toml")
    env = {k: v for k, v in os.environ.items() if k not in WRAPPER_VARIABLES}
    env["CARGO_TARGET_DIR"] = str(tmp_path / "target")
    env.update(extra_env or {})
    cargo = shutil.which("cargo")
    assert cargo is not None, "the contract suite requires Cargo"
    result = subprocess.run(  # noqa: S603 - a fixed command
        [cargo, f"+{channel}", "build", "--color", "never", "--offline", "-v"],
        cwd=crate,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, f"cargo build failed:\n{result.stderr}"
    lines = RUSTC_LINE.findall(result.stderr)
    assert lines, f"no rustc command line in Cargo's output:\n{result.stderr}"
    return lines[0]


def test_cargo_builds_by_default_without_incremental_state_or_debug_info(
    tmp_path: Path,
) -> None:
    """A default dev build passes no ``debuginfo`` or ``incremental`` and no embedding."""
    command = _compile_fixture(tmp_path)
    assert "-C debuginfo" not in command, f"debug info must be off: {command}"
    assert "-C incremental" not in command, f"incremental must be off: {command}"
    assert "-Z embed-metadata=no" in command, (
        f"Cargo must coordinate metadata natively: {command}"
    )


def test_the_documented_debug_override_turns_debug_info_back_on(
    tmp_path: Path,
) -> None:
    """``CARGO_PROFILE_DEV_DEBUG=true`` restores debug information."""
    command = _compile_fixture(tmp_path, {"CARGO_PROFILE_DEV_DEBUG": "true"})
    assert "-C debuginfo=2" in command, (
        f"the override must restore debug info: {command}"
    )


def test_the_documented_incremental_override_turns_incremental_back_on(
    tmp_path: Path,
) -> None:
    """``CARGO_INCREMENTAL=1`` restores incremental compilation."""
    command = _compile_fixture(tmp_path, {"CARGO_INCREMENTAL": "1"})
    assert "-C incremental=" in command, (
        f"the override must restore incremental: {command}"
    )
