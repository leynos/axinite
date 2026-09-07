"""Contracts for the binary-only Kani consumer and its pinned assets."""

import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile

import pytest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "kani_binary_tool", ROOT / "tools/kani/binary_tool.py"
)
assert SPEC is not None and SPEC.loader is not None
binary_tool = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(binary_tool)


@pytest.fixture
def archive(tmp_path):
    path = tmp_path / "downloads/tool.tar.gz"
    path.parent.mkdir()
    with tarfile.open(path, "w:gz") as bundle:
        entry = tarfile.TarInfo("release/bin/driver")
        entry.mode = 0o755
        entry.size = len(b"binary contents")
        bundle.addfile(entry, io.BytesIO(b"binary contents"))
    asset = {
        "url": "https://example.invalid/tool.tar.gz",
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "prefix": "release/",
        "destination": ".",
    }
    return path, asset


def test_cached_download_is_rehashed(tmp_path, archive):
    path, asset = archive
    assert binary_tool.archive_path(tmp_path, asset, download=False) == path
    path.write_bytes(b"corrupted archive")
    with pytest.raises(RuntimeError, match="Checksum failure for cached"):
        binary_tool.archive_path(tmp_path, asset, download=False)


@pytest.mark.parametrize("corruption", ["contents", "mode", "missing", "symlink"])
def test_cached_installation_is_checked_against_verified_archive(
    tmp_path, archive, corruption
):
    path, asset = archive
    base = tmp_path / "installation"
    binary_tool.materialize(path, asset, base, fresh=True)
    binary_tool.materialize(path, asset, base, fresh=False)
    target = base / "bin/driver"
    if corruption == "contents":
        target.write_bytes(b"changed executable")
    elif corruption == "mode":
        target.chmod(0o644)
    else:
        target.unlink()
        if corruption == "symlink":
            target.symlink_to(path)
    with pytest.raises(RuntimeError, match="Cached binary mismatch"):
        binary_tool.materialize(path, asset, base, fresh=False)


def test_missing_component_fails(tmp_path, archive):
    path, asset = archive
    asset["prefix"] = "absent/"
    with pytest.raises(RuntimeError, match="Missing required component"):
        binary_tool.materialize(path, asset, tmp_path / "installation", fresh=True)


def test_unsupported_platform_fails_before_install(monkeypatch):
    monkeypatch.setattr(binary_tool.platform, "system", lambda: "Windows")
    with pytest.raises(RuntimeError, match="only Linux x86_64"):
        binary_tool.configuration()


def test_asset_pins_are_complete_and_official():
    pins = json.loads((ROOT / "tools/kani/binaries.json").read_text())
    assert pins["version"] == (ROOT / "tools/kani/VERSION").read_text().strip()
    assert len(pins["assets"]) == 4
    for asset in pins["assets"]:
        assert len(bytes.fromhex(asset["sha256"])) == 32
        assert asset["url"].startswith(
            (
                "https://github.com/model-checking/kani/releases/download/kani-0.67.0/",
                "https://static.rust-lang.org/dist/2025-11-21/",
            )
        )


@pytest.mark.parametrize(
    "stdout", ["", "Complete - 0 successfully verified harnesses, 0 failures, 0 total."]
)
def test_installation_rejects_missing_smoke_proof(monkeypatch, tmp_path, stdout):
    monkeypatch.setattr(
        binary_tool.subprocess,
        "run",
        lambda *args, **kwargs: binary_tool.subprocess.CompletedProcess(
            args, 0, stdout, ""
        ),
    )
    with pytest.raises(RuntimeError, match="expected successful proof"):
        binary_tool.invoke(
            tmp_path, [], cargo=False, expected="binary_installation_smoke"
        )


def test_formal_job_requires_installation_and_proofs():
    import yaml

    workflow = yaml.safe_load((ROOT / ".github/workflows/formal.yml").read_text())
    job = workflow["jobs"]["kani-smoke"]
    commands = [step["run"] for step in job["steps"] if "run" in step]
    assert commands == ["make install-kani", "make kani"]
    assert job["runs-on"] == "ubuntu-latest"
    assert job["timeout-minutes"] == 20
    assert "continue-on-error" not in job
    assert all(
        "continue-on-error" not in step and "if" not in step
        for step in job["steps"]
        if "run" in step
    )


def test_proof_package_is_independent_and_uses_shared_source():
    import tomllib

    manifest = tomllib.loads(
        (ROOT / "verification/repair-claims/Cargo.toml").read_text()
    )
    application = tomllib.loads((ROOT / "Cargo.toml").read_text())
    assert manifest["workspace"] == {}
    assert not manifest.get("dependencies")
    assert not manifest.get("dev-dependencies")
    assert "verification/repair-claims" in application["workspace"]["exclude"]
    source = (ROOT / "verification/repair-claims/src/lib.rs").read_text()
    assert '#[path = "../../../src/agent/self_repair/claim_registry.rs"]' in source


def test_missing_version_checker_does_not_bootstrap(tmp_path, monkeypatch):
    calls = []
    monkeypatch.setattr(
        binary_tool.subprocess, "run", lambda *args, **kwargs: calls.append(args)
    )
    with pytest.raises(RuntimeError, match="Missing prepared version checker"):
        binary_tool.check_version(tmp_path, tmp_path, {})
    assert not calls
