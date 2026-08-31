#!/usr/bin/env python3
"""Reject path-scoped cargo-audit ignores when their dependency path changes."""

from __future__ import annotations

import sys
import tomllib
from collections import defaultdict, deque
from pathlib import Path

EXPECTED_PATHS = {
    ("rkyv", "0.7.46"): "rust_decimal",
    ("h2", "0.3.27"): "libsql",
}

PackageKey = tuple[str, str]


def package_key(package: dict[str, object]) -> PackageKey | None:
    """Return a lockfile package's unambiguous name and version key."""
    name = package.get("name")
    version = package.get("version")
    if isinstance(name, str) and isinstance(version, str):
        return name, version
    return None


def dependency_keys(
    dependency: str,
    packages_by_name: dict[str, set[PackageKey]],
) -> set[PackageKey]:
    """Resolve one Cargo.lock dependency specification to package keys."""
    name, *version = dependency.split(" ", maxsplit=1)
    if version:
        return {(name, version[0])}
    return packages_by_name[name]


def build_packages_by_name(
    packages: list[dict[str, object]],
) -> defaultdict[str, set[PackageKey]]:
    """Index lockfile package keys by name for unversioned dependencies."""
    packages_by_name: defaultdict[str, set[PackageKey]] = defaultdict(set)
    for package in packages:
        if package_key_value := package_key(package):
            packages_by_name[package_key_value[0]].add(package_key_value)
    return packages_by_name


def dependency_parent_edges(
    package: dict[str, object],
    packages_by_name: dict[str, set[PackageKey]],
) -> list[tuple[PackageKey, PackageKey]]:
    """Return dependency-to-parent edges contributed by one lockfile package."""
    package_key_value = package_key(package)
    if package_key_value is None:
        return []

    dependencies = package.get("dependencies", [])
    if not isinstance(dependencies, list):
        return []

    return [
        (dependency_key, package_key_value)
        for dependency in dependencies
        if isinstance(dependency, str)
        for dependency_key in dependency_keys(dependency, packages_by_name)
    ]


def build_parent_map(
    packages: list[dict[str, object]],
    packages_by_name: dict[str, set[PackageKey]],
) -> defaultdict[PackageKey, set[PackageKey]]:
    """Map each package to the packages that depend on it."""
    parents: defaultdict[PackageKey, set[PackageKey]] = defaultdict(set)
    for package in packages:
        for dependency_key, parent_key in dependency_parent_edges(
            package, packages_by_name
        ):
            parents[dependency_key].add(parent_key)
    return parents


def reverse_paths_cross_expected_ancestor(
    parents: dict[PackageKey, set[PackageKey]],
    vulnerable_package: PackageKey,
    expected_ancestor: str,
) -> bool:
    """Check whether every reverse path reaches the expected ancestor."""
    pending = deque(parents[vulnerable_package])
    visited: set[PackageKey] = set()
    found_path = False
    while pending:
        parent = pending.popleft()
        if parent in visited:
            continue
        visited.add(parent)
        if parent[0] == expected_ancestor:
            found_path = True
            continue
        parent_dependencies = parents[parent]
        if not parent_dependencies:
            return False
        pending.extend(parent_dependencies)

    return found_path


def has_only_expected_dependency_paths(
    packages: list[dict[str, object]],
    vulnerable_package: PackageKey,
    expected_ancestor: str,
) -> bool:
    """Check that every reverse dependency path crosses the expected ancestor."""
    packages_by_name = build_packages_by_name(packages)

    if vulnerable_package not in packages_by_name[vulnerable_package[0]]:
        return True

    parents = build_parent_map(packages, packages_by_name)
    return reverse_paths_cross_expected_ancestor(
        parents, vulnerable_package, expected_ancestor
    )


def verify(lockfile: Path) -> list[str]:
    """Return messages describing invalid path-scoped audit-ignore assumptions."""
    contents = tomllib.loads(lockfile.read_text())
    packages = contents.get("package", [])
    if not isinstance(packages, list):
        return [f"{lockfile}: Cargo.lock has no package list"]

    return [
        (
            f"{lockfile}: {package[0]} is not exclusively reachable through "
            f"{expected_ancestor}; refusing its RustSec ignore"
        )
        for package, expected_ancestor in EXPECTED_PATHS.items()
        if not has_only_expected_dependency_paths(packages, package, expected_ancestor)
    ]


def main() -> int:
    """Validate the Cargo.lock path assumptions for the scoped audit ignores."""
    if len(sys.argv) != 2:
        print("usage: verify_audit_ignore_paths.py CARGO_LOCK", file=sys.stderr)
        return 2

    errors = verify(Path(sys.argv[1]))
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
