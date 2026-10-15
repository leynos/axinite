"""Shared fixture and subprocess helpers for nextest boundary contracts."""

import os
import shutil
import subprocess
import typing as typ
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path

import pytest
from _workflow_policy import REPOSITORY_ROOT
from contract_sources import read_source
from timeout_budgets import NEXTEST_CONFIG

FIXTURE_CRATE = (
    REPOSITORY_ROOT
    / "tests"
    / "workflow_contracts"
    / "fixtures"
    / "nextest_boundary"
)
SUBPROCESS_TIMEOUT_SECONDS = 600.0

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


class Fixture(typ.NamedTuple):
    """Where a copied nextest fixture crate lives and where it builds."""

    crate: Path
    target: Path


def _tool_is_present(command: str) -> bool:
    """Return whether a command resolves on `PATH`."""
    return shutil.which(command) is not None


def _run(
    arguments: Sequence[str],
    crate: Path,
    target: Path,
    environment_overrides: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run a Cargo command in the copied crate and capture its output.

    The fixture source is copied outside the repository because Cargo finds
    `.cargo/config.toml` in parent directories. Its build artefacts stay in
    the repository's ignored `target` directory and use the shared Cargo
    package cache.
    """
    environment = dict(os.environ)
    environment["CARGO_TARGET_DIR"] = str(target)
    environment["CARGO_BUILD_BUILD_DIR"] = str(target / "build")
    environment.update(environment_overrides or {})
    return subprocess.run(  # noqa: S603 - fixed argument vector, no shell
        list(arguments),
        cwd=crate,
        env=environment,
        capture_output=True,
        text=True,
        timeout=SUBPROCESS_TIMEOUT_SECONDS,
        check=False,
    )


@pytest.fixture(scope="module")
def fixture_crate(tmp_path_factory: pytest.TempPathFactory) -> Fixture:
    """Return the fixture crate copied outside the repository tree."""
    root = tmp_path_factory.mktemp("nextest-boundary")
    crate = root / "crate"
    shutil.copytree(FIXTURE_CRATE, crate)
    target = REPOSITORY_ROOT / "target" / "workflow-contracts" / "nextest-boundary"
    return Fixture(crate=crate, target=target)


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
    fixture_crate: Fixture, configuration: Path, profile: str, binary: str
) -> subprocess.CompletedProcess[str]:
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
        fixture_crate.crate,
        fixture_crate.target,
        {"AXINITE_NEXTEST_SLEEP_SECONDS": str(ALLOWANCE_SLEEP_SECONDS)},
    )


@pytest.fixture(scope="module", autouse=True)
def _require_the_toolchain() -> None:
    """Fail rather than skip when the required Cargo tools are missing."""
    missing = [
        tool for tool in ("cargo", "cargo-nextest") if not _tool_is_present(tool)
    ]
    assert not missing, (
        f"this contract runs nextest itself and needs {missing} on PATH; "
        "`make test` requires them too"
    )
