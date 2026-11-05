"""Real Verus checks for set intersection and CLI sequencing behaviour."""

from __future__ import annotations

import os
import shlex
import shutil
from pathlib import Path

from prover_test_support import (
    REPOSITORY_ROOT,
    _run_cli_with_closed_readers,
    _run_make,
)

VERUS_PROOF = REPOSITORY_ROOT / "verus" / "tooling_smoke" / "main.rs"


def _verus_cli_command(*proof_files: Path) -> list[str]:
    """Build the pinned CLI command with each proof path as a separate option."""
    prover_tools = shlex.split(os.environ["PROVER_TOOLS"])
    return [
        *prover_tools,
        "verus",
        "run",
        *[
            argument
            for proof_file in proof_files
            for argument in ("--proof-file", str(proof_file))
        ],
    ]


def _z3_environment() -> dict[str, str]:
    """Select the same absolute Z3 binary used by the Make adapter."""
    z3_install_dir = Path(os.environ["Z3_INSTALL_DIR"])
    return {"VERUS_Z3_PATH": str((z3_install_dir / "z3").resolve())}


def _verus_flags(*proof_files: Path) -> str:
    """Encode repeated proof paths without splitting paths that contain spaces."""
    return " ".join(
        f"--proof-file {shlex.quote(str(proof_file))}" for proof_file in proof_files
    )


def _write_proof(path: Path, name: str, postcondition: str) -> Path:
    """Write a standalone Verus proof entry point for CLI ordering checks."""
    path.write_text(
        "use vstd::prelude::*;\n\n"
        "verus! {\n"
        f"proof fn {name}(value: int) ensures {postcondition} {{ }}\n"
        "fn main() {}\n"
        "}\n",
        encoding="utf-8",
    )
    return path


def _proof_directory(tmp_path: Path) -> Path:
    """Keep path arguments space-containing without invalid crate-name stems."""
    directory = tmp_path / "proof files with spaces"
    directory.mkdir()
    return directory


def test_verus_proves_unbounded_set_intersection_membership() -> None:
    """Require both subset obligations and concrete witnesses to verify."""
    proof_source = VERUS_PROOF.read_text(encoding="utf-8")
    assert "proof fn intersection_membership_iff_both" in proof_source
    assert "proof fn intersection_is_subset_of_both_inputs" in proof_source
    assert "proof fn empty_and_nonempty_intersection_witnesses" in proof_source

    result = _run_make("verus", "verus")
    output = result.stdout + result.stderr

    assert result.returncode == 0, output
    assert "4 verified, 0 errors" in output


def test_verus_rejects_a_false_union_membership_claim(tmp_path: Path) -> None:
    """Reject a well-formed false equality in a temporary proof copy."""
    mutant = _proof_directory(tmp_path) / "union_is_not_intersection.rs"
    contents = VERUS_PROOF.read_text(encoding="utf-8")
    original = "left.intersect(right).contains(value)"
    assert contents.count(original) == 5
    mutated = contents.replace(original, "left.union(right).contains(value)", 1)
    shutil.copyfile(VERUS_PROOF, mutant)
    mutant.write_text(mutated, encoding="utf-8")

    result = _run_make(
        "verus",
        "verus",
        VERUS_FLAGS=_verus_flags(mutant),
    )
    output = result.stdout + result.stderr

    assert result.returncode != 0, output
    assert "postcondition not satisfied" in output
    assert "verification results::" in output
    assert "1 errors" in output


def test_verus_stops_on_the_first_failed_file_and_names_proven_files(
    tmp_path: Path,
) -> None:
    """Run files in order and leave the third proof unstarted after failure."""
    proof_directory = _proof_directory(tmp_path)
    first = _write_proof(
        proof_directory / "first_proved.rs", "first_proof", "value == value"
    )
    second = _write_proof(
        proof_directory / "second_failed.rs", "false_proof", "value + 1 == value"
    )
    third = _write_proof(
        proof_directory / "third_must_not_run.rs", "later_proof", "true"
    )

    result = _run_make(
        "verus",
        "verus",
        VERUS_FLAGS=_verus_flags(first, second, third),
    )
    output = result.stdout + result.stderr

    assert result.returncode != 0, output
    assert "postcondition not satisfied" in output
    assert "Proved before the failure:" in output
    assert str(first) in output
    assert str(second) in output
    assert str(third) not in output


def test_verus_preflights_all_files_before_running_any_proof(tmp_path: Path) -> None:
    """A missing later file prevents execution of an earlier valid proof."""
    proof_directory = _proof_directory(tmp_path)
    first = _write_proof(
        proof_directory / "first_valid.rs", "first_proof", "value == value"
    )
    missing = proof_directory / "missing_later.rs"

    result = _run_make(
        "verus",
        "verus",
        VERUS_FLAGS=_verus_flags(first, missing),
    )
    output = result.stdout + result.stderr

    assert result.returncode != 0, output
    assert str(missing) in output
    assert "verification results::" not in output


def test_verus_preserves_success_status_when_output_readers_close_early() -> None:
    """Keep a successful verifier status after stdout and stderr close."""
    return_code = _run_cli_with_closed_readers(
        _verus_cli_command(VERUS_PROOF),
        "verus",
        **_z3_environment(),
    )

    assert return_code == 0


def test_verus_preserves_failure_status_when_output_readers_close_early(
    tmp_path: Path,
) -> None:
    """Keep the verifier's non-zero result when both output readers close."""
    failing_proof = _write_proof(
        _proof_directory(tmp_path) / "false_postcondition.rs",
        "false_proof",
        "value + 1 == value",
    )

    confirmed_failure = _run_make(
        "verus",
        "verus",
        VERUS_FLAGS=_verus_flags(failing_proof),
    )
    output = confirmed_failure.stdout + confirmed_failure.stderr
    assert confirmed_failure.returncode != 0, output
    assert "postcondition not satisfied" in output

    return_code = _run_cli_with_closed_readers(
        _verus_cli_command(failing_proof),
        "verus",
        **_z3_environment(),
    )

    assert return_code == 1
