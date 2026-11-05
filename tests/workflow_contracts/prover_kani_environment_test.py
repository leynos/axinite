"""Kani installation isolation and encoded compiler-flag contracts."""

from __future__ import annotations

import shlex

import pytest
from _makefile_test_support import _MakeTestContext
from prover_tools_test import _calls, _kani_inputs, _prover_harness


@pytest.mark.parametrize("encoded", ["", "--cfg\x1fprover_contract"])
def test_encoded_flags_reach_kani_cli_unchanged(
    make_context: _MakeTestContext, encoded: str
) -> None:
    """Preserve an explicitly empty value as well as separated arguments."""
    harness = _prover_harness(make_context)
    result = harness.run(
        "kani",
        **_kani_inputs(make_context.tmp_path),
        CARGO_ENCODED_RUSTFLAGS=encoded,
    )
    assert result.returncode == 0, result.stderr
    assert all(
        call["env"]["CARGO_ENCODED_RUSTFLAGS"] == encoded
        for call in _calls(harness.log)
    )


def test_kani_install_and_check_select_worktree_local_frontend(
    make_context: _MakeTestContext,
) -> None:
    """Install and check the same explicitly selected frontend, even with spaces."""
    harness = _prover_harness(make_context)
    inputs = _kani_inputs(make_context.tmp_path)
    frontend = make_context.tmp_path / "frontend with spaces"
    home = make_context.tmp_path / "kani home with spaces"
    inputs.update(KANI_BIN_DIR=str(frontend), KANI_HOME=str(home))
    for target in ("install-kani", "kani-check"):
        result = harness.run(target, **inputs)
        assert result.returncode == 0, result.stderr
    install, check = _calls(harness.log)
    args = install["argv"]
    assert args[args.index("--install-dir") + 1] == str(frontend)
    assert install["env"]["KANI_HOME"] == str(home)
    check_args = check["argv"]
    command = check_args[check_args.index("--kani-command") + 1]
    assert shlex.split(command) == [str(frontend / "cargo-kani")]
    assert check["env"]["KANI_HOME"] == str(home)
