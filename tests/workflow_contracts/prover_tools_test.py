"""Contracts for local Kani and Verus Make targets.

A JSON-lines fake records arguments and child settings without installing either
prover or inspecting Makefile implementation details. Run via
``make test-workflow-contracts``.
"""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

import pytest
from _makefile_test_support import _MakeTestContext, shell_quote

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
HARNESS = "verify_exact_projection_four_entries"
RECORDED_ENVIRONMENT = (
    "CARGO_INCREMENTAL",
    "CARGO_ENCODED_RUSTFLAGS",
    "KANI_HOME",
    "KANI_VERSION_FILE",
    "KANI_INSTALLATION",
    "KANI_PROOF_DIR",
    "VERUS_PROOF_FILE",
    "VERUS_FLAGS",
    "Z3_INSTALL_DIR",
    "VERUS_Z3_PATH",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "RUSTDOC",
    "RUSTFLAGS",
    "KANI_RUSTFLAGS",
)


def _fake_prover(make_context: _MakeTestContext) -> tuple[Path, Path]:
    """Create an executable that records prover arguments and child settings."""
    directory = make_context.tmp_path / "fake prover"
    directory.mkdir()
    executable = directory / "recording prover"
    log = make_context.tmp_path / "prover calls.jsonl"
    environment_keys = repr(RECORDED_ENVIRONMENT)
    executable.write_text(
        "#!/usr/bin/env python3\n"
        "import json\n"
        "import os\n"
        "import sys\n"
        f"environment_keys = {environment_keys}\n"
        "record = {\n"
        "    'argv': sys.argv[1:],\n"
        "    'env': {key: os.environ.get(key) for key in environment_keys},\n"
        "}\n"
        "with open(os.environ['PROVER_CALL_LOG'], 'a', encoding='utf-8') as calls:\n"
        "    calls.write(json.dumps(record, sort_keys=True) + '\\n')\n"
        "failure_prefix = os.environ.get('PROVER_FAKE_FAIL_ON', '').split()\n"
        "if failure_prefix and sys.argv[1:1 + len(failure_prefix)] == failure_prefix:\n"
        "    raise SystemExit(int(os.environ.get('PROVER_FAKE_EXIT', '23')))\n",
        encoding="utf-8",
    )
    executable.chmod(0o755)
    return executable, log


def _run_make(
    make_context: _MakeTestContext,
    executable: Path,
    log: Path,
    target: str,
    **overrides: str,
) -> subprocess.CompletedProcess[str]:
    """Run one real Make target with the recording executable and isolated env."""
    environment = os.environ.copy()
    environment.update(
        {
            "PATH": os.pathsep.join(
                (str(make_context.utility_bin), os.environ.get("PATH", ""))
            ),
            "HOME": str(make_context.tmp_path / "home"),
            "PROVER_TOOLS": shell_quote(str(executable)),
            "PROVER_CALL_LOG": str(log),
        }
    )
    environment.update(overrides)
    return subprocess.run(
        [make_context.make_executable, "--no-print-directory", target],
        cwd=REPOSITORY_ROOT,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )


def _calls(log: Path) -> list[dict[str, object]]:
    """Decode the fake prover's one-record-per-invocation log."""
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text(encoding="utf-8").splitlines()]


def _kani_inputs(tmp_path: Path) -> dict[str, str]:
    """Create valid, custom Kani paths with spaces for command-boundary tests."""
    root = tmp_path / "kani inputs"
    version_file = root / "custom pin" / "VERSION"
    version_file.parent.mkdir(parents=True)
    version_file.write_text("0.67.0\n", encoding="utf-8")
    installation = root / "selected release"
    installation.mkdir(parents=True)
    proof_dir = root / "proof package"
    (proof_dir / "src").mkdir(parents=True)
    (proof_dir / "Cargo.toml").write_text(
        '[package]\nname = "contract-fixture"\nversion = "0.1.0"\n'
        'edition = "2021"\n[workspace]\n',
        encoding="utf-8",
    )
    (proof_dir / "src" / "lib.rs").write_text("pub fn fixture() {}\n", encoding="utf-8")
    return {
        "KANI_VERSION_FILE": str(version_file),
        "KANI_INSTALLATION": str(installation),
        "KANI_PROOF_DIR": str(proof_dir),
    }


@pytest.mark.parametrize(
    ("target", "subcommand"),
    [
        ("install-kani", "install"),
        ("kani-check", "check-version"),
    ],
)
def test_kani_installation_and_check_honour_custom_version_file(
    make_context: _MakeTestContext,
    target: str,
    subcommand: str,
) -> None:
    """Pass the caller's custom Kani version file to both CLI boundaries."""
    executable, log = _fake_prover(make_context)
    inputs = _kani_inputs(make_context.tmp_path)

    result = _run_make(
        make_context,
        executable,
        log,
        target,
        CARGO_INCREMENTAL="1",
        **inputs,
    )

    assert result.returncode == 0, result.stderr
    calls = _calls(log)
    arguments = calls[0]["argv"]
    assert calls[0]["env"]["CARGO_INCREMENTAL"] == "0"
    assert arguments[:2] == ["kani", subcommand]
    if target == "install-kani":
        assert "--no-compile" in arguments
    version_options = [
        index
        for index, argument in enumerate(arguments)
        if argument == "--version-file"
    ]
    assert len(version_options) == 1
    assert arguments[version_options[0] + 1] == inputs["KANI_VERSION_FILE"]


@pytest.mark.parametrize("target", ("kani", "kani-full"))
def test_kani_targets_check_the_pin_before_exec_with_literal_paths(
    make_context: _MakeTestContext,
    target: str,
) -> None:
    """Check Kani execution order, selected harness, and preserved child env."""
    executable, log = _fake_prover(make_context)
    inputs = _kani_inputs(make_context.tmp_path)
    wrappers = {
        "RUSTC_WRAPPER": "/managed/rustc wrapper",
        "RUSTC_WORKSPACE_WRAPPER": "/managed/workspace wrapper",
        "RUSTDOC": "/managed/rustdoc wrapper",
        "RUSTFLAGS": "-Zthreads=8 -Clink-arg=-fuse-ld=mold",
        "KANI_RUSTFLAGS": "--cfg=prover_contract",
    }

    result = _run_make(
        make_context,
        executable,
        log,
        target,
        CARGO_INCREMENTAL="1",
        **inputs,
        **wrappers,
    )

    assert result.returncode == 0, result.stderr
    calls = _calls(log)
    assert [call["argv"][:2] for call in calls] == [
        ["kani", "check-version"],
        ["kani", "exec"],
    ]
    check_arguments = calls[0]["argv"]
    assert check_arguments[:2] == ["kani", "check-version"]
    version_options = [
        index
        for index, argument in enumerate(check_arguments)
        if argument == "--version-file"
    ]
    assert len(version_options) == 1
    assert check_arguments[version_options[0] + 1] == inputs["KANI_VERSION_FILE"]
    exec_arguments = calls[1]["argv"]
    assert exec_arguments[:2] == ["kani", "exec"]
    for option, expected in (
        ("--installation", inputs["KANI_INSTALLATION"]),
        ("--cwd", inputs["KANI_PROOF_DIR"]),
    ):
        option_indexes = [
            index for index, argument in enumerate(exec_arguments) if argument == option
        ]
        assert len(option_indexes) == 1
        assert exec_arguments[option_indexes[0] + 1] == expected
    extra_arguments = [
        argument for argument in exec_arguments if argument.startswith("--extra-arg=")
    ]
    if target == "kani":
        assert f"--extra-arg=--harness={HARNESS}" in extra_arguments
        assert not any("--output-format=regular" in arg for arg in extra_arguments)
    else:
        assert extra_arguments == ["--extra-arg=--output-format=regular"]
        assert not any("--harness" in arg for arg in extra_arguments)
    child_environment = calls[1]["env"]
    assert child_environment["CARGO_INCREMENTAL"] == "0"
    for name, expected in {**inputs, **wrappers}.items():
        assert child_environment[name] == expected


def _verus_inputs(tmp_path: Path) -> tuple[dict[str, str], Path, Path]:
    """Create a default proof, a custom Z3 cache, and its expected binary path."""
    root = tmp_path / "verus inputs"
    default_proof = root / "default proof.rs"
    default_proof.parent.mkdir(parents=True)
    default_proof.write_text("fn main() {}\n", encoding="utf-8")
    z3_install_dir = root / "z3 cache"
    z3_path = z3_install_dir / "z3"
    z3_path.parent.mkdir(parents=True)
    z3_path.write_text("fake solver\n", encoding="utf-8")
    return (
        {
            "VERUS_PROOF_FILE": str(default_proof),
            "Z3_INSTALL_DIR": str(z3_install_dir),
        },
        default_proof,
        z3_install_dir,
    )


def test_verus_uses_one_configured_proof_file_and_absolute_z3_path(
    make_context: _MakeTestContext,
) -> None:
    """Use exactly the configured entry point and an absolute cached solver."""
    executable, log = _fake_prover(make_context)
    inputs, proof_file, z3_install_dir = _verus_inputs(make_context.tmp_path)

    result = _run_make(
        make_context,
        executable,
        log,
        "verus",
        CARGO_INCREMENTAL="1",
        **inputs,
    )

    assert result.returncode == 0, result.stderr
    calls = _calls(log)
    assert len(calls) == 1
    assert calls[0]["env"]["CARGO_INCREMENTAL"] == "0"
    arguments = calls[0]["argv"]
    proof_options = [
        index for index, argument in enumerate(arguments) if argument == "--proof-file"
    ]
    assert len(proof_options) == 1
    assert arguments[proof_options[0] + 1] == str(proof_file)
    z3_path = Path(calls[0]["env"]["VERUS_Z3_PATH"])
    assert z3_path == (z3_install_dir / "z3").resolve()


def test_install_z3_honours_custom_directory_and_propagates_failure(
    make_context: _MakeTestContext,
) -> None:
    """Pass the Z3 install directory and preserve the installer's failure."""
    executable, log = _fake_prover(make_context)
    z3_install_dir = make_context.tmp_path / "custom z3 install"

    result = _run_make(
        make_context,
        executable,
        log,
        "install-z3",
        Z3_INSTALL_DIR=str(z3_install_dir),
        PROVER_FAKE_FAIL_ON="z3 install",
        PROVER_FAKE_EXIT="23",
    )

    assert result.returncode != 0
    calls = _calls(log)
    assert len(calls) == 1
    arguments = calls[0]["argv"]
    assert arguments[:2] == ["z3", "install"]
    install_options = [
        index for index, argument in enumerate(arguments) if argument == "--install-dir"
    ]
    assert len(install_options) == 1
    assert arguments[install_options[0] + 1] == str(z3_install_dir)


def test_install_verus_propagates_installer_failure(
    make_context: _MakeTestContext,
) -> None:
    """Return a failed package Verus installation as a failed Make target."""
    executable, log = _fake_prover(make_context)

    result = _run_make(
        make_context,
        executable,
        log,
        "install-verus",
        PROVER_FAKE_FAIL_ON="verus install",
        PROVER_FAKE_EXIT="23",
    )

    assert result.returncode != 0
    calls = _calls(log)
    assert len(calls) == 1
    assert calls[0]["argv"][:2] == ["verus", "install"]


def test_repeated_quoted_verus_proof_files_replace_default_in_order(
    make_context: _MakeTestContext,
) -> None:
    """Keep repeated proof paths intact and let the explicit list replace default."""
    executable, log = _fake_prover(make_context)
    inputs, default_proof, _ = _verus_inputs(make_context.tmp_path)
    proof_files = [
        make_context.tmp_path / "first proof with spaces.rs",
        make_context.tmp_path / "second proof with spaces.rs",
    ]
    for proof_file in proof_files:
        proof_file.write_text("fn main() {}\n", encoding="utf-8")
    inputs["VERUS_FLAGS"] = " ".join(
        f"--proof-file {shell_quote(str(proof_file))}" for proof_file in proof_files
    )

    result = _run_make(make_context, executable, log, "verus", **inputs)

    assert result.returncode == 0, result.stderr
    arguments = _calls(log)[0]["argv"]
    proof_options = [
        index for index, argument in enumerate(arguments) if argument == "--proof-file"
    ]
    assert [arguments[index + 1] for index in proof_options] == [
        str(proof_file) for proof_file in proof_files
    ]
    assert str(default_proof) not in arguments


def test_malformed_verus_flags_fail_before_the_prover_is_invoked(
    make_context: _MakeTestContext,
) -> None:
    """Reject an unterminated quoted argument without starting verification."""
    executable, log = _fake_prover(make_context)
    inputs, _, _ = _verus_inputs(make_context.tmp_path)
    inputs["VERUS_FLAGS"] = "--proof-file 'unterminated path"

    result = _run_make(make_context, executable, log, "verus", **inputs)

    assert result.returncode != 0
    assert _calls(log) == []


def test_failed_kani_proof_stops_formal_pr_before_verus(
    make_context: _MakeTestContext,
) -> None:
    """Propagate a proof failure and do not launch the later Verus target."""
    executable, log = _fake_prover(make_context)
    inputs = _kani_inputs(make_context.tmp_path)
    verus_inputs, _, _ = _verus_inputs(make_context.tmp_path)
    result = _run_make(
        make_context,
        executable,
        log,
        "formal-pr",
        **inputs,
        **verus_inputs,
        PROVER_FAKE_FAIL_ON="kani exec",
        PROVER_FAKE_EXIT="23",
    )

    assert result.returncode != 0
    calls = _calls(log)
    assert [call["argv"][:2] for call in calls] == [
        ["kani", "check-version"],
        ["kani", "exec"],
    ]


def test_verus_failure_diagnostics_do_not_echo_secret_flags(
    make_context: _MakeTestContext,
) -> None:
    """Avoid echoing arbitrary secret-bearing options when verification fails."""
    executable, log = _fake_prover(make_context)
    inputs, _, _ = _verus_inputs(make_context.tmp_path)
    inputs["VERUS_FLAGS"] = "--api-token 'contract secret value'"

    result = _run_make(
        make_context,
        executable,
        log,
        "verus",
        **inputs,
        PROVER_FAKE_FAIL_ON="verus run",
        PROVER_FAKE_EXIT="23",
    )

    assert result.returncode != 0
    assert "contract secret value" not in result.stdout
    assert "contract secret value" not in result.stderr
