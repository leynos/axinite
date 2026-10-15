"""Specify preparation budgets, metadata reuse and timeout cleanup."""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from collections.abc import Sequence
from pathlib import Path

import pytest
import nextest_fixture as fixture_support
from nextest_fixture import FixturePreparationError
from nextest_process import ProcessContext, ProcessResult, ProcessTimeout, run_process
from nextest_test_support import (
    ProcessOutcome,
    SyntheticTiming,
    binary_inventory,
    fixture_with_saved_metadata,
    synthetic_process_result,
)


def test_preparation_uses_one_deadline_and_probes_keep_their_600_second_cap(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """Synthetic phases share 1200 s; a later probe still gets only 600 s."""
    root = tmp_path / "preparation"
    crate = root / "crate"
    crate.mkdir(parents=True)
    target = tmp_path / "target"
    now = [100.0]
    calls: list[tuple[tuple[str, ...], float]] = []

    def fake_process(
        command: Sequence[str],
        *,
        phase: str,
        timeout_seconds: float,
        context: ProcessContext,
    ) -> ProcessResult:
        del context
        calls.append((tuple(command), timeout_seconds))
        elapsed = 850.0 if command[1:2] == ("metadata",) else 349.0
        if command[1:2] == ("metadata",):
            stdout = json.dumps(
                {"packages": [{"name": "nextest-boundary-fixture"}]}
            )
        else:
            stdout = json.dumps(binary_inventory())
        result = synthetic_process_result(
            phase,
            command,
            ProcessOutcome(stdout=stdout),
            SyntheticTiming(
                started_epoch_seconds=now[0], elapsed_seconds=elapsed
            ),
        )
        now[0] += elapsed
        return result

    monkeypatch.setattr(fixture_support, "run_process", fake_process)
    prepared_fixture = fixture_support.prepare_fixture(
        root, crate, target, clock=lambda: now[0]
    )
    assert [timeout for _, timeout in calls] == [1200.0, 350.0]
    assert len(prepared_fixture.preparation_records) == 2
    assert prepared_fixture.preparation_records[0].phase == "cargo metadata preparation"
    inventory_phase = prepared_fixture.preparation_records[1].phase
    assert inventory_phase == "Nextest binary inventory preparation"

    probe_calls: list[tuple[str, ...]] = []

    def fake_probe(
        command: Sequence[str],
        *,
        phase: str,
        timeout_seconds: float,
        context: ProcessContext,
    ) -> ProcessResult:
        del context
        assert timeout_seconds == fixture_support.SUBPROCESS_TIMEOUT_SECONDS
        probe_calls.append(tuple(command))
        return synthetic_process_result(phase, command)

    monkeypatch.setattr(fixture_support, "run_process", fake_probe)
    fixture_support.run_nextest_probe(("cargo", "nextest", "list"), prepared_fixture)
    assert len(probe_calls) == 1
    assert "--cargo-metadata" in probe_calls[0]
    assert "--binaries-metadata" in probe_calls[0]


@pytest.mark.parametrize("failure", ["nonzero", "malformed"], ids=str)
def test_preparation_stops_before_binary_listing_on_bad_cargo_metadata(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, failure: str
) -> None:
    """A failed first phase cannot publish a fixture or fall back to Cargo."""
    root = tmp_path / "preparation"
    crate = root / "crate"
    crate.mkdir(parents=True)
    calls: list[tuple[str, ...]] = []

    def fake_process(
        command: Sequence[str],
        *,
        phase: str,
        timeout_seconds: float,
        context: ProcessContext,
    ) -> ProcessResult:
        del timeout_seconds, context
        calls.append(tuple(command))
        return synthetic_process_result(
            phase,
            command,
            ProcessOutcome(
                returncode=17 if failure == "nonzero" else 0,
                stdout="not-json" if failure == "malformed" else "",
                stderr="metadata failed",
            ),
        )

    monkeypatch.setattr(fixture_support, "run_process", fake_process)
    with pytest.raises(FixturePreparationError):
        fixture_support.prepare_fixture(
            root, crate, tmp_path / "target", clock=lambda: 1.0
        )
    assert len(calls) == 1
    assert calls[0][1:2] == ("metadata",)
    assert not (root / "binaries-metadata.json").exists()


def test_binary_inventory_failure_is_recorded_without_publishing_fixture(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """Malformed Nextest inventory cannot become a reusable fixture."""
    root = tmp_path / "preparation"
    crate = root / "crate"
    crate.mkdir(parents=True)
    commands: list[tuple[str, ...]] = []

    def fake_process(
        command: Sequence[str],
        *,
        phase: str,
        timeout_seconds: float,
        context: ProcessContext,
    ) -> ProcessResult:
        del timeout_seconds, context
        commands.append(tuple(command))
        stdout = (
            json.dumps({"packages": [{"name": "nextest-boundary-fixture"}]})
            if len(commands) == 1
            else "{broken"
        )
        return synthetic_process_result(
            phase, command, ProcessOutcome(stdout=stdout)
        )

    monkeypatch.setattr(fixture_support, "run_process", fake_process)
    with pytest.raises(FixturePreparationError) as error:
        fixture_support.prepare_fixture(
            root, crate, tmp_path / "target", clock=lambda: 1.0
        )
    assert error.value.record is not None
    assert error.value.record.phase == "Nextest binary inventory preparation"
    assert len(commands) == 2
    assert "--list-type" in commands[1]
    assert commands[1][-2:] == ("--message-format", "json")
    assert not (root / "binaries-metadata.json").exists()


def test_preparation_refuses_existing_metadata_instead_of_reusing_it(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """A reused fixture root cannot supply stale Cargo or Nextest metadata."""
    root = tmp_path / "preparation"
    crate = root / "crate"
    crate.mkdir(parents=True)
    stale = root / "cargo-metadata.json"
    stale.write_text("{}", encoding="utf-8")
    calls: list[tuple[str, ...]] = []

    def forbidden_process(
        command: Sequence[str],
        *,
        phase: str,
        timeout_seconds: float,
        context: ProcessContext,
    ) -> ProcessResult:
        del phase, timeout_seconds, context
        calls.append(tuple(command))
        return synthetic_process_result("unexpected", command)

    monkeypatch.setattr(fixture_support, "run_process", forbidden_process)
    with pytest.raises(FixturePreparationError, match="stale"):
        fixture_support.prepare_fixture(
            root, crate, tmp_path / "target", clock=lambda: 1.0
        )
    assert calls == []


@pytest.mark.parametrize(
    "metadata_name,condition",
    [
        ("cargo", "missing"),
        ("cargo", "malformed"),
        ("cargo", "undecodable"),
        ("binaries", "missing"),
        ("binaries", "malformed"),
        ("binaries", "undecodable"),
    ],
    ids=[
        "cargo-missing",
        "cargo-malformed",
        "cargo-undecodable",
        "binaries-missing",
        "binaries-malformed",
        "binaries-undecodable",
    ],
)
def test_probe_fails_closed_when_saved_metadata_is_unusable(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    metadata_name: str,
    condition: str,
) -> None:
    """Missing or corrupt preparation files never trigger a rebuild fallback."""
    fixture = fixture_with_saved_metadata(tmp_path)
    metadata_path = (
        fixture.cargo_metadata
        if metadata_name == "cargo"
        else fixture.binaries_metadata
    )
    if condition == "missing":
        metadata_path.unlink()
    elif condition == "undecodable":
        metadata_path.write_bytes(b"\xff")
    else:
        metadata_path.write_text("{broken", encoding="utf-8")
    calls: list[tuple[str, ...]] = []

    def forbidden_process(
        command: Sequence[str],
        *,
        phase: str,
        timeout_seconds: float,
        context: ProcessContext,
    ) -> ProcessResult:
        del phase, timeout_seconds, context
        calls.append(tuple(command))
        return synthetic_process_result("unexpected", command)

    monkeypatch.setattr(fixture_support, "run_process", forbidden_process)
    with pytest.raises(FixturePreparationError) as error:
        fixture_support.run_nextest_probe(
            ("cargo", "nextest", "show-config", "test-groups"), fixture
        )
    assert calls == []
    if condition == "undecodable":
        assert "not valid UTF-8" in str(error.value)


def test_timeout_preserves_output_and_terminates_its_descendant(
    tmp_path: Path,
) -> None:
    """Timeout preserves output and stops a child that ignores SIGTERM."""
    child_ready = tmp_path / "child-ready"
    child_pid = tmp_path / "child-pid"
    child_late = tmp_path / "child-late"
    child_code = (
        "import os,pathlib,signal,time; "
        "signal.signal(signal.SIGTERM, signal.SIG_IGN); "
        f"pathlib.Path({str(child_pid)!r}).write_text(str(os.getpid())); "
        f"pathlib.Path({str(child_ready)!r}).touch(); "
        "time.sleep(5); "
        f"pathlib.Path({str(child_late)!r}).touch()"
    )
    parent_code = "\n".join(
        (
            "import pathlib,subprocess,sys,time",
            f"subprocess.Popen([sys.executable, '-c', {child_code!r}])",
            f"ready=pathlib.Path({str(child_ready)!r})",
            "deadline=time.monotonic()+1",
            "while not ready.exists() and time.monotonic()<deadline:",
            "    time.sleep(.01)",
            "print('parent-started', flush=True)",
            "print('parent-error', file=sys.stderr, flush=True)",
            "time.sleep(5)",
        )
    )
    command = (sys.executable, "-c", parent_code)

    with pytest.raises(ProcessTimeout) as timeout:
        run_process(
            command,
            phase="watchdog regression",
            timeout_seconds=2.0,
            context=ProcessContext(tmp_path, dict(os.environ)),
        )

    assert isinstance(timeout.value, subprocess.TimeoutExpired)
    assert timeout.value.record.returncode is not None
    assert timeout.value.record.phase == "watchdog regression"
    assert "parent-started" in timeout.value.record.stdout
    assert "parent-error" in timeout.value.record.stderr
    assert child_ready.exists()
    descendant_pid = int(child_pid.read_text(encoding="utf-8"))
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        try:
            process_status = Path(f"/proc/{descendant_pid}/stat").read_text(
                encoding="utf-8"
            )
        except FileNotFoundError:
            break
        if process_status[process_status.rfind(")") + 2] == "Z":
            break
        time.sleep(0.02)
    else:
        pytest.fail("the timed-out command's descendant remained live")
    assert not child_late.exists()
