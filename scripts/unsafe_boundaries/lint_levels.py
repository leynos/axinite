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
        problems: inheriting from a missing table, or mixing inheritance with
        local overrides, which Cargo rejects and the policy forbids.
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
    """Resolve `[lints] workspace = true` against the workspace lint table."""
    problems = []
    if set(lints) - {"workspace"}:
        problems.append("`[lints] workspace = true` is mixed with local lint overrides")
    if workspace_manifest is None:
        return None, [*problems, "`workspace = true` outside a workspace member"]
    rust = workspace_manifest.get("workspace", {}).get("lints", {}).get("rust", {})
    return _level_of(rust.get("unsafe_code")), problems
