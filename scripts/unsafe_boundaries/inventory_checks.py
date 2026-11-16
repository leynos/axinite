"""Check that every Cargo manifest is inventoried and the workspace matches."""

from __future__ import annotations

import typing as typ

from .files import discover_manifests, read_manifest
from .inventory import EXEMPT, INVENTORY, Crate

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Sequence
    from pathlib import Path


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
    standalone = {c.path for c in inventory if c.standalone}
    return [
        *_unexpected(
            "workspace.members",
            set(workspace.get("members", [])),
            {c.path for c in inventory if not c.standalone},
            "an inventoried root-workspace crate",
        ),
        *_unexpected(
            "workspace.exclude",
            set(workspace.get("exclude", [])) - standalone,
            set(),
            "an inventoried standalone root",
        ),
        *_missing_members(
            set(workspace.get("members", [])),
            {c.path for c in inventory if not c.standalone},
        ),
        *_without_own_workspace(root, standalone),
    ]


def _unexpected(
    label: str, actual: set[str], allowed: set[str], what: str
) -> list[str]:
    """Report each entry of `actual` that is not in `allowed`."""
    return [f"{label}: {path!r} is not {what}" for path in sorted(actual - allowed)]


def _missing_members(members: set[str], expected: set[str]) -> list[str]:
    """Report each inventoried root-workspace crate the workspace omits."""
    return [
        f"workspace.members: inventoried crate {path!r} is missing"
        for path in sorted(expected - members)
    ]


def _without_own_workspace(root: Path, standalone: set[str]) -> list[str]:
    """Report standalone roots whose manifest has no `[workspace]` table."""
    manifests = {path: root / path / "Cargo.toml" for path in sorted(standalone)}
    return [
        f"{path}: standalone root declares no [workspace] table of its own"
        for path, manifest in manifests.items()
        if manifest.exists() and "workspace" not in read_manifest(manifest)
    ]
