"""Read the manifests and sources the contract judges."""

from __future__ import annotations

import os
import typing as typ
from pathlib import Path

import tomllib

from .inventory import PRUNED_DIRECTORIES

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterator


class BoundaryInputError(ValueError):
    """A required boundary-contract input could not be read or traversed."""


def read_text(path: Path) -> str:
    """Return a required file's text, naming the path when reading fails."""
    try:
        return path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        message = f"{path}: cannot read required text: {error}"
        raise BoundaryInputError(message) from error


def read_optional_text(path: Path) -> str | None:
    """Read an optional target root, returning `None` only when it is absent."""
    try:
        return path.read_text(encoding="utf-8")
    except FileNotFoundError:
        return None
    except (OSError, UnicodeError) as error:
        message = f"{path}: cannot read optional text: {error}"
        raise BoundaryInputError(message) from error


def read_manifest(path: Path) -> dict[str, typ.Any]:
    """Parse a manifest, raising `BoundaryInputError` naming it on failure."""
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        message = f"{path}: cannot read manifest: {error}"
        raise BoundaryInputError(message) from error


def _raise_walk_error(error: OSError) -> None:
    """Stop a scan when the operating system cannot enumerate a directory."""
    location = error.filename or "<unknown path>"
    message = f"{location}: cannot traverse directory: {error}"
    raise BoundaryInputError(message) from error


def discover_manifests(root: Path) -> set[str]:
    """Return the repository-relative directory of every `Cargo.toml`."""
    found: set[str] = set()
    for directory, names, files in os.walk(root, onerror=_raise_walk_error):
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
    for directory, names, files in os.walk(crate_dir, onerror=_raise_walk_error):
        names[:] = [
            name
            for name in names
            if name not in PRUNED_DIRECTORIES
            and not (Path(directory) / name / "Cargo.toml").exists()
        ]
        for name in files:
            if name.endswith(".rs"):
                yield Path(directory) / name
