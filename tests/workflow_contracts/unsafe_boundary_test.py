"""Contracts for the audited unsafe-boundary allow-list (ADR 015).

Every crate the repository builds is inventoried, every safe crate forbids
unsafe code in all of its targets, and every boundary crate confines unsafe
code to one reviewed bindings module. The repository itself must satisfy the
contract, and each deviation the contract exists to catch is applied to a
disposable copy of the real inputs and must be reported.

The fault cases mutate the real files, not synthetic stand-ins, and fail closed
when the fragment they mutate stops matching exactly once, so a refactor that
moves the policy cannot silently turn a fault case into a no-op.

Run via ``make test-workflow-contracts``. The compiler half of the contract,
which proves that rustc rejects unsafe probes, runs via
``make boundary-contract``.
"""

from __future__ import annotations

import functools
import shutil
import sys
from pathlib import Path

import pytest

REPOSITORY = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPOSITORY / "scripts"))

from unsafe_boundaries import files, inventory  # noqa: E402
from unsafe_boundaries import static_checks as checks  # noqa: E402

from _unsafe_boundary_faults import FAULTS, WEB_SEARCH, Fault, append  # noqa: E402


@functools.cache
def _real_manifests() -> tuple[str, ...]:
    """Return the real repository's manifest directories, discovered once."""
    return tuple(sorted(files.discover_manifests(REPOSITORY)))


def _copy_file(relative: str, destination: Path) -> None:
    """Copy one repository file to the same relative path, if it exists."""
    source = REPOSITORY / relative
    if source.exists():
        (destination / relative).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(source, destination / relative)


def _copy_boundary_sources(crate: inventory.Crate, destination: Path) -> None:
    """Copy every Rust source of a boundary crate, which the contract scans."""
    root = REPOSITORY / crate.path
    for path in files.rust_sources(root):
        _copy_file(path.relative_to(REPOSITORY).as_posix(), destination)


def _copy_safe_roots(crate: inventory.Crate, destination: Path) -> None:
    """Copy the crate roots of a safe crate, which are all the contract reads."""
    for name in ("src/lib.rs", "src/main.rs", "build.rs"):
        _copy_file(f"{crate.path}/{name}", destination)


def _copy_contract_inputs(destination: Path) -> None:
    """Copy exactly the files the static contract reads into `destination`.

    Safe crates contribute their manifest and crate roots; boundary crates
    contribute every source, because the contract scans them all; every other
    manifest is copied so inventory discovery sees the real set.
    """
    for relative in ("Makefile", *(f"{m}/Cargo.toml" for m in _real_manifests())):
        _copy_file(relative, destination)
    for crate in inventory.INVENTORY:
        if crate.role is inventory.Role.BOUNDARY:
            _copy_boundary_sources(crate, destination)
        else:
            _copy_safe_roots(crate, destination)


@pytest.fixture
def contract_copy(tmp_path: Path) -> Path:
    """Return a disposable copy of the inputs the static contract reads."""
    destination = tmp_path / "repository"
    _copy_contract_inputs(destination)
    return destination


def test_the_repository_satisfies_the_contract() -> None:
    """The real manifests and sources conform to the inventory."""
    assert checks.check_repository(REPOSITORY) == []


def test_the_pristine_copy_satisfies_the_contract(contract_copy: Path) -> None:
    """The fault cases start from a conforming copy, so a hit is the fault's."""
    assert checks.check_repository(contract_copy) == []


def test_every_inventoried_boundary_is_a_wasm_component() -> None:
    """The only boundary crates are the fourteen WebAssembly components."""
    boundaries = {
        c.path for c in inventory.INVENTORY if c.role is inventory.Role.BOUNDARY
    }
    assert boundaries == set(inventory.WASM_BOUNDARIES)
    assert len(boundaries) == 14


def test_every_exemption_carries_a_reason() -> None:
    """An exemption without a reason is an unreviewed exception."""
    assert all(reason.strip() for reason in inventory.EXEMPT.values())


@pytest.mark.parametrize("fault", FAULTS, ids=[fault.name for fault in FAULTS])
def test_each_deviation_is_reported(contract_copy: Path, fault: Fault) -> None:
    """A deviation from the allow-list produces the message that names it."""
    fault.apply(contract_copy)
    problems = checks.check_repository(contract_copy)
    assert any(fault.expected in problem for problem in problems), (
        f"{fault.name}: expected a problem containing {fault.expected!r}; "
        f"got {problems}"
    )


@pytest.mark.parametrize(
    "removed", sorted(inventory.WASM_BOUNDARIES), ids=lambda path: path
)
def test_a_crate_missing_from_the_inventory_is_reported(
    contract_copy: Path, removed: str
) -> None:
    """Dropping any boundary from the inventory leaves its manifest unlisted."""
    trimmed = tuple(c for c in inventory.INVENTORY if c.path != removed)
    problems = checks.check_repository(contract_copy, trimmed)
    assert any(
        problem.startswith(f"{removed}: Cargo manifest has no") for problem in problems
    )


@pytest.mark.parametrize("path", sorted(inventory.WASM_BOUNDARIES), ids=lambda p: p)
def test_a_boundary_reclassified_as_safe_is_reported(
    contract_copy: Path, path: str
) -> None:
    """A boundary crate cannot be waved through by relabelling it safe."""
    relabelled = tuple(
        inventory.Crate(c.path, inventory.Role.SAFE, c.standalone, c.vendored)
        if c.path == path
        else c
        for c in inventory.INVENTORY
    )
    problems = checks.check_repository(contract_copy, relabelled)
    assert any(
        problem.startswith(f"{path}: safe crate must forbid") for problem in problems
    )


NOISE = (
    "// unsafe { }\n"
    "/* unsafe /* nested unsafe */ unsafe */\n"
    'const A: &str = "unsafe {}";\n',
    'const B: &str = r#"unsafe { } "quoted" "#;\nconst C: &[u8] = br"unsafe";\n',
    "const D: char = '\"';\n"
    'const E: &str = "escaped \\" unsafe";\n'
    "fn f<'a>(x: &'a str) {}\n",
)


@pytest.mark.parametrize(
    "noise", NOISE, ids=["comments", "raw-strings", "quotes-and-lifetimes"]
)
def test_the_word_unsafe_in_comments_and_literals_is_not_unsafe_code(
    contract_copy: Path, noise: str
) -> None:
    """Generated-tool templates and prose may mention `unsafe` without tripping it."""
    append(contract_copy, f"{WEB_SEARCH}/src/lib.rs", noise)
    assert checks.check_repository(contract_copy) == []


def test_real_unsafe_after_noise_is_still_found(contract_copy: Path) -> None:
    """The lexer resumes after a literal, so code that follows it is searched."""
    append(
        contract_copy,
        f"{WEB_SEARCH}/src/lib.rs",
        'const A: &str = "x"; fn f() { unsafe {} }',
    )
    assert any("`unsafe` outside" in p for p in checks.check_repository(contract_copy))
