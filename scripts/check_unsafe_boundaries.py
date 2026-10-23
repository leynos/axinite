#!/usr/bin/env python3
"""Enforce the audited unsafe-boundary inventory and prove it with the compiler.

Axinite confines unsafe Rust to an explicit allow-list of boundary crates
(ADR 015). Everything else forbids it. Two halves:

* The static half reads manifests and sources and reports every deviation from
  the inventory in `unsafe_boundaries/inventory.py`. It is a pure function of
  the files it is given, so the workflow-contract suite mutates disposable
  copies and proves that each deviation is detected.
* The compiler half copies a safe crate, and a boundary crate of each WIT
  world, into a scratch workspace, appends deliberate unsafe probes to every
  target, and requires the compiler to reject them. A control run with the
  policy stripped must accept the same probe.

Usage::

    python3 scripts/check_unsafe_boundaries.py            # static + compiler
    python3 scripts/check_unsafe_boundaries.py --static   # static half only

Run via ``make boundary-contract``. The static half is also exercised by
``make test-workflow-contracts``.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from unsafe_boundaries.probes import probe_repository
from unsafe_boundaries.static_checks import check_repository

ROOT = Path(__file__).resolve().parent.parent


def main(argv: list[str] | None = None) -> int:
    """Check the inventory, then prove the compiler enforces it."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--static", action="store_true", help="skip compiler probes")
    arguments = parser.parse_args(argv)
    root = arguments.root.resolve()
    problems = check_repository(root)
    if not problems and not arguments.static:
        problems = probe_repository(
            root,
            scratch=root / "target/unsafe-boundary-scratch",
            logs=root / "target/unsafe-boundary-verification",
            target_dir=root / "target/unsafe-boundary-contract",
        )
    for problem in problems:
        print(f"unsafe boundary contract: {problem}", file=sys.stderr)
    if not problems:
        print("unsafe boundary contract: ok")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
