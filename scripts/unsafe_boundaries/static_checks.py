"""Static checks: inventory, lint levels, and where unsafe code may appear.

Every function here is a pure function of the files under a root directory, so
tests can build a disposable copy, mutate one input, and require a violation.
"""

from __future__ import annotations

import re
import typing as typ

from .bindings_checks import (
    check_abi_test_module,
    check_bindings_file,
    declared_abi_test_modules,
)
from .files import discover_manifests, read_manifest, read_text, rust_sources
from .inventory import (
    BINDINGS_DIRECTORY,
    BINDINGS_FILE,
    BINDINGS_MODULE,
    EXEMPT,
    EXPECT_REASON,
    INVENTORY,
    Crate,
    Role,
)
from .lint_levels import effective_unsafe_level
from .rust_lex import ABI_ATTRIBUTES, attribute_bodies, strip_rust

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Sequence
    from pathlib import Path


_EXPECT_DECLARATION = re.compile(
    r"#\[expect\(\s*unsafe_code\s*,\s*reason\s*=\s*"
    + re.escape(f'"{EXPECT_REASON}"')
    + r"\s*,?\s*\)\]\s*mod\s+"
    + BINDINGS_MODULE
    + r"\s*;"
)


def check_inventory(root: Path, inventory: Sequence[Crate] = INVENTORY) -> list[str]:
    """Require every manifest to be inventoried and the workspace to match it.

    Parameters
    ----------
    root
        Repository root.
    inventory
        The crate inventory to check against.

    Returns
    -------
    list of str
        One message per deviation.
    """
    problems: list[str] = []
    listed = {crate.path for crate in inventory}
    found = discover_manifests(root)
    problems += [
        f"{path}: Cargo manifest has no unsafe-boundary inventory decision"
        for path in sorted(found - listed - set(EXEMPT))
    ]
    problems += [
        f"{path}: inventoried crate has no manifest" for path in sorted(listed - found)
    ]
    problems += [
        f"{path}: exempt manifest no longer exists"
        for path in sorted(set(EXEMPT) - found)
    ]
    problems += [
        f"{path}: is both inventoried and exempt"
        for path in sorted(listed & set(EXEMPT))
    ]
    problems += _check_workspace_membership(root, inventory)
    return problems


def _check_workspace_membership(root: Path, inventory: Sequence[Crate]) -> list[str]:
    """Compare the root workspace's members and exclusions to the inventory."""
    manifest_path = root / "Cargo.toml"
    if not manifest_path.exists():
        return ["Cargo.toml: root manifest is missing"]
    workspace = read_manifest(manifest_path).get("workspace", {})
    members = set(workspace.get("members", []))
    excluded = set(workspace.get("exclude", []))
    expected_members = {c.path for c in inventory if not c.standalone}
    standalone = {c.path for c in inventory if c.standalone}
    problems = [
        f"workspace.members: {path!r} is not an inventoried root-workspace crate"
        for path in sorted(members - expected_members)
    ]
    problems += [
        f"workspace.members: inventoried crate {path!r} is missing"
        for path in sorted(expected_members - members)
    ]
    problems += [
        f"workspace.exclude: {path!r} is not an inventoried standalone root"
        for path in sorted(excluded - standalone)
    ]
    problems += [
        f"{path}: standalone root declares no [workspace] table of its own"
        for path in sorted(standalone)
        if (root / path / "Cargo.toml").exists()
        and "workspace" not in read_manifest(root / path / "Cargo.toml")
    ]
    return problems


def check_safe_crate(root: Path, crate: Crate) -> list[str]:
    """Require a safe crate to forbid unsafe code in every target.

    The manifest lint table covers every Cargo target, present and future. The
    crate-root attribute makes the prohibition visible beside the code, and
    is required for first-party library, binary and build-script roots.
    """
    manifest = read_manifest(root / crate.path / "Cargo.toml")
    workspace = None if crate.standalone else read_manifest(root / "Cargo.toml")
    level, problems = effective_unsafe_level(manifest, workspace)
    if level != "forbid":
        problems.append(
            f"safe crate must forbid unsafe_code in its lint table, not {level!r}"
        )
    if not crate.vendored:
        problems += _missing_root_attributes(root / crate.path)
    return [f"{crate.path}: {problem}" for problem in problems]


def _missing_root_attributes(crate_dir: Path) -> list[str]:
    """Return a message per crate root lacking `#![forbid(unsafe_code)]`."""
    problems = []
    for relative in ("src/lib.rs", "src/main.rs", "build.rs"):
        source = strip_rust(read_text(crate_dir / relative))
        if not source:
            continue
        if not re.search(r"#!\[\s*forbid\(\s*unsafe_code\s*\)\s*\]", source):
            problems.append(f"{relative} lacks #![forbid(unsafe_code)]")
    return problems


def check_boundary_crate(root: Path, crate: Crate) -> list[str]:
    """Require a boundary crate to confine unsafe code to its bindings module.

    The crate must deny (not inherit, not weaken) `unsafe_code`, declare the
    bindings module behind exactly one reasoned `expect`, hold no `unsafe`
    token, ABI attribute or lint override anywhere else, and keep the
    bindings file to generated-binding invocations only.
    """
    crate_dir = root / crate.path
    manifest = read_manifest(crate_dir / "Cargo.toml")
    level, problems = effective_unsafe_level(manifest, None)
    if level != "deny":
        problems.append(
            f"boundary crate must deny unsafe_code in its own table, not {level!r}"
        )
    problems += _check_declaration(crate_dir)
    abi_tests = declared_abi_test_modules(crate_dir)
    for source in sorted(rust_sources(crate_dir)):
        relative = source.relative_to(crate_dir).as_posix()
        text = read_text(source)
        if relative == BINDINGS_FILE:
            problems += check_bindings_file(source)
        elif relative in abi_tests:
            problems += check_abi_test_module(relative, text)
        elif relative.startswith(BINDINGS_DIRECTORY):
            problems += [
                f"{relative}: module is not declared as a 32-bit test module "
                f"by {BINDINGS_FILE}"
            ]
        else:
            problems += _check_safe_module(relative, text)
    return [f"{crate.path}: {problem}" for problem in problems]


def _check_declaration(crate_dir: Path) -> list[str]:
    """Require exactly one reasoned expect declaring the bindings module."""
    lib = strip_rust(read_text(crate_dir / "src/lib.rs"), keep_literals=True)
    declarations = _EXPECT_DECLARATION.findall(lib)
    if len(declarations) != 1:
        return [
            "src/lib.rs must declare the bindings module exactly once behind "
            f'#[expect(unsafe_code, reason = "{EXPECT_REASON}")]'
        ]
    return []


def _check_safe_module(relative: str, text: str) -> list[str]:
    """Reject unsafe, ABI attributes and lint overrides outside the bindings.

    In `src/lib.rs` the one approved `#[expect(unsafe_code, ...)]` declaration
    is removed before scanning, so a second override, or a changed reason,
    still fails.
    """
    literal_text = strip_rust(text, keep_literals=True)
    if relative == "src/lib.rs":
        literal_text = _EXPECT_DECLARATION.sub(
            f"mod {BINDINGS_MODULE};", literal_text, count=1
        )
    stripped = strip_rust(literal_text)
    problems = []
    if re.search(r"\bunsafe\b", stripped):
        problems.append(f"{relative}: `unsafe` outside {BINDINGS_FILE}")
    for body in attribute_bodies(stripped):
        if ABI_ATTRIBUTES.search(body):
            problems.append(
                f"{relative}: ABI attribute outside {BINDINGS_FILE}: #[{body.strip()}]"
            )
        if "unsafe_code" in body:
            problems.append(f"{relative}: unsafe_code lint override: #[{body.strip()}]")
    return problems


def check_repository(root: Path, inventory: Sequence[Crate] = INVENTORY) -> list[str]:
    """Run every static check and return all violations.

    Parameters
    ----------
    root
        Repository root (or a disposable copy of the files the checks read).
    inventory
        The crate inventory to enforce.

    Returns
    -------
    list of str
        One message per violation; empty when the contract holds.
    """
    problems = check_inventory(root, inventory)
    for crate in inventory:
        if not (root / crate.path / "Cargo.toml").exists():
            continue
        if crate.role is Role.SAFE:
            problems += check_safe_crate(root, crate)
        else:
            problems += check_boundary_crate(root, crate)
    return problems
