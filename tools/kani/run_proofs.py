#!/usr/bin/env python3
"""Fail closed on the isolated package's required lifecycle proof inventory."""

from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = (
    "same_tool_is_refused_while_held",
    "distinct_tools_coexist",
    "dropped_claim_can_be_reclaimed",
    "bounded_operation_sequences",
)


def main() -> int:
    manifest = ROOT / "verification/repair-claims/Cargo.toml"
    lockfile = manifest.with_name("Cargo.lock")
    original_lock = lockfile.read_bytes()
    wrapper = ROOT / "tools/kani/binary_tool.py"
    # The compatibility probe is intentionally a separate failing prerequisite:
    # it exposes unsupported RandomState entropy before a costly lifecycle run.
    scratch = ROOT / "target/repair-claims-kani/compatibility"
    scratch.mkdir(parents=True, exist_ok=True)
    reproducer = scratch / "hashset-reproducer.rs"
    shutil.copyfile(ROOT / "tools/kani/hashset-reproducer.rs", reproducer)
    result = subprocess.run(
        [
            sys.executable,
            str(wrapper),
            "standalone",
            str(reproducer),
            "--output-format",
            "terse",
        ],
        check=False,
    )
    if result.returncode:
        print(
            "Lifecycle verification blocked by the standard-library compatibility probe.",
            file=sys.stderr,
        )
        return result.returncode
    result = subprocess.run(
        [
            sys.executable,
            str(wrapper),
            "cargo",
            "--manifest-path",
            str(manifest),
            "--target-dir",
            str(ROOT / "target/repair-claims-kani"),
            "--lib",
            "--output-format",
            "terse",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    if lockfile.read_bytes() != original_lock:
        print(
            "Kani changed the isolated lockfile; refusing proof result.",
            file=sys.stderr,
        )
        return 1
    if result.returncode:
        return result.returncode
    for harness in EXPECTED:
        marker = f"Checking harness claim_registry::verification::{harness}..."
        if marker not in result.stdout:
            print(
                f"Missing required lifecycle proof result: {harness}", file=sys.stderr
            )
            return 1
    if (
        "Complete - 4 successfully verified harnesses, 0 failures, 4 total."
        not in result.stdout
    ):
        print(
            "Expected exactly four successful lifecycle proofs; refusing incomplete results.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
