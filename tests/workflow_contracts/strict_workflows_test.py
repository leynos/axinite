"""Unit tests for how `read_workflows` reads a directory, and how it fails.

The estate's own directory is readable, so each failure is stated against a
temporary tree: whatever goes wrong must surface as `WorkflowReadError` naming
the path and what failed, never as a bare `OSError` or `UnicodeDecodeError`
from inside a function the coverage-publication fixture treats as a query.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import os
import typing as typ

import pytest
from _strict_workflows import WorkflowReadError, read_workflows

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from pathlib import Path

#: The smallest document the strict reader accepts.
MINIMAL = "on: push\njobs: {}\n"


def test_every_workflow_file_is_read_in_name_order(tmp_path: Path) -> None:
    """Both extensions GitHub accepts are read; anything else is not."""
    (tmp_path / "b.yaml").write_text(MINIMAL, encoding="utf-8")
    (tmp_path / "a.yml").write_text(MINIMAL, encoding="utf-8")
    (tmp_path / "notes.md").write_text("not a workflow", encoding="utf-8")
    (tmp_path / "dir.yml").mkdir()
    found = list(read_workflows(tmp_path))
    assert found == ["a.yml", "b.yaml"], f"expected the two workflows; got {found}"


def test_a_directory_that_cannot_be_listed_is_named(tmp_path: Path) -> None:
    """A missing directory fails as a read error naming it."""
    missing = tmp_path / "absent"
    with pytest.raises(WorkflowReadError, match="cannot be listed") as raised:
        read_workflows(missing)
    assert str(missing) in str(raised.value), f"the error said {raised.value}"


def test_a_workflow_that_is_not_utf8_is_named(tmp_path: Path) -> None:
    """Undecodable bytes fail as a read error naming the file."""
    path = tmp_path / "ci.yml"
    path.write_bytes(b"on: push\n\xff\xfe\n")
    with pytest.raises(WorkflowReadError, match="is not UTF-8") as raised:
        read_workflows(tmp_path)
    assert str(path) in str(raised.value), f"the error said {raised.value}"


def test_a_workflow_that_cannot_be_read_is_named(tmp_path: Path) -> None:
    """A file without read permission fails as a read error naming it."""
    path = tmp_path / "ci.yml"
    path.write_text(MINIMAL, encoding="utf-8")
    path.chmod(0)
    try:
        if os.access(path, os.R_OK):
            pytest.skip("this user reads the file regardless of its mode")
        with pytest.raises(WorkflowReadError, match="cannot be read") as raised:
            read_workflows(tmp_path)
        assert str(path) in str(raised.value), f"the error said {raised.value}"
    finally:
        path.chmod(0o600)
