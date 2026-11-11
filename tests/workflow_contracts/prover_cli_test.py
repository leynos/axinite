"""Direct contracts for the prover CLI's validation and failure boundaries.

The Make-level contracts cover argument forwarding. These tests invoke
``scripts/prover_cli.py`` itself, so a rejected setting, command, or action is
proved to fail before any prover process starts. Run via
``make test-workflow-contracts``.
"""

from __future__ import annotations

import importlib.util
import json
import os
import shlex
import subprocess
import sys
from collections.abc import Mapping
from pathlib import Path
from types import ModuleType

import pytest
from hypothesis import given
from hypothesis import strategies as st

from _makefile_test_support import shell_quote

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
PROVER_CLI = REPOSITORY_ROOT / "scripts" / "prover_cli.py"
CALL_LOG_NAME = "prover calls.jsonl"


def _load_cli() -> ModuleType:
    """Load the prover CLI as a module for direct query-boundary tests."""
    spec = importlib.util.spec_from_file_location("prover_cli_under_test", PROVER_CLI)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    # Dataclass creation looks the module up by name, so register it first.
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _fake_prover(
    tmp_path: Path,
    *,
    terminate: bool = False,
    exit_status: int | None = None,
) -> Path:
    """Create an executable that records arguments and may fail deliberately."""
    executable = tmp_path / "fake prover"
    termination = (
        "import signal\nsignal.raise_signal(signal.SIGTERM)\n" if terminate else ""
    )
    if exit_status is not None:
        termination += f"sys.exit({exit_status})\n"
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


def test_normal_non_zero_exit_status_is_preserved(tmp_path: Path) -> None:
    """Return a child's exact non-zero status instead of a fixed failure."""
    prover = _fake_prover(tmp_path, exit_status=23)

    result = _run_cli(
        tmp_path,
        "install-z3",
        environment={
            "PROVER_TOOLS": shell_quote(str(prover)),
            "Z3_INSTALL_DIR": "/unused",
        },
    )

    assert result.returncode == 23
    assert len(_calls(tmp_path)) == 1


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


class _DependencyRecorder:
    """Record which injected dependency an argument query asked for."""

    def __init__(self) -> None:
        """Start the recorder with no recorded queries."""
        self.names: list[str] = []

    def setting(self, name: str) -> str:
        """Record and resolve one required-setting query."""
        self.names.append(name)
        return f"/resolved/{name.lower()}"

    def proof_arguments(self) -> list[str]:
        """Record and resolve the Verus proof argument query."""
        self.names.append("proof_arguments")
        return ["--proof-file", "/resolved/proof.rs"]

    def kani_installation(self) -> str:
        """Record and resolve the Kani installation query."""
        self.names.append("kani_installation")
        return "/resolved/kani release"


def _sources(cli: ModuleType, recorder: _DependencyRecorder) -> object:
    """Bundle the recorder's resolvers as the builder's injected sources."""
    return cli._ArgumentSources(
        setting=recorder.setting,
        proof_arguments=recorder.proof_arguments,
        kani_installation=recorder.kani_installation,
        version_file="/custom/pin",
    )


def test_argument_queries_resolve_only_the_dependencies_they_need() -> None:
    """Query arguments without ambient reads or eager fallible resolution."""
    cli = _load_cli()

    recorder = _DependencyRecorder()
    assert cli._arguments("kani-check", _sources(cli, recorder)) == [
        "kani",
        "check-version",
        "--version-file",
        "/custom/pin",
        "--kani-command",
        "/resolved/kani_bin_dir/cargo-kani",
    ]
    assert recorder.names == ["KANI_BIN_DIR"]

    recorder = _DependencyRecorder()
    assert cli._arguments("kani-full", _sources(cli, recorder)) == [
        "kani",
        "exec",
        "--installation",
        "/resolved/kani release",
        "--cwd",
        "/resolved/kani_proof_dir",
        "--extra-arg=--output-format=regular",
    ]
    assert recorder.names == ["kani_installation", "KANI_PROOF_DIR"]

    recorder = _DependencyRecorder()
    assert cli._arguments("verus", _sources(cli, recorder)) == [
        "verus",
        "run",
        "--proof-file",
        "/resolved/proof.rs",
    ]
    assert recorder.names == ["proof_arguments"]


def test_verus_proof_query_inserts_the_default_proof_file() -> None:
    """Prepend the configured proof file when the flags select none."""
    cli = _load_cli()

    assert cli._proof_arguments({"VERUS_PROOF_FILE": "/proofs/main.rs"}) == [
        "--proof-file",
        "/proofs/main.rs",
    ]
    assert cli._proof_arguments(
        {
            "VERUS_FLAGS": "--verify-root /proofs/root.rs",
            "VERUS_PROOF_FILE": "/proofs/main.rs",
        }
    ) == ["--proof-file", "/proofs/main.rs", "--verify-root", "/proofs/root.rs"]


def test_verus_proof_query_passes_a_padded_path_through_unchanged() -> None:
    """Keep a configured path's own whitespace instead of trimming it."""
    cli = _load_cli()

    assert cli._proof_arguments({"VERUS_PROOF_FILE": " /proofs/main.rs "}) == [
        "--proof-file",
        " /proofs/main.rs ",
    ]


@given(
    flags=st.lists(st.text(min_size=1), max_size=6),
    default=st.text(min_size=1).filter(lambda value: value.strip()),
)
def test_verus_proof_query_round_trips_quoted_flags(
    flags: list[str],
    default: str,
) -> None:
    """Preserve every literal argument and its order through quoting."""
    cli = _load_cli()
    rendered = " ".join(shlex.quote(flag) for flag in flags)

    arguments = cli._proof_arguments(
        {"VERUS_FLAGS": rendered, "VERUS_PROOF_FILE": default}
    )

    selects_proof = any(
        flag == "--proof-file" or flag.startswith("--proof-file=") for flag in flags
    )
    if selects_proof:
        assert arguments == flags
    else:
        assert arguments == ["--proof-file", default, *flags]


def test_verus_proof_query_keeps_an_explicit_proof_selection() -> None:
    """Do not add the default proof file when the flags select one."""
    cli = _load_cli()

    assert cli._proof_arguments(
        {"VERUS_FLAGS": "--proof-file=/proofs/other.rs", "VERUS_PROOF_FILE": "/unused"}
    ) == ["--proof-file=/proofs/other.rs"]
    assert cli._proof_arguments(
        {"VERUS_FLAGS": "--proof-file /proofs/other.rs"}
    ) == ["--proof-file", "/proofs/other.rs"]


@pytest.mark.parametrize(
    ("settings", "message"),
    [
        ({"VERUS_FLAGS": "'unterminated"}, "invalid VERUS_FLAGS: unmatched quoting"),
        ({}, "VERUS_PROOF_FILE must be configured"),
        ({"VERUS_FLAGS": "  "}, "VERUS_PROOF_FILE must be configured"),
        ({"VERUS_PROOF_FILE": " \t"}, "VERUS_PROOF_FILE must be configured"),
    ],
)
def test_verus_proof_query_reports_its_failure_boundary(
    settings: Mapping[str, str],
    message: str,
) -> None:
    """Raise the documented errors for unparsable flags and a missing proof."""
    cli = _load_cli()

    with pytest.raises(ValueError, match=message):
        cli._proof_arguments(settings)


def test_argument_queries_propagate_a_failed_kani_pin_read() -> None:
    """Keep the fallible installation boundary visible to the caller."""
    cli = _load_cli()

    def unusable_pin() -> str:
        """Raise the error a pin file without a version produces."""
        raise ValueError("Kani version file /pin is empty")

    sources = cli._ArgumentSources(
        setting=lambda name: f"/resolved/{name.lower()}",
        proof_arguments=list,
        kani_installation=unusable_pin,
        version_file="/pin",
    )

    with pytest.raises(ValueError, match="is empty"):
        cli._arguments("kani", sources)


def test_argument_queries_propagate_malformed_verus_flags() -> None:
    """Keep the Verus flag parsing failure visible through the dispatch."""
    cli = _load_cli()

    sources = cli._ArgumentSources(
        setting=lambda name: f"/resolved/{name.lower()}",
        proof_arguments=lambda: cli._proof_arguments(
            {"VERUS_FLAGS": "'unterminated"}
        ),
        kani_installation=lambda: "/resolved/kani release",
        version_file="/pin",
    )

    with pytest.raises(ValueError, match="invalid VERUS_FLAGS: unmatched quoting"):
        cli._arguments("verus", sources)


@pytest.mark.parametrize("pin_contents", ["", " \n\t"])
def test_empty_kani_pin_fails_before_the_proof_starts(
    tmp_path: Path,
    pin_contents: str,
) -> None:
    """Reject an empty pin at the default installation boundary."""
    prover = _fake_prover(tmp_path)
    pin = tmp_path / "VERSION"
    pin.write_text(pin_contents, encoding="utf-8")

    result = _run_cli(
        tmp_path,
        "kani",
        environment={
            "PROVER_TOOLS": shell_quote(str(prover)),
            "KANI_VERSION_FILE": str(pin),
            "KANI_HOME": str(tmp_path / "kani home"),
            "KANI_PROOF_DIR": "/unused",
        },
    )

    assert result.returncode == 1
    assert "is empty" in result.stderr
    assert _calls(tmp_path) == []
