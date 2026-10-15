"""Keep private security patches in every supported distribution build."""

from __future__ import annotations

import copy
import typing as typ
import xml.etree.ElementTree as ET
from pathlib import Path

import pytest
from _sources import read_text, read_toml

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


def _assert_distribution_boundary(manifest: typ.Mapping[str, typ.Any]) -> None:
    """Reject registry publication or accidental binary-release exclusion."""
    package = manifest["package"]
    assert package.get("publish") is False, "registry publication loses local patches"
    distribution = package.get("metadata", {}).get("dist", {})
    assert distribution.get("dist") is True, "source-built binaries must remain selected"


def test_root_retains_source_and_binary_distribution() -> None:
    """The root explicitly excludes registry archives and selects binary releases."""
    _assert_distribution_boundary(read_toml(REPOSITORY_ROOT / "Cargo.toml"))


@pytest.mark.parametrize("publish", [None, True, ["crates-io"]])
def test_registry_publication_regressions_are_rejected(publish: object) -> None:
    """Missing, unrestricted and registry-specific publication all lose patches."""
    manifest = copy.deepcopy(read_toml(REPOSITORY_ROOT / "Cargo.toml"))
    if publish is None:
        manifest["package"].pop("publish", None)
    else:
        manifest["package"]["publish"] = publish
    with pytest.raises(AssertionError, match="registry publication"):
        _assert_distribution_boundary(manifest)


@pytest.mark.parametrize("dist", [None, False])
def test_binary_selection_regressions_are_rejected(dist: object) -> None:
    """Cargo-dist must not inherit publish=false as binary exclusion."""
    manifest = copy.deepcopy(read_toml(REPOSITORY_ROOT / "Cargo.toml"))
    if dist is None:
        manifest["package"]["metadata"]["dist"].pop("dist", None)
    else:
        manifest["package"]["metadata"]["dist"]["dist"] = dist
    with pytest.raises(AssertionError, match="source-built binaries"):
        _assert_distribution_boundary(manifest)


@pytest.mark.parametrize("crate", ["ambient-fs", "hyper-rustls"])
def test_private_workspace_crates_remain_non_publishable(crate: str) -> None:
    """Private filesystem and transport patches stay in the source workspace."""
    manifest = read_toml(REPOSITORY_ROOT / "crates" / crate / "Cargo.toml")
    assert manifest["package"].get("publish") is False


def test_windows_installer_matches_package_authors_and_upgrade_identity() -> None:
    """Keep generated MSI branding current without changing upgrade identities."""
    package = read_toml(REPOSITORY_ROOT / "Cargo.toml")["package"]
    wix = package["metadata"]["wix"]
    manufacturer = "; ".join(
        author.split("<", 1)[0].strip() for author in package["authors"]
    )
    document = ET.fromstring(read_text(REPOSITORY_ROOT / "wix" / "main.wxs"))
    namespace = "{http://schemas.microsoft.com/wix/2006/wi}"
    product = document.find(f"{namespace}Product")
    assert product is not None, "MSI template must declare the product"
    installer = product.find(f"{namespace}Package")
    assert installer is not None, "MSI template must declare the installer package"
    assert product.get("Manufacturer") == manufacturer
    assert installer.get("Manufacturer") == manufacturer
    assert product.get("UpgradeCode") == wix["upgrade-guid"]
    path = product.find(f".//{namespace}Component[@Id='Path']")
    assert path is not None, "MSI template must retain the PATH component"
    assert path.get("Guid") == wix["path-guid"]
