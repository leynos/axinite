"""Check that `make miri` interprets every boundary crate that has tests."""

from __future__ import annotations

import re
import typing as typ

from .bindings_checks import declared_abi_test_modules
from .files import read_text, rust_sources
from .inventory import INVENTORY, Crate, Role
from .rust_lex import strip_rust

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Sequence
    from pathlib import Path


def makefile_list(makefile_text: str, name: str) -> list[str]:
    """Return the words of a simple Makefile list variable, continuations joined."""
    pattern = re.compile(rf"^{re.escape(name)}\s*:?=((?:.*\\\n)*.*)$", re.MULTILINE)
    match = pattern.search(makefile_text)
    if match is None:
        return []
    return match.group(1).replace("\\\n", " ").split()


def miri_component_roots(makefile_text: str) -> list[str]:
    """Return the component roots `make miri` interprets, in Makefile order."""
    return makefile_list(makefile_text, "MIRI_COMPONENT_ROOTS")


def _has_host_tests(crate_dir: Path) -> bool:
    """Report whether a crate has a test that `cargo test` runs on the host."""
    return any(
        re.search(r"#\[test\]", strip_rust(read_text(source)))
        for source in rust_sources(crate_dir)
    )


def check_miri_roots(
    root: Path, makefile_text: str, inventory: Sequence[Crate] = INVENTORY
) -> list[str]:
    """Require `make miri` to interpret every boundary crate that has tests.

    A component crate whose tests are missing from `MIRI_COMPONENT_ROOTS` runs
    natively but never under Miri, and nothing else would notice. A listed root
    that is not a boundary crate would run unreviewed code under Miri. The ABI
    round trips, which run with the aliasing model off, are confined to the
    crates that declare them and to exactly those crates.
    """
    listed = miri_component_roots(makefile_text)
    boundaries = {c.path for c in inventory if c.role is Role.BOUNDARY}
    problems = [
        f"Makefile MIRI_COMPONENT_ROOTS lists {path!r}, which is not a boundary crate"
        for path in listed
        if path not in boundaries
    ]
    problems += [
        f"{path}: has host tests but is missing from Makefile MIRI_COMPONENT_ROOTS"
        for path in sorted(boundaries - set(listed))
        if _has_host_tests(root / path)
    ]
    problems += [
        f"{path}: listed in MIRI_COMPONENT_ROOTS but has no host tests"
        for path in listed
        if path in boundaries and not _has_host_tests(root / path)
    ]
    return problems + _check_abi_roots(root, makefile_text, boundaries)


def _check_abi_roots(root: Path, makefile_text: str, boundaries: set[str]) -> list[str]:
    """Require `MIRI_ABI_ROOTS` to name exactly the crates with ABI test modules."""
    listed = set(makefile_list(makefile_text, "MIRI_ABI_ROOTS"))
    declared = {path for path in boundaries if declared_abi_test_modules(root / path)}
    problems = [
        f"{path}: declares an ABI round-trip module but is missing from "
        "Makefile MIRI_ABI_ROOTS"
        for path in sorted(declared - listed)
    ]
    problems += [
        f"Makefile MIRI_ABI_ROOTS lists {path!r}, which declares no ABI round trips"
        for path in sorted(listed - declared)
    ]
    return problems
