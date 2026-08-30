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


def write_lockfile(path: Path, unrelated_dependency: str | None = None) -> None:
    """Write the minimum lock graph containing the two documented audit paths."""
    root_dependencies = ' "rust_decimal", "libsql",'
    if unrelated_dependency is not None:
        root_dependencies += f' "{unrelated_dependency}",'

    path.write_text(
        f'''\
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
dependencies = ["h2"]

[[package]]
name = "h2"
version = "0.3.27"
'''
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
    """Permit the documented rust_decimal and libSQL dependency paths."""
    lockfile = tmp_path / "Cargo.lock"
    write_lockfile(lockfile)

    assert verify(lockfile).returncode == 0


def test_audit_ignores_allow_locks_without_the_ignored_packages(tmp_path: Path) -> None:
    """Allow manifests whose lockfile contains neither path-scoped advisory."""
    lockfile = tmp_path / "Cargo.lock"
    lockfile.write_text(
        '''\
version = 4

[[package]]
name = "extension"
version = "1.0.0"
'''
    )

    assert verify(lockfile).returncode == 0


@pytest.mark.parametrize("unrelated_dependency", ("rkyv", "h2"))
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
