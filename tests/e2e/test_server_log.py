"""Tests for the server-log capture in conftest.py.

These need no server or browser: they point the capture at a temporary file.
"""

from pathlib import Path
from types import SimpleNamespace

import conftest
import pytest
from helpers import AUTH_TOKEN


@pytest.fixture
def server_log(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """Point the capture at a log file inside the test's temporary directory."""
    log = tmp_path / "axinite-server.log"
    monkeypatch.setattr(conftest, "_SERVER_LOG", log)
    return log


def test_missing_log_is_reported_as_unavailable(server_log: Path) -> None:
    """A log that cannot be read must not read as a silent server."""
    assert "server log unavailable" in conftest._server_log_tail()


def test_empty_log_is_an_empty_tail(server_log: Path) -> None:
    """An existing but empty log is empty, not unavailable."""
    server_log.write_bytes(b"")
    assert conftest._server_log_tail() == ""


def test_short_log_is_returned_whole(server_log: Path) -> None:
    server_log.write_text("started\nlistening\n", encoding="utf-8")
    assert conftest._server_log_tail() == "started\nlistening\n"


def test_long_log_is_cut_to_the_last_tail_bytes(server_log: Path) -> None:
    """Only the end of an over-long log is returned."""
    limit = conftest._SERVER_LOG_TAIL_BYTES
    server_log.write_bytes(b"a" * limit + b"b" * limit)
    assert conftest._server_log_tail() == "b" * limit


def test_invalid_utf8_is_replaced_not_raised(server_log: Path) -> None:
    server_log.write_bytes(b"ok \xff\xfe end")
    tail = conftest._server_log_tail()
    assert tail.startswith("ok ") and tail.endswith(" end")
    assert "�" in tail


def test_the_auth_token_is_redacted(server_log: Path) -> None:
    server_log.write_text(f"GET /?token={AUTH_TOKEN} 200\n", encoding="utf-8")
    tail = conftest._server_log_tail()
    assert AUTH_TOKEN not in tail
    assert "token=***" in tail


def _run_hook(failed: bool, when: str) -> SimpleNamespace:
    """Drive the report hook wrapper by hand and return the report it saw."""
    report = SimpleNamespace(failed=failed, sections=[])
    hook = conftest.pytest_runtest_makereport(
        item=SimpleNamespace(), call=SimpleNamespace(when=when)
    )
    next(hook)
    with pytest.raises(StopIteration):
        hook.send(SimpleNamespace(get_result=lambda: report))
    return report


@pytest.mark.parametrize("when", ["setup", "call", "teardown"])
def test_a_failed_phase_gets_the_server_log(server_log: Path, when: str) -> None:
    """The page fixture fails in setup, so setup must be covered too."""
    server_log.write_text("the server's view\n", encoding="utf-8")
    report = _run_hook(failed=True, when=when)
    assert report.sections == [
        ("axinite server log (tail)", "the server's view\n")
    ]


def test_a_passing_phase_is_left_alone(server_log: Path) -> None:
    server_log.write_text("noise\n", encoding="utf-8")
    assert _run_hook(failed=False, when="call").sections == []
