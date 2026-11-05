"""Real Kani checks for the bounded capability projection example."""

from __future__ import annotations

import shutil
from pathlib import Path

from prover_test_support import REPOSITORY_ROOT, _run_make

KANI_PACKAGE = REPOSITORY_ROOT / "tests" / "verification" / "kani-smoke"
KANI_EXPECTED_FAILURE = "projected membership must match"


def test_kani_proves_the_named_four_entry_projection_harness() -> None:
    """Run the configured harness and require Kani's successful result."""
    result = _run_make("kani", "kani")
    output = result.stdout + result.stderr

    assert result.returncode == 0, output
    assert "verify_exact_projection_four_entries" in output
    assert "VERIFICATION:- SUCCESSFUL" in output


def test_kani_rejects_a_union_mutant_in_a_temporary_package(tmp_path: Path) -> None:
    """Show arbitrary inputs distinguish intersection from a faulty union."""
    mutant_package = tmp_path / "kani mutant package"
    shutil.copytree(KANI_PACKAGE, mutant_package)
    source = mutant_package / "src" / "lib.rs"
    contents = source.read_text(encoding="utf-8")
    original = "requested & permitted & FOUR_ENTRY_MASK"
    assert contents.count(original) == 1
    source.write_text(
        contents.replace(original, "requested | permitted", 1),
        encoding="utf-8",
    )

    result = _run_make(
        "kani",
        "kani",
        KANI_PROOF_DIR=str(mutant_package),
    )
    output = result.stdout + result.stderr

    assert result.returncode != 0, output
    assert "VERIFICATION:- FAILED" in output
    assert KANI_EXPECTED_FAILURE in output
