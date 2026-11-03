"""Check that failed Kani pin checks prevent proof execution."""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

import pytest
from _makefile_test_support import _MakeTestContext, shell_quote

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


@pytest.mark.parametrize("pin_state", ("missing", "malformed", "mismatch"))
def test_kani_pin_check_failure_stops_before_exec(
    make_context: _MakeTestContext, pin_state: str
) -> None:
    """Forward the selected pin and propagate its verifier's rejection."""
    root = make_context.tmp_path
    pin = root / "pin with spaces" / "VERSION"
    pin.parent.mkdir()
    if pin_state != "missing":
        pin.write_text("not-a-version\n" if pin_state == "malformed" else "0.66.0\n")
    log = root / "calls.jsonl"
    fake = root / "fake-prover"
    fake.write_text(
        "#!/usr/bin/env python3\n"
        "import json, os, pathlib, re, sys\n"
        "args = sys.argv[1:]\n"
        "with open(os.environ['PROVER_CALL_LOG'], 'a') as out:\n"
        "    out.write(json.dumps(args) + '\\n')\n"
        "pin = pathlib.Path(args[args.index('--version-file') + 1])\n"
        "value = pin.read_text().strip() if pin.is_file() else ''\n"
        "if not re.fullmatch(r'\\d+\\.\\d+\\.\\d+', value) or value != '0.67.0':\n"
        "    raise SystemExit(23)\n",
        encoding="utf-8",
    )
    fake.chmod(0o755)
    env = os.environ.copy()
    env.update(
        PROVER_TOOLS=shell_quote(str(fake)),
        PROVER_CALL_LOG=str(log),
        KANI_VERSION_FILE=str(pin),
        KANI_INSTALLATION=str(root / "installation"),
        KANI_PROOF_DIR=str(root / "proof"),
    )
    result = subprocess.run(
        [make_context.make_executable, "--no-print-directory", "kani"],
        cwd=REPOSITORY_ROOT,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode != 0
    calls = [json.loads(line) for line in log.read_text().splitlines()]
    assert len(calls) == 1
    assert calls[0][:2] == ["kani", "check-version"]
    assert calls[0][calls[0].index("--version-file") + 1] == str(pin)
