"""Direct contracts for the prover CLI's validation and failure boundaries.

The Make-level contracts cover argument forwarding. These tests invoke
``scripts/prover_cli.py`` itself, so a rejected setting, command, or action is
proved to fail before any prover process starts. Run via
``make test-workflow-contracts``.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest
from _makefile_test_support import shell_quote

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
PROVER_CLI = REPOSITORY_ROOT / "scripts" / "prover_cli.py"
CALL_LOG_NAME = "prover calls.jsonl"


def _fake_prover(tmp_path: Path, *, terminate: bool = False) -> Path:
    """Create an executable that records arguments and may die by signal."""
    executable = tmp_path / "fake prover"
    termination = (
        "import signal\nsignal.raise_signal(signal.SIGTERM)\n" if terminate else ""
    )
    executable.write_text(
        f"#!{sys.executable}\n"
        "import json\n"
        "import os\n"
        "import sys\n"
        "with open(os.environ['PROVER_CALL_LOG'], 'a', encoding='utf-8') as log:\n"
        "    log.write(json.dumps(sys.argv[1:]) + '\\n')\n"
        f"{termination}",
        encoding="utf-8",
    )
    executable.chmod(0o755)
    return executable


def _run_cli(
    tmp_path: Path,
    action: str,
    *,
    environment: dict[str, str],
) -> subprocess.CompletedProcess[str]:
    """Run one prover action in an environment containing only what it needs."""
    return subprocess.run(
        [sys.executable, str(PROVER_CLI), action],
        cwd=REPOSITORY_ROOT,
        env={
            "PATH": os.environ.get("PATH", ""),
            "PROVER_CALL_LOG": str(tmp_path / CALL_LOG_NAME),
            **environment,
        },
        check=False,
        capture_output=True,
        text=True,
    )


def _calls(tmp_path: Path) -> list[list[str]]:
    """Decode the fake prover's recorded argument lists."""
    log = tmp_path / CALL_LOG_NAME
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]


@pytest.mark.parametrize(
    ("action", "message"),
    [
        ("kani-check", "KANI_BIN_DIR must be configured"),
        ("verus", "VERUS_PROOF_FILE must be configured"),
    ],
)
def test_missing_setting_fails_before_the_prover_starts(
    tmp_path: Path,
    action: str,
    message: str,
) -> None:
    """Reject a missing required setting without executing the prover."""
    prover = _fake_prover(tmp_path)

    result = _run_cli(
        tmp_path,
        action,
        environment={"PROVER_TOOLS": shell_quote(str(prover))},
    )

    assert result.returncode == 1
    assert message in result.stderr
    assert _calls(tmp_path) == []


@pytest.mark.parametrize(
    ("prover_tools", "message"),
    [
        ("", "PROVER_TOOLS must be configured"),
        ("   ", "PROVER_TOOLS must contain a command"),
        ("'unterminated", "invalid PROVER_TOOLS: unmatched quoting"),
    ],
)
def test_invalid_prover_tools_is_rejected(
    tmp_path: Path,
    prover_tools: str,
    message: str,
) -> None:
    """Reject empty, blank, and malformed prover commands."""
    result = _run_cli(
        tmp_path,
        "install-z3",
        environment={"PROVER_TOOLS": prover_tools, "Z3_INSTALL_DIR": "/unused"},
    )

    assert result.returncode == 1
    assert message in result.stderr
    assert _calls(tmp_path) == []


def test_unsupported_kani_routing_action_does_not_run_the_prover(
    tmp_path: Path,
) -> None:
    """Reject an unknown Kani routing action without executing the prover."""
    prover = _fake_prover(tmp_path)

    result = _run_cli(
        tmp_path,
        "kani-unsupported",
        environment={"PROVER_TOOLS": shell_quote(str(prover))},
    )

    assert result.returncode == 1
    assert "unknown prover action" in result.stderr
    assert _calls(tmp_path) == []


def test_signalled_child_reports_a_shell_style_status(tmp_path: Path) -> None:
    """Report a child killed by SIGTERM as 128 plus the signal number."""
    prover = _fake_prover(tmp_path, terminate=True)

    result = _run_cli(
        tmp_path,
        "install-z3",
        environment={
            "PROVER_TOOLS": shell_quote(str(prover)),
            "Z3_INSTALL_DIR": "/unused",
        },
    )

    assert result.returncode == 143


def test_install_verus_forwards_custom_version_and_checksum_files(
    tmp_path: Path,
) -> None:
    """Pass custom Verus pin and checksum files as literal arguments."""
    prover = _fake_prover(tmp_path)

    result = _run_cli(
        tmp_path,
        "install-verus",
        environment={
            "PROVER_TOOLS": shell_quote(str(prover)),
            "VERUS_VERSION_FILE": "/custom/verus version",
            "VERUS_CHECKSUM_FILE": "/custom/verus checksums",
        },
    )

    assert result.returncode == 0
    assert _calls(tmp_path) == [
        [
            "verus",
            "install",
            "--version-file",
            "/custom/verus version",
            "--checksum-file",
            "/custom/verus checksums",
        ]
    ]
