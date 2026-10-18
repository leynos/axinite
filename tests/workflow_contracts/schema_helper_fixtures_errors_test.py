"""Keep fixture inventory acquisition failures inside its source boundary."""

import errno
from collections.abc import Iterator
from pathlib import Path

import pytest
from contract_sources import SourceReadError
from schema_helper_fixtures import read_fixture_inventory


def test_missing_pass_directory_raises_source_read_error(tmp_path: Path) -> None:
    """A missing fixture directory cannot look like an empty inventory."""
    pass_directory = tmp_path / "pass"

    with pytest.raises(SourceReadError) as raised:
        read_fixture_inventory(tmp_path)

    assert raised.value.path == pass_directory, (
        "the missing pass directory must be named"
    )
    assert isinstance(raised.value.__cause__, FileNotFoundError), (
        "the missing pass directory error must remain chained"
    )


def test_unlistable_pass_directory_raises_source_read_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A listing failure cannot silently omit every fixture."""
    pass_directory = tmp_path / "pass"
    pass_directory.mkdir()
    original_iterdir = Path.iterdir
    cause = PermissionError(errno.EACCES, "permission denied", str(pass_directory))

    def fail_listing(path: Path) -> Iterator[Path]:
        if path == pass_directory:
            raise cause
        return original_iterdir(path)

    monkeypatch.setattr(Path, "iterdir", fail_listing)

    with pytest.raises(SourceReadError) as raised:
        read_fixture_inventory(tmp_path)

    assert raised.value.path == pass_directory, (
        "the unlistable pass directory must be named"
    )
    assert raised.value.__cause__ is cause, (
        "the listing failure must remain chained"
    )


def test_missing_main_source_raises_source_read_error(tmp_path: Path) -> None:
    """A missing harness cannot be read as an empty source file."""
    (tmp_path / "pass").mkdir()
    main_source = tmp_path / "main.rs"

    with pytest.raises(SourceReadError) as raised:
        read_fixture_inventory(tmp_path)

    assert raised.value.path == main_source, (
        "the missing main source must be named"
    )
    assert isinstance(raised.value.__cause__, FileNotFoundError), (
        "the missing main source error must remain chained"
    )


def test_unreadable_main_source_raises_source_read_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A harness read failure retains its original operating-system cause."""
    (tmp_path / "pass").mkdir()
    main_source = tmp_path / "main.rs"
    original_read_text = Path.read_text
    cause = PermissionError(errno.EACCES, "permission denied", str(main_source))

    def fail_read(
        path: Path, encoding: str | None = None, errors: str | None = None
    ) -> str:
        if path == main_source:
            raise cause
        return original_read_text(path, encoding=encoding, errors=errors)

    monkeypatch.setattr(Path, "read_text", fail_read)

    with pytest.raises(SourceReadError) as raised:
        read_fixture_inventory(tmp_path)

    assert raised.value.path == main_source, (
        "the unreadable main source must be named"
    )
    assert raised.value.__cause__ is cause, (
        "the main-source read failure must remain chained"
    )


def test_invalid_utf8_main_source_raises_source_read_error(tmp_path: Path) -> None:
    """An undecodable harness is an acquisition failure with a path."""
    (tmp_path / "pass").mkdir()
    main_source = tmp_path / "main.rs"
    main_source.write_bytes(b"\xff")

    with pytest.raises(SourceReadError) as raised:
        read_fixture_inventory(tmp_path)

    assert raised.value.path == main_source, (
        "the undecodable main source must be named"
    )
    assert isinstance(raised.value.__cause__, UnicodeDecodeError), (
        "the UTF-8 decoding error must remain chained"
    )
