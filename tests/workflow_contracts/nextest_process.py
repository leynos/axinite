"""Run bounded commands in isolated process groups."""

from __future__ import annotations

import os
import signal
import subprocess
import time
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

TERMINATION_GRACE_SECONDS = 2.0


@dataclass(frozen=True, slots=True)
class ProcessContext:
    """Carry the working directory and environment for one command."""

    cwd: Path
    environment: Mapping[str, str]


@dataclass(frozen=True, slots=True)
class ProcessResult:
    """Capture one command's identity, timing, exit status and output."""

    phase: str
    command: tuple[str, ...]
    started_at: datetime
    finished_at: datetime
    elapsed_seconds: float
    returncode: int
    stdout: str
    stderr: str


class ProcessStartError(RuntimeError):
    """Identify a command that could not start for a contract phase."""

    def __init__(
        self, phase: str, command: Sequence[str], cause: OSError
    ) -> None:
        """Retain the phase, argv and operating-system start failure.

        Parameters
        ----------
        phase
            Contract phase that attempted to launch the command.
        command
            Argument vector passed to the process launcher.
        cause
            Operating-system error raised while starting the command.
        """
        self.phase = phase
        self.command = tuple(command)
        super().__init__(f"could not start {phase}: {self.command!r}: {cause}")


class ProcessTimeout(subprocess.TimeoutExpired):
    """Keep timeout semantics together with the reaped command-leader record."""

    def __init__(self, record: ProcessResult, timeout_seconds: float) -> None:
        """Expose captured output and the completed process record.

        Parameters
        ----------
        record
            Command result after its process group was signalled and leader
            reaped.
        timeout_seconds
            Watchdog limit that elapsed before the command completed.
        """
        super().__init__(
            list(record.command),
            timeout_seconds,
            output=record.stdout,
            stderr=record.stderr,
        )
        self.record = record


def _signal_process_group(process: subprocess.Popen[str], sig: signal.Signals) -> None:
    """Signal the isolated command group, ignoring only an already-gone group."""
    try:
        os.killpg(process.pid, sig)
    except ProcessLookupError:
        pass


def _stop_process_group(process: subprocess.Popen[str]) -> tuple[str, str]:
    """Terminate the process group and reap its command leader."""
    _signal_process_group(process, signal.SIGTERM)
    try:
        stdout, stderr = process.communicate(timeout=TERMINATION_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        _signal_process_group(process, signal.SIGKILL)
        stdout, stderr = process.communicate()
    else:
        # The leader may exit while a child in the group keeps running with
        # closed output streams. A final group signal stops any remaining child.
        _signal_process_group(process, signal.SIGKILL)
    return stdout, stderr


def run_process(
    command: Sequence[str],
    *,
    phase: str,
    timeout_seconds: float,
    context: ProcessContext,
) -> ProcessResult:
    """Run a fixed argv command and return its bounded execution record.

    Non-zero exits are returned so behavioural contracts can inspect them.
    Timeouts remain ``TimeoutExpired`` exceptions after the command process
    group is signalled and its leader is reaped; their record retains output.
    """
    if timeout_seconds <= 0:
        raise ValueError("a process timeout must be positive")

    argv = tuple(command)
    started_at = datetime.now(timezone.utc)
    started_clock = time.monotonic()
    try:
        process = subprocess.Popen(
            argv,
            cwd=context.cwd,
            env=dict(context.environment),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            start_new_session=True,
        )
    except OSError as error:
        raise ProcessStartError(phase, argv, error) from error

    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as timeout_error:
        stdout, stderr = _stop_process_group(process)
        finished_at = datetime.now(timezone.utc)
        finished_clock = time.monotonic()
        record = ProcessResult(
            phase=phase,
            command=argv,
            started_at=started_at,
            finished_at=finished_at,
            elapsed_seconds=finished_clock - started_clock,
            returncode=process.returncode if process.returncode is not None else -1,
            stdout=stdout,
            stderr=stderr,
        )
        raise ProcessTimeout(record, timeout_seconds) from timeout_error

    finished_at = datetime.now(timezone.utc)
    finished_clock = time.monotonic()
    return ProcessResult(
        phase=phase,
        command=argv,
        started_at=started_at,
        finished_at=finished_at,
        elapsed_seconds=finished_clock - started_clock,
        returncode=process.returncode,
        stdout=stdout,
        stderr=stderr,
    )
