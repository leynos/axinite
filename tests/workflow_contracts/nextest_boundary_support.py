"""Prepare the shared Nextest fixture and scale its timeout probes."""

from __future__ import annotations

import json
import shutil
import typing as typ
from dataclasses import dataclass
from pathlib import Path
from uuid import uuid4

import pytest
from _workflow_policy import REPOSITORY_ROOT
from contract_sources import read_source
from nextest_fixture import Fixture, prepare_fixture, run_nextest_probe as _run
from nextest_process import ProcessResult
from timeout_budgets import NEXTEST_CONFIG

FIXTURE_CRATE = (
    REPOSITORY_ROOT
    / "tests"
    / "workflow_contracts"
    / "fixtures"
    / "nextest_boundary"
)

#: Test duration and scaled equivalents of schema's 300 s and trybuild's 900 s.
ALLOWANCE_SLEEP_SECONDS: typ.Final[int] = 5
SCALED_SCHEMA_HELPERS_UI_PERIOD: typ.Final[str] = "2s"
SCALED_NESTED_COMPILER_PERIOD: typ.Final[str] = "20s"


@dataclass(frozen=True, slots=True)
class TimeoutScalePolicy:
    """Identify one pinned per-binary period and its test-only scaled value."""

    selector: str
    expected_period: str
    scaled_period: str


SCHEMA_HELPER_UI_POLICY: typ.Final[TimeoutScalePolicy] = TimeoutScalePolicy(
    selector="binary(schema_helpers_ui)",
    expected_period="300s",
    scaled_period=SCALED_SCHEMA_HELPERS_UI_PERIOD,
)
NESTED_COMPILER_POLICY: typ.Final[TimeoutScalePolicy] = TimeoutScalePolicy(
    selector="binary(trybuild) | binary(schema_helpers_ui)",
    expected_period="900s",
    scaled_period=SCALED_NESTED_COMPILER_PERIOD,
)


def _require_the_toolchain() -> None:
    """Fail before fixture preparation when Cargo or Nextest is unavailable."""
    missing = [
        tool for tool in ("cargo", "cargo-nextest") if shutil.which(tool) is None
    ]
    assert not missing, (
        f"this contract runs nextest itself and needs {missing} on PATH; "
        "`make test` requires them too"
    )


def prepare_fixture_crate(tmp_path_factory: pytest.TempPathFactory) -> Fixture:
    """Copy the fixture and prepare its Cargo metadata exactly once per run.

    The shared conftest fixture owns this call. Keeping preparation in an
    ordinary function lets pytest register one session fixture for every
    consuming contract module.
    """
    _require_the_toolchain()
    root = tmp_path_factory.mktemp("nextest-boundary")
    crate = root / "crate"
    shutil.copytree(FIXTURE_CRATE, crate)
    target = (
        REPOSITORY_ROOT / "target" / "workflow-contracts" / "nextest-boundary"
    ).resolve()
    fixture = prepare_fixture(root, crate, target)
    receipt_directory = target / "preparation-receipts"
    receipt_directory.mkdir(parents=True, exist_ok=True)
    receipt = receipt_directory / f"{uuid4().hex}.jsonl"
    receipt.write_text(
        "".join(
            json.dumps(
                {
                    "copy": str(root.resolve()),
                    "phase": record.phase,
                    "command": record.command,
                    "elapsed_seconds": record.elapsed_seconds,
                    "status": record.returncode,
                },
                sort_keys=True,
            )
            + "\n"
            for record in fixture.preparation_records
        ),
        encoding="utf-8",
    )
    return fixture


def _scaled_per_binary_periods() -> str:
    """Return the real config with only its per-binary periods scaled down.

    The filters, override order and other fields remain unchanged. Nextest
    resolves the scaled configuration during the boundary tests.

    Raises
    ------
    AssertionError
        If a profile no longer has the pinned schema or trybuild override.
    SourceReadError
        If the configuration cannot be read as UTF-8.
    """
    configuration = read_source(NEXTEST_CONFIG)
    for profile in ("default", "ci"):
        configuration = _scale_override_period(
            configuration,
            profile,
            SCHEMA_HELPER_UI_POLICY,
        )
        configuration = _scale_override_period(
            configuration,
            profile,
            NESTED_COMPILER_POLICY,
        )
    return configuration


def _scale_override_period(
    configuration: str,
    profile: str,
    policy: TimeoutScalePolicy,
) -> str:
    """Scale one selected profile override after checking its pinned period."""
    marker = (
        f"[[profile.{profile}.overrides]]\nfilter = '{policy.selector}'\n"
    )
    assert configuration.count(marker) == 1, (
        f"[profile.{profile}] must declare one override for {policy.selector!r}"
    )
    start = configuration.index(marker)
    next_override = configuration.find("\n[[profile.", start + len(marker))
    next_profile = configuration.find("\n[profile.", start + len(marker))
    ends = [offset for offset in (next_override, next_profile) if offset >= 0]
    end = min(ends) if ends else len(configuration)
    block = configuration[start:end]
    expected = f'period = "{policy.expected_period}"'
    assert block.count(expected) == 1, (
        f"[profile.{profile}] override {policy.selector!r} must carry one "
        f"{expected}, got {block!r}"
    )
    return configuration[:start] + block.replace(
        expected, f'period = "{policy.scaled_period}"', 1
    ) + configuration[end:]


def _run_bounded_sleep(
    fixture: Fixture, configuration: Path, profile: str, binary: str
) -> ProcessResult:
    """Run one fixture binary's sleeping test with scaled timeouts."""
    return _run(
        (
            "cargo",
            "nextest",
            "run",
            "--config-file",
            str(configuration),
            "--profile",
            profile,
            "-E",
            f"binary({binary}) & test(bounded_sleep)",
        ),
        fixture,
        {"AXINITE_NEXTEST_SLEEP_SECONDS": str(ALLOWANCE_SLEEP_SECONDS)},
    )
