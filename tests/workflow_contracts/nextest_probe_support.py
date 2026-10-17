"""Set up controlled live probes of the pinned Nextest executable."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from nextest_boundary_support import _run
from nextest_fixture import Fixture, SUBPROCESS_TIMEOUT_SECONDS
from nextest_process import ProcessContext, ProcessResult, run_process
from timeout_budgets import NEXTEST_CONFIG


@dataclass(frozen=True, slots=True)
class CargoProxy:
    """Identify the reject-by-default Cargo executable and its call log."""

    executable: Path
    log: Path
    environment: dict[str, str]


@dataclass(frozen=True, slots=True)
class ProbeCase:
    """Describe a live probe, its expected invocation and result policy."""

    command: tuple[str, ...]
    expected_prefix: tuple[str, ...]
    rejects_metadata: bool
    selects_fixture_test: bool


def create_cargo_proxy(tmp_path: Path) -> CargoProxy:
    """Create a proxy that only allows Nextest to reach the real Cargo tool."""
    cargo = shutil.which("cargo")
    assert cargo is not None, "the contract suite requires Cargo"
    proxy_directory = tmp_path / "cargo-proxy"
    proxy_directory.mkdir()
    log_path = tmp_path / "cargo-proxy.jsonl"
    proxy = proxy_directory / "cargo"
    proxy.write_text(
        "\n".join(
            (
                f"#!{sys.executable}",
                "import json, os, sys",
                "arguments = sys.argv[1:]",
                "with open(",
                "    os.environ['AXINITE_CARGO_PROXY_LOG'],",
                "    'a', encoding='utf-8',",
                ") as log:",
                "    log.write(json.dumps(arguments) + '\\n')",
                "if arguments[:1] == ['nextest']:",
                f"    os.execv({cargo!r}, [{cargo!r}, *arguments])",
                "raise SystemExit(97)",
                "",
            )
        ),
        encoding="utf-8",
    )
    proxy.chmod(0o755)
    environment = {
        "PATH": os.pathsep.join((str(proxy_directory), os.environ["PATH"])),
        "CARGO": str(proxy),
        "AXINITE_CARGO_PROXY_LOG": str(log_path),
    }
    return CargoProxy(proxy, log_path, environment)


def assert_metadata_is_rejected(proxy: CargoProxy, crate: Path) -> None:
    """Prove the proxy blocks Cargo metadata instead of falling through."""
    rejected = subprocess.run(
        (str(proxy.executable), "metadata", "--format-version", "1"),
        cwd=crate,
        env={**os.environ, **proxy.environment},
        check=False,
        capture_output=True,
        text=True,
    )
    assert rejected.returncode == 97, (
        "the Cargo proxy must reject unexpected metadata invocations without "
        f"falling through: {rejected.stdout}\n{rejected.stderr}"
    )
    calls = _proxy_calls(proxy)
    assert calls == [["metadata", "--format-version", "1"]]
    proxy.log.unlink()


def select_probe_case(
    probe: str, fixture: Fixture, tmp_path: Path
) -> ProbeCase:
    """Choose real Nextest argv and metadata policy for one contract form."""
    config_args = ("--config-file", str(NEXTEST_CONFIG), "--profile", "ci")
    if probe == "show-config":
        command = (
            "cargo",
            "nextest",
            "show-config",
            "test-groups",
            *config_args,
            "--show-default",
            "--no-pager",
            "--color",
            "never",
        )
        return ProbeCase(
            command=command,
            expected_prefix=("nextest", "show-config", "test-groups"),
            rejects_metadata=False,
            selects_fixture_test=False,
        )
    if probe == "malformed-metadata":
        return _malformed_metadata_case(fixture, tmp_path)
    return _selection_probe_case(probe, config_args)


def _malformed_metadata_case(fixture: Fixture, tmp_path: Path) -> ProbeCase:
    """Use a damaged copy so live rejection cannot alter session metadata."""
    bad_metadata = tmp_path / "malformed-binaries-metadata.json"
    shutil.copyfile(fixture.binaries_metadata, bad_metadata)
    bad_metadata.write_text("{broken", encoding="utf-8")
    command = (
        "cargo",
        "nextest",
        "list",
        "--cargo-metadata",
        str(fixture.cargo_metadata),
        "--binaries-metadata",
        str(bad_metadata),
        "--color",
        "never",
    )
    return ProbeCase(
        command=command,
        expected_prefix=("nextest", "list"),
        rejects_metadata=True,
        selects_fixture_test=False,
    )


def _selection_probe_case(
    probe: str, config_args: tuple[str, ...]
) -> ProbeCase:
    """Select one real list or run form that executes the fixture contract."""
    assert probe in {"list", "run"}, f"unsupported live probe: {probe}"
    selection = "binary(schema_helpers_ui) & test(shared_preparation_contract_a)"
    command = (
        "cargo",
        "nextest",
        probe,
        *config_args,
        "-E",
        selection,
        *(("--status-level", "pass") if probe == "run" else ()),
        "--color",
        "never",
    )
    return ProbeCase(
        command=command,
        expected_prefix=("nextest", probe),
        rejects_metadata=False,
        selects_fixture_test=True,
    )


def execute_probe_case(
    case: ProbeCase, fixture: Fixture, proxy: CargoProxy
) -> ProcessResult:
    """Execute a positive reuse probe or an isolated malformed-input check."""
    if not case.rejects_metadata:
        return _run(case.command, fixture, proxy.environment)
    environment = {
        **os.environ,
        **proxy.environment,
        "CARGO_TARGET_DIR": str(fixture.target),
        "CARGO_BUILD_BUILD_DIR": str(fixture.target / "build"),
    }
    return run_process(
        case.command,
        phase="Nextest malformed metadata rejection",
        timeout_seconds=SUBPROCESS_TIMEOUT_SECONDS,
        context=ProcessContext(fixture.crate, environment),
    )


def assert_probe_result(case: ProbeCase, result: ProcessResult) -> None:
    """Check command status, malformed-input diagnostics and selection output."""
    if case.rejects_metadata:
        assert result.returncode != 0, (
            "Nextest accepted malformed copied binaries metadata:\n"
            f"{result.stdout}\n{result.stderr}"
        )
        assert "metadata" in (result.stdout + result.stderr).lower(), (
            "Nextest's rejection did not identify its malformed metadata input:\n"
            f"{result.stdout}\n{result.stderr}"
        )
    else:
        assert result.returncode == 0, (
            "Nextest failed while reusing its preparation metadata:\n"
            f"{result.stdout}\n{result.stderr}"
        )
    if case.selects_fixture_test:
        selected_output = result.stdout + result.stderr
        assert "shared_preparation_contract_a" in selected_output, (
            "the live probe did not select the expected schema helper test:\n"
            f"{selected_output}"
        )


def assert_single_probe_invocation(proxy: CargoProxy, case: ProbeCase) -> None:
    """Require one Nextest call carrying both saved preparation inputs."""
    calls = _proxy_calls(proxy)
    assert len(calls) == 1 and calls[0][: len(case.expected_prefix)] == list(
        case.expected_prefix
    ), (
        "a live Nextest probe spawned Cargo for another metadata or build step: "
        f"{calls}"
    )
    assert "--cargo-metadata" in calls[0]
    assert "--binaries-metadata" in calls[0]


def _proxy_calls(proxy: CargoProxy) -> list[list[str]]:
    """Decode the proxy's append-only JSONL command record."""
    return [
        json.loads(line)
        for line in proxy.log.read_text(encoding="utf-8").splitlines()
    ]
