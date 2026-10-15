"""Shared inputs and manifest selection for Cargo audit contracts."""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path

def selected_cargo_manifests(
    utility_bin: Path,
    repository_root: Path,
) -> list[str]:
    """Return Cargo.toml paths selected by the Makefile audit find expression."""
    result = subprocess.run(
        [
            str(utility_bin / "find"),
            ".",
            "(",
            "-path",
            "*/target/*",
            "-o",
            "-path",
            "*/node_modules/*",
            "-o",
            "-path",
            "*/.venv/*",
            "-o",
            "(",
            "-path",
            "./crates/*",
            "!",
            "-path",
            "./crates/libsql",
            "!",
            "-path",
            "./crates/libsql/*",
            ")",
            ")",
            "-prune",
            "-o",
            "-name",
            "Cargo.toml",
            "-print",
        ],
        cwd=repository_root,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.splitlines()


@dataclass(frozen=True)
class CargoAuditCase:
    """Immutable inputs and expected executable for one audit case.

    Parameters
    ----------
    cargo_override : str or None
        Optional value supplied through the ``CARGO`` environment variable.
    has_path_cargo : bool
        Whether the test provides Cargo on ``PATH``.
    has_home_cargo : bool
        Whether the test provides Cargo under ``$HOME/.cargo/bin``.
    expected_location : str
        Expected source of the executable: ``path``, ``home``, or ``override``.

    Returns
    -------
    CargoAuditCase
        Immutable case describing the environment and expected resolution.
    """

    cargo_override: str | None
    has_path_cargo: bool
    has_home_cargo: bool
    expected_location: str
