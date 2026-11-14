"""Resolve the `unsafe_code` level a Cargo manifest imposes on every target."""

from __future__ import annotations

import typing as typ


def _level_of(value: object) -> str | None:
    """Return the level string of a lint entry (`"forbid"` or `{level = ..}`)."""
    match value:
        case str() as level:
            return level
        case {"level": str() as level}:
            return level
        case _:
            return None


def effective_unsafe_level(
    manifest: dict[str, typ.Any], workspace_manifest: dict[str, typ.Any] | None
) -> tuple[str | None, list[str]]:
    """Resolve the `unsafe_code` level a manifest imposes on every target.

    Parameters
    ----------
    manifest
        The crate's parsed manifest.
    workspace_manifest
        The parsed root workspace manifest, used when the crate inherits the
        workspace lint table. `None` for a standalone root.

    Returns
    -------
    tuple
        The level (`None` when the manifest sets none) and any structural
        problems: inheriting from a missing or malformed table, or mixing
        inheritance with local overrides, which Cargo rejects and the policy
        forbids.
    """
    lints = manifest.get("lints", {})
    if not isinstance(lints, dict):
        return None, ["`lints` is not a table"]
    if lints.get("workspace") is True:
        return _inherited_level(lints, workspace_manifest)
    rust = lints.get("rust", {})
    level = _level_of(rust.get("unsafe_code")) if isinstance(rust, dict) else None
    return level, []


def _inherited_level(
    lints: dict[str, typ.Any], workspace_manifest: dict[str, typ.Any] | None
) -> tuple[str | None, list[str]]:
    """Resolve `[lints] workspace = true` against the workspace lint table.

    Each level of the inherited table is checked before it is read: a malformed
    root manifest is a structural problem the contract reports by name, not an
    `AttributeError` that stops the checker.
    """
    problems = []
    if set(lints) - {"workspace"}:
        problems.append("`[lints] workspace = true` is mixed with local lint overrides")
    if workspace_manifest is None:
        return None, [*problems, "`workspace = true` outside a workspace member"]
    workspace, problem = _table(workspace_manifest, "workspace", "`[workspace]`")
    if problem is not None:
        return None, [*problems, problem]
    inherited, problem = _table(workspace, "lints", "workspace `[lints]`")
    if problem is not None:
        return None, [*problems, problem]
    rust, problem = _table(inherited, "rust", "workspace `[lints.rust]`")
    if problem is not None:
        return None, [*problems, problem]
    return _level_of(rust.get("unsafe_code")), problems


def _table(
    parent: dict[str, typ.Any], key: str, label: str
) -> tuple[dict[str, typ.Any], str | None]:
    """Return one nested table, or a problem naming it when it is not a table."""
    value = parent.get(key, {})
    if isinstance(value, dict):
        return value, None
    return {}, f"{label} is not a table"
