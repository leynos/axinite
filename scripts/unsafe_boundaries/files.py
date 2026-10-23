"""Read the manifests and sources the contract judges."""

from __future__ import annotations

import os
import typing as typ
from pathlib import Path

import tomllib

from .inventory import PRUNED_DIRECTORIES

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator


def read_text(path: Path) -> str:
    """Return a file's text, or an empty string when it does not exist."""
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return ""


def read_manifest(path: Path) -> dict[str, typ.Any]:
    """Parse a manifest, raising `ValueError` naming it on failure."""
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        message = f"{path}: cannot read manifest: {error}"
        raise ValueError(message) from error


def discover_manifests(root: Path) -> set[str]:
    """Return the repository-relative directory of every `Cargo.toml`."""
    found: set[str] = set()
    for directory, names, files in os.walk(root):
        names[:] = [name for name in names if name not in PRUNED_DIRECTORIES]
        if "Cargo.toml" in files:
            relative = Path(directory).relative_to(root).as_posix()
            found.add(relative)
    return found


def rust_sources(crate_dir: Path) -> Iterator[Path]:
    """Yield every Rust source belonging to a crate, whatever its target.

    A nested directory with its own manifest is a different crate, judged by
    its own inventory entry or exemption, so it is not descended into.
    """
    for directory, names, files in os.walk(crate_dir):
        names[:] = [
            name
            for name in names
            if name not in PRUNED_DIRECTORIES
            and not (Path(directory) / name / "Cargo.toml").exists()
        ]
        for name in files:
            if name.endswith(".rs"):
                yield Path(directory) / name
