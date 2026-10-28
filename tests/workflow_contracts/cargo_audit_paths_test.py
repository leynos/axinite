"""Contracts for path-scoped RustSec ignores in the manifest audit sweep.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import pytest

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
VERIFY_SCRIPT = REPOSITORY_ROOT / "scripts" / "verify_audit_ignore_paths.py"


def write_lockfile(
    path: Path,
    unrelated_dependency: str | None = None,
    additional_packages: str = "",
) -> None:
    """Write the minimum lock graph containing all documented audit paths."""
    root_dependencies = (
        ' "rust_decimal", "libsql", "pdf-extract", "wasmtime-internal-cache",'
    )
    if unrelated_dependency is not None:
        root_dependencies += f' "{unrelated_dependency}",'

    path.write_text(
        f"""\
version = 4

[[package]]
name = "axinite"
version = "0.18.0"
dependencies = [{root_dependencies}]

[[package]]
name = "rust_decimal"
version = "1.42.0"
dependencies = ["rkyv"]

[[package]]
name = "rkyv"
version = "0.7.46"

[[package]]
name = "libsql"
version = "0.9.30"
dependencies = ["h2 0.3.27"]

[[package]]
name = "h2"
version = "0.3.27"

[[package]]
name = "pdf-extract"
version = "0.12.0"
dependencies = ["lopdf"]

[[package]]
name = "lopdf"
version = "0.42.0"
dependencies = ["ttf-parser"]

[[package]]
name = "ttf-parser"
version = "0.25.1"
[[package]]
name = "wasmtime-internal-cache"
version = "48.0.7"
dependencies = ["zstd 0.13.3"]

[[package]]
name = "zstd"
version = "0.13.3"
dependencies = ["zstd-safe 7.2.4"]

[[package]]
name = "zstd-safe"
version = "7.2.4"
dependencies = ["zstd-sys 2.0.16+zstd.1.5.7"]

[[package]]
name = "zstd-sys"
version = "2.0.16+zstd.1.5.7"
{additional_packages}
"""
    )


def verify(lockfile: Path) -> subprocess.CompletedProcess[str]:
    """Run the audit-ignore validator against one lockfile fixture."""
    return subprocess.run(
        [sys.executable, VERIFY_SCRIPT, lockfile],
        check=False,
        capture_output=True,
        text=True,
    )


def test_audit_ignores_accept_documented_dependency_paths(tmp_path: Path) -> None:
    """Permit the documented rust_decimal, libSQL and pdf-extract dependency paths."""
    lockfile = tmp_path / "Cargo.lock"
    write_lockfile(lockfile)

    assert verify(lockfile).returncode == 0


def test_audit_ignores_allow_locks_without_the_ignored_packages(tmp_path: Path) -> None:
    """Allow manifests whose lockfile contains neither path-scoped advisory."""
    lockfile = tmp_path / "Cargo.lock"
    lockfile.write_text(
        """\
version = 4

[[package]]
name = "extension"
version = "1.0.0"
"""
    )

    assert verify(lockfile).returncode == 0


def test_audit_ignores_accept_fixed_h2_version_outside_libsql(
    tmp_path: Path,
) -> None:
    """Keep the vulnerable h2 path scoped while allowing its fixed release."""
    lockfile = tmp_path / "Cargo.lock"
    write_lockfile(lockfile)
    contents = lockfile.read_text().replace(
        '"wasmtime-internal-cache",',
        '"wasmtime-internal-cache", "h2 0.4.16",',
        1,
    )
    contents += '\n[[package]]\nname = "h2"\nversion = "0.4.16"\n'
    lockfile.write_text(contents)

    assert verify(lockfile).returncode == 0


@pytest.mark.parametrize(
    "unrelated_dependency",
    ["rkyv", "h2", "ttf-parser", "zstd", "zstd-safe", "zstd-sys"],
)
def test_audit_ignores_reject_unrelated_dependency_paths(
    tmp_path: Path,
    unrelated_dependency: str,
) -> None:
    """Reject an ignored advisory that gains an unrelated root dependency path."""
    lockfile = tmp_path / "Cargo.lock"
    write_lockfile(lockfile, unrelated_dependency)

    result = verify(lockfile)

    assert result.returncode == 1
    assert f"{unrelated_dependency} is not exclusively reachable" in result.stderr


def test_audit_ignores_accept_cyclic_paths_to_expected_ancestor(
    tmp_path: Path,
) -> None:
    """Permit cycles when every reverse path can still reach rust_decimal."""
    lockfile = tmp_path / "Cargo.lock"
    write_lockfile(
        lockfile,
        additional_packages="""\

[[package]]
name = "cycle_a"
version = "1.0.0"
dependencies = ["rkyv", "cycle_b"]

[[package]]
name = "cycle_b"
version = "1.0.0"
dependencies = ["cycle_a"]

[[package]]
name = "rust_decimal"
version = "1.43.0"
dependencies = ["cycle_a"]
""",
    )

    assert verify(lockfile).returncode == 0


def test_audit_ignores_reject_reverse_path_without_expected_ancestor(
    tmp_path: Path,
) -> None:
    """Reject a reverse path that terminates before reaching rust_decimal."""
    lockfile = tmp_path / "Cargo.lock"
    write_lockfile(
        lockfile,
        additional_packages="""\

[[package]]
name = "orphan"
version = "1.0.0"
dependencies = ["rkyv"]
""",
    )

    result = verify(lockfile)

    assert result.returncode == 1
    assert "rkyv is not exclusively reachable" in result.stderr
