"""Failure contracts for the unsafe-boundary input readers and tree walks."""

from __future__ import annotations

import errno
import sys
import typing as typ
from collections.abc import Callable, Iterator
from pathlib import Path

import pytest

REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "scripts"))

from unsafe_boundaries import files  # noqa: E402


def test_required_read_reports_a_missing_path(tmp_path: Path) -> None:
    """A missing required source cannot look like an empty Rust file."""
    missing = tmp_path / "src" / "lib.rs"

    with pytest.raises(files.BoundaryInputError) as error:
        files.read_text(missing)

    assert str(missing) in str(error.value)


def test_optional_read_returns_none_only_for_a_missing_target(tmp_path: Path) -> None:
    """Crates may omit target roots, while present roots retain their contents."""
    optional = tmp_path / "build.rs"
    assert files.read_optional_text(optional) is None

    optional.write_text("fn main() {}", encoding="utf-8")
    assert files.read_optional_text(optional) == "fn main() {}"


@pytest.mark.parametrize("optional", (False, True), ids=("required", "optional"))
def test_read_reports_permission_errors_with_the_path(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    optional: bool,
) -> None:
    """An unreadable source fails closed even when the test runs as root."""
    unreadable = tmp_path / "src" / "lib.rs"
    original_read_text = Path.read_text

    def deny_read(path: Path, *args: typ.Any, **kwargs: typ.Any) -> str:
        if path == unreadable:
            raise PermissionError(errno.EACCES, "permission denied", str(path))
        return original_read_text(path, *args, **kwargs)

    monkeypatch.setattr(Path, "read_text", deny_read)
    with pytest.raises(files.BoundaryInputError) as error:
        reader = files.read_optional_text if optional else files.read_text
        reader(unreadable)

    assert str(unreadable) in str(error.value)


@pytest.mark.parametrize("scan", ("manifests", "sources"))
def test_directory_walk_errors_fail_with_the_path(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    scan: str,
) -> None:
    """Unreadable subdirectories cannot silently disappear from the scan."""
    blocked = tmp_path / "crates" / "blocked"

    def fail_walk(
        root: Path,
        *,
        onerror: Callable[[OSError], None],
    ) -> Iterator[tuple[str, list[str], list[str]]]:
        assert root == tmp_path
        error = PermissionError(errno.EACCES, "permission denied", str(blocked))
        onerror(error)
        return iter(())

    monkeypatch.setattr(files.os, "walk", fail_walk)
    with pytest.raises(files.BoundaryInputError) as error:
        if scan == "manifests":
            files.discover_manifests(tmp_path)
        else:
            list(files.rust_sources(tmp_path))

    assert str(blocked) in str(error.value)
