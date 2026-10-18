"""Prepare fixture metadata and run Nextest probes with saved build inputs."""

from __future__ import annotations

import json
import os
import time
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from json import JSONDecodeError
from pathlib import Path

from nextest_process import ProcessContext, ProcessResult, run_process

FIXTURE_PREPARATION_TIMEOUT_SECONDS = 1200.0
SUBPROCESS_TIMEOUT_SECONDS = 600.0
EXPECTED_FIXTURE_BINARIES = frozenset({"schema_helpers_ui", "trybuild", "slow"})
JsonCheck = Callable[[str], None]


@dataclass(frozen=True, slots=True)
class Fixture:
    """Hold the copied crate and metadata produced by its preparation phases."""

    crate: Path
    target: Path
    cargo_metadata: Path
    binaries_metadata: Path
    preparation_records: tuple[ProcessResult, ProcessResult]


class FixturePreparationError(RuntimeError):
    """Report unusable fixture metadata or a failed preparation phase."""

    def __init__(self, message: str, record: ProcessResult | None = None) -> None:
        """Attach optional process evidence to the preparation failure.

        Parameters
        ----------
        message
            Structural, timing or phase failure to report.
        record
            Completed command record when a preparation process was involved.
        """
        self.record = record
        super().__init__(message)


def _environment_for_target(
    target: Path, environment_overrides: Mapping[str, str] | None = None
) -> dict[str, str]:
    """Keep fixture builds in the shared repository target directory."""
    environment = dict(os.environ)
    environment["CARGO_TARGET_DIR"] = str(target)
    environment["CARGO_BUILD_BUILD_DIR"] = str(target / "build")
    environment.update(environment_overrides or {})
    return environment


def _nextest_probe_arguments(args: Sequence[str], fixture: Fixture) -> tuple[str, ...]:
    """Insert saved build inputs into each supported Nextest probe form."""
    command = tuple(args)
    if command[:2] != ("cargo", "nextest"):
        raise ValueError("boundary probes must invoke cargo nextest")
    if command[2:4] == ("show-config", "test-groups"):
        insertion = 4
    elif command[2:3] in (("run",), ("list",)):
        insertion = 3
    else:
        raise ValueError(f"unsupported Nextest probe form: {command!r}")
    metadata = (
        "--cargo-metadata",
        str(fixture.cargo_metadata),
        "--binaries-metadata",
        str(fixture.binaries_metadata),
    )
    return (*command[:insertion], *metadata, *command[insertion:])


def _json_object(text: str, phase: str) -> dict[str, object]:
    """Decode exactly one non-empty JSON object from a preparation command."""
    try:
        value = json.loads(text)
    except JSONDecodeError as error:
        raise FixturePreparationError(
            f"{phase} did not emit one valid JSON object at character "
            f"{error.pos}: {error}"
        ) from error
    if not isinstance(value, dict) or not value:
        raise FixturePreparationError(
            f"{phase} must emit one non-empty JSON object"
        )
    return value


def _validate_cargo_metadata(text: str) -> None:
    """Require Cargo's metadata object to describe the copied fixture crate."""
    metadata = _json_object(text, "cargo metadata")
    if not _contains_fixture_package(metadata.get("packages")):
        raise FixturePreparationError(
            "cargo metadata omitted nextest-boundary-fixture"
        )


def _contains_fixture_package(packages: object) -> bool:
    """Find the copied crate by validating each Cargo package record."""
    return isinstance(packages, list) and any(
        _is_fixture_package(package) for package in packages
    )


def _is_fixture_package(package: object) -> bool:
    """Recognize only an object record naming the fixture package."""
    return (
        isinstance(package, dict)
        and package.get("name") == "nextest-boundary-fixture"
    )


def _required_object_field(
    document: Mapping[str, object], field: str
) -> dict[str, object]:
    """Read one required object from Nextest's saved inventory document."""
    value = document.get(field)
    if not isinstance(value, dict):
        raise FixturePreparationError(
            "cargo nextest binary inventory must contain object fields; "
            f"{field!r} is not an object"
        )
    return value


def _binary_record_name(record: object) -> str:
    """Read a binary name only from a structurally valid inventory record."""
    name = record.get("binary-name") if isinstance(record, dict) else None
    if not isinstance(name, str):
        raise FixturePreparationError(
            "cargo nextest binary inventory records must contain a "
            "string 'binary-name' field"
        )
    return name


def _binary_names(records: Mapping[str, object]) -> set[str]:
    """Collect names from validated records in Nextest's binary map."""
    return {_binary_record_name(record) for record in records.values()}


def _validate_binaries_metadata(text: str) -> None:
    """Require the pinned Nextest object shape and every fixture binary name."""
    document = _json_object(text, "cargo nextest binary inventory")
    _required_object_field(document, "rust-build-meta")
    binaries = _required_object_field(document, "rust-binaries")
    missing = EXPECTED_FIXTURE_BINARIES - _binary_names(binaries)
    if missing:
        raise FixturePreparationError(
            "cargo nextest binary inventory omitted fixture targets "
            f"{sorted(missing)}"
        )


def _validate_result_metadata(result: ProcessResult, validator: JsonCheck) -> None:
    """Attach command evidence when successful output is malformed."""
    try:
        validator(result.stdout)
    except FixturePreparationError as error:
        raise FixturePreparationError(
            f"{result.phase} emitted invalid metadata; "
            f"started={result.started_at.isoformat()}; "
            f"finished={result.finished_at.isoformat()}; "
            f"command={result.command!r}; elapsed={result.elapsed_seconds:.3f}s; "
            f"status={result.returncode}; stdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}; validation: {error}",
            record=result,
        ) from error


def _validate_saved_metadata(path: Path, phase: str, validator: JsonCheck) -> None:
    """Reject missing or malformed preparation input before a Nextest probe."""
    try:
        content = path.read_text(encoding="utf-8")
    except OSError as error:
        raise FixturePreparationError(
            f"{phase} metadata is unavailable at {path}: {error}"
        ) from error
    except UnicodeError as error:
        raise FixturePreparationError(
            f"{phase} metadata at {path} is not valid UTF-8: {error}"
        ) from error
    try:
        validator(content)
    except FixturePreparationError as error:
        raise FixturePreparationError(
            f"{phase} metadata at {path} is invalid: {error}"
        ) from error


def _remaining_budget(deadline: float, clock: Callable[[], float]) -> float:
    """Return the remaining preparation budget, failing closed at expiry."""
    remaining = deadline - clock()
    if remaining <= 0:
        raise FixturePreparationError(
            "fixture preparation exhausted its single 1200 second budget"
        )
    return remaining


def _require_success(result: ProcessResult) -> None:
    """Turn preparation command failures into a distinct, evidence-rich error."""
    if result.returncode != 0:
        raise FixturePreparationError(
            f"{result.phase} exited {result.returncode}; "
            f"started={result.started_at.isoformat()}; "
            f"finished={result.finished_at.isoformat()}; "
            f"command={result.command!r}; elapsed={result.elapsed_seconds:.3f}s; "
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}",
            record=result,
        )


def prepare_fixture(
    root: Path,
    crate: Path,
    target: Path,
    *,
    clock: Callable[[], float] | None = None,
) -> Fixture:
    """Acquire Cargo and Nextest metadata once within a shared 1200 s deadline."""
    cargo_metadata = (root / "cargo-metadata.json").resolve()
    binaries_metadata = (root / "binaries-metadata.json").resolve()
    if cargo_metadata.exists() or binaries_metadata.exists():
        raise FixturePreparationError(
            f"refusing stale fixture metadata under {root}"
        )

    now = clock or time.monotonic
    deadline = now() + FIXTURE_PREPARATION_TIMEOUT_SECONDS
    environment = _environment_for_target(target)
    context = ProcessContext(crate, environment)
    cargo_result = run_process(
        ("cargo", "metadata", "--format-version", "1"),
        phase="cargo metadata preparation",
        timeout_seconds=_remaining_budget(deadline, now),
        context=context,
    )
    _require_success(cargo_result)
    _validate_result_metadata(cargo_result, _validate_cargo_metadata)
    cargo_metadata.write_text(cargo_result.stdout, encoding="utf-8")

    binaries_result = run_process(
        (
            "cargo",
            "nextest",
            "list",
            "--cargo-metadata",
            str(cargo_metadata),
            "--list-type",
            "binaries-only",
            "--message-format",
            "json",
        ),
        phase="Nextest binary inventory preparation",
        timeout_seconds=_remaining_budget(deadline, now),
        context=context,
    )
    _require_success(binaries_result)
    _validate_result_metadata(binaries_result, _validate_binaries_metadata)
    binaries_metadata.write_text(binaries_result.stdout, encoding="utf-8")
    if now() > deadline:
        raise FixturePreparationError(
            "fixture metadata validation exceeded its single 1200 second budget"
        )

    return Fixture(
        crate=crate.resolve(),
        target=target.resolve(),
        cargo_metadata=cargo_metadata,
        binaries_metadata=binaries_metadata,
        preparation_records=(cargo_result, binaries_result),
    )


def run_nextest_probe(
    arguments: Sequence[str],
    fixture: Fixture,
    environment_overrides: Mapping[str, str] | None = None,
) -> ProcessResult:
    """Run a probe with complete saved inputs and its own 600 s watchdog."""
    _validate_saved_metadata(
        fixture.cargo_metadata, "cargo", _validate_cargo_metadata
    )
    _validate_saved_metadata(
        fixture.binaries_metadata,
        "Nextest binary inventory",
        _validate_binaries_metadata,
    )
    command = _nextest_probe_arguments(arguments, fixture)
    return run_process(
        command,
        phase=f"Nextest {command[2]} probe",
        timeout_seconds=SUBPROCESS_TIMEOUT_SECONDS,
        context=ProcessContext(
            fixture.crate,
            _environment_for_target(fixture.target, environment_overrides),
        ),
    )
