"""Share synthetic Nextest records and fixture metadata across contracts."""

from __future__ import annotations

import json
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

from nextest_fixture import EXPECTED_FIXTURE_BINARIES, Fixture
from nextest_process import ProcessResult


@dataclass(frozen=True, slots=True)
class ProcessOutcome:
    """Group the exit status and captured streams of a synthetic command."""

    returncode: int = 0
    stdout: str = ""
    stderr: str = ""


@dataclass(frozen=True, slots=True)
class SyntheticTiming:
    """Describe a synthetic command's start time and elapsed duration."""

    started_epoch_seconds: float = 0.0
    elapsed_seconds: float = 0.0


def synthetic_process_result(
    phase: str,
    command: Sequence[str],
    outcome: ProcessOutcome = ProcessOutcome(),
    timing: SyntheticTiming = SyntheticTiming(),
) -> ProcessResult:
    """Build one process record from its distinct outcome and timing data."""
    started_at = datetime.fromtimestamp(
        timing.started_epoch_seconds, timezone.utc
    )
    return ProcessResult(
        phase=phase,
        command=tuple(command),
        started_at=started_at,
        finished_at=started_at + timedelta(seconds=timing.elapsed_seconds),
        elapsed_seconds=timing.elapsed_seconds,
        returncode=outcome.returncode,
        stdout=outcome.stdout,
        stderr=outcome.stderr,
    )


def binary_records(names: Iterable[str]) -> dict[str, dict[str, str]]:
    """Build binary summary records keyed by fixture target identifiers."""
    return {
        f"fixture-target-{index}": {"binary-name": name}
        for index, name in enumerate(sorted(names))
    }


def binary_inventory(
    names: Sequence[str] = tuple(EXPECTED_FIXTURE_BINARIES),
) -> dict[str, object]:
    """Build the binary-summary fields checked by preparation validation."""
    return {
        "rust-build-meta": {},
        "rust-binaries": binary_records(names),
    }


def fixture_with_saved_metadata(root: Path) -> Fixture:
    """Create valid saved probe inputs without running Cargo or Nextest."""
    crate = root / "crate"
    crate.mkdir(parents=True, exist_ok=True)
    cargo_metadata = root / "cargo-metadata.json"
    cargo_metadata.write_text(
        json.dumps({"packages": [{"name": "nextest-boundary-fixture"}]}),
        encoding="utf-8",
    )
    binaries_metadata = root / "binaries-metadata.json"
    binaries_metadata.write_text(
        json.dumps(binary_inventory()),
        encoding="utf-8",
    )
    empty_record = synthetic_process_result("preparation", ())
    return Fixture(
        crate=crate,
        target=root / "target",
        cargo_metadata=cargo_metadata,
        binaries_metadata=binaries_metadata,
        preparation_records=(empty_record, empty_record),
    )
