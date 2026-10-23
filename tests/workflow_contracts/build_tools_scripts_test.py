"""Behavioural contracts for the build-tools check and install scripts.

``make check-build-tools`` runs before every build, test, lint and typecheck
target, and ``make install-build-tools`` runs in every CI job that compiles, so
what they do on a bad day is part of the standard: a missing or drifted tool
must stop the build with an installation hint and never fall back to a slower
one, and an unverifiable download must never be installed.

The scripts run in the sandbox of `_build_tools_sandbox`, with the release URL
pointed at a local fixture.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import hashlib
import platform
import subprocess
import tarfile
import typing as typ
from pathlib import Path

import pytest

from _build_standard import REPOSITORY_ROOT
from _build_tools_sandbox import CHANNEL, COMPONENTS, Sandbox, fake_mold, make_sandbox

pytestmark = pytest.mark.skipif(
    platform.system() != "Linux",
    reason="the scripts are Linux-first; mold ships for Linux alone",
)

CHECK = REPOSITORY_ROOT / "scripts" / "check-build-tools.sh"
INSTALL = REPOSITORY_ROOT / "scripts" / "install-build-tools.sh"


@pytest.fixture
def sandbox(tmp_path: Path) -> Sandbox:
    """Build a sandbox where every tool is present and correct."""
    return make_sandbox(tmp_path)


def test_a_complete_environment_passes(sandbox: Sandbox) -> None:
    """With every tool present and pinned, the check succeeds quietly."""
    result = sandbox.run(CHECK)
    assert result.returncode == 0, result.stderr
    assert "mold 2.41.0" in result.stderr
    assert "available with its components and targets" in result.stderr


CHECK_FAILURES = {
    "mold-missing": (lambda s: (s.bin / "mold").unlink(), "mold not found on PATH"),
    "mold-drifted": (
        lambda s: s.stub("mold", fake_mold("2.40.4")),
        "does not match the pin 2.41.0",
    ),
    "mold-broken": (
        lambda s: s.stub("mold", "#!/usr/bin/env bash\nexit 3\n"),
        "cannot report its version",
    ),
    "clang-missing": (lambda s: (s.bin / "clang").unlink(), "clang not found on PATH"),
    "rustup-missing": (
        lambda s: (s.bin / "rustup").unlink(),
        "rustup not found on PATH",
    ),
}


@pytest.mark.parametrize("name", sorted(CHECK_FAILURES), ids=sorted(CHECK_FAILURES))
def test_the_check_refuses_with_an_actionable_message(
    sandbox: Sandbox, name: str
) -> None:
    """Each missing or drifted tool fails the check and says how to fix it."""
    change, fragment = CHECK_FAILURES[name]
    change(sandbox)
    result = sandbox.run(CHECK)
    assert result.returncode == 1, result.stderr
    assert fragment in result.stderr
    assert "capability check failed" in result.stderr


@pytest.mark.parametrize(
    ("env", "fragment"),
    [
        (
            {"STUB_TOOLCHAINS": "stable-x86_64-unknown-linux-gnu"},
            f"toolchain {CHANNEL} is not installed",
        ),
        (
            {
                "STUB_COMPONENTS": "rustfmt-x86_64-unknown-linux-gnu\nclippy-x86_64-unknown-linux-gnu"
            },
            "component llvm-tools-preview is missing",
        ),
        (
            {"STUB_TARGETS": "x86_64-unknown-linux-gnu"},
            "target wasm32-wasip2 is missing",
        ),
    ],
    ids=("toolchain-missing", "llvm-tools-component-missing", "wasm-target-missing"),
)
def test_the_check_refuses_an_incomplete_toolchain(
    sandbox: Sandbox, env: dict[str, str], fragment: str
) -> None:
    """A missing toolchain, component or target is named, not left to Cargo."""
    result = sandbox.run(CHECK, **env)
    assert result.returncode == 1
    assert fragment in result.stderr


def test_every_problem_is_reported_in_one_pass(sandbox: Sandbox) -> None:
    """A developer sees the linker and the toolchain faults together."""
    (sandbox.bin / "mold").unlink()
    result = sandbox.run(CHECK, STUB_TOOLCHAINS="stable-x86_64-unknown-linux-gnu")
    assert result.returncode == 1
    assert "mold not found" in result.stderr and "is not installed" in result.stderr


def test_an_unreadable_pin_stops_the_check(sandbox: Sandbox) -> None:
    """A missing pin file aborts instead of reporting a nonsensical drift."""
    (sandbox.root / "VERSION").unlink()
    result = sandbox.run(CHECK)
    assert result.returncode == 1
    assert "missing version pin" in result.stderr


# -- installation -------------------------------------------------------------


def _publish_mold(
    sandbox: Sandbox, *, digest: str | None = None, rows: int = 1
) -> Path:
    """Create a release directory and a SHA256SUMS for it; return the base URL path."""
    arch = platform.machine().replace("amd64", "x86_64").replace("arm64", "aarch64")
    name = f"mold-2.41.0-{arch}-linux.tar.gz"
    release = sandbox.root / "release" / "v2.41.0"
    release.mkdir(parents=True)
    staged = sandbox.root / "stage" / f"mold-2.41.0-{arch}-linux" / "bin"
    staged.mkdir(parents=True)
    (staged / "mold").write_text(fake_mold("2.41.0"), encoding="utf-8")
    (staged / "mold").chmod(0o755)
    archive = release / name
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(
            sandbox.root / "stage" / f"mold-2.41.0-{arch}-linux",
            arcname=f"mold-2.41.0-{arch}-linux",
        )
    actual = hashlib.sha256(archive.read_bytes()).hexdigest()
    line = f"{digest or actual}  {name}\n"
    (sandbox.root / "SHA256SUMS").write_text(line * rows, encoding="utf-8")
    return sandbox.root / "release"


def _install(
    sandbox: Sandbox, release: Path, **extra: str
) -> subprocess.CompletedProcess[str]:
    return sandbox.run(INSTALL, MOLD_RELEASE_BASE_URL=f"file://{release}", **extra)


def test_a_verified_release_is_installed_and_the_toolchain_requested(
    sandbox: Sandbox,
) -> None:
    """The archive lands in the prefix, and rustup is asked for the pin's full set."""
    release = _publish_mold(sandbox)
    result = _install(sandbox, release)
    assert result.returncode == 0, result.stderr
    assert (sandbox.root / "prefix" / "bin" / "mold").is_file()
    calls = sandbox.log.read_text(encoding="utf-8")
    assert f"toolchain install {CHANNEL} --profile minimal" in calls
    for component in COMPONENTS:
        assert f"--component {component}" in calls
    assert "--target wasm32-wasip2" in calls


def test_the_install_prefix_joins_the_ci_path(sandbox: Sandbox) -> None:
    """Under CI the prefix's bin goes first on PATH for the later direct steps."""
    release = _publish_mold(sandbox)
    github_path = sandbox.root / "github_path"
    result = _install(sandbox, release, GITHUB_PATH=str(github_path))
    assert result.returncode == 0, result.stderr
    assert github_path.read_text(encoding="utf-8").strip() == str(
        sandbox.root / "prefix" / "bin"
    )


@pytest.mark.parametrize(
    ("kwargs", "fragment"),
    [
        ({"digest": "0" * 64}, "checksum mismatch"),
        ({"rows": 2}, "2 checksums recorded"),
    ],
    ids=("digest-mismatch", "ambiguous-sums"),
)
def test_an_unverifiable_release_is_never_installed(
    sandbox: Sandbox, kwargs: dict[str, typ.Any], fragment: str
) -> None:
    """A bad or ambiguous digest stops the install before extraction."""
    release = _publish_mold(sandbox, **kwargs)
    result = _install(sandbox, release)
    assert result.returncode == 1
    assert fragment in result.stderr
    assert not (sandbox.root / "prefix" / "bin" / "mold").exists()
    assert not sandbox.log.exists(), (
        "the toolchain must not be fetched after a failed verification"
    )


def test_an_unlisted_artefact_is_a_hard_failure(sandbox: Sandbox) -> None:
    """No checksum recorded for the archive means no install, not a skip."""
    release = _publish_mold(sandbox)
    (sandbox.root / "SHA256SUMS").write_text("", encoding="utf-8")
    result = _install(sandbox, release)
    assert result.returncode == 1
    assert "no checksum recorded" in result.stderr


def test_a_failed_download_fails_the_install(sandbox: Sandbox) -> None:
    """An unreachable release is an error, never a silent fallback."""
    _publish_mold(sandbox)
    result = _install(
        sandbox, sandbox.root / "no-such-release", CURL_CONNECT_TIMEOUT="2"
    )
    assert result.returncode == 1
    assert "failed to download" in result.stderr


def test_a_failed_toolchain_install_fails_the_install(sandbox: Sandbox) -> None:
    """rustup failing is reported, not ignored."""
    release = _publish_mold(sandbox)
    result = _install(sandbox, release, STUB_INSTALL_STATUS="1")
    assert result.returncode == 1
    assert "failed to install toolchain" in result.stderr


def test_the_environment_is_isolated_from_the_host(sandbox: Sandbox) -> None:
    """The sandbox PATH cannot reach the host's mold, so absence is genuine.

    The fixture's TOOLCHAIN_FILE also carries a comment inside the components
    array; ``test_a_complete_environment_passes`` would fail if that comment
    were read as a component, because the stub toolchain has no such component.
    """
    (sandbox.bin / "mold").unlink()
    probe = subprocess.run(  # noqa: S603 S607 - sandboxed lookup
        ["bash", "-c", "command -v mold"],
        env={"PATH": str(sandbox.bin)},
        capture_output=True,
        text=True,
        check=False,
    )
    assert probe.returncode != 0, probe.stdout
