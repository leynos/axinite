#!/usr/bin/env python3
"""Consume pinned Linux Kani/Rust binaries; never build or install tools via Cargo."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import shlex
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
PINS = Path(__file__).with_name("binaries.json")


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def configuration() -> tuple[dict, Path]:
    pins = json.loads(PINS.read_text())
    if (
        platform.system() != "Linux"
        or platform.machine() != "x86_64"
        or platform.libc_ver()[0] != "glibc"
    ):
        raise RuntimeError("Kani binary setup supports only Linux x86_64 (GNU libc).")
    if pins["version"] != PINS.with_name("VERSION").read_text().strip():
        raise RuntimeError("Kani VERSION and binaries.json disagree; update both pins.")
    cache = Path(os.environ.get("KANI_BINARY_CACHE", ROOT / ".kani-binaries"))
    return pins, cache.resolve()


def archive_path(cache: Path, asset: dict, *, download: bool) -> Path:
    archive = cache / "downloads" / asset["url"].rsplit("/", 1)[-1]
    if not archive.exists():
        if not download:
            raise RuntimeError(f"Missing {archive}; run make install-kani.")
        archive.parent.mkdir(parents=True, exist_ok=True)
        print(f"Downloading pinned asset {asset['url']}", flush=True)
        with tempfile.NamedTemporaryFile(dir=archive.parent, delete=False) as stream:
            temporary = Path(stream.name)
            try:
                with urllib.request.urlopen(asset["url"], timeout=120) as response:
                    shutil.copyfileobj(response, stream)
                stream.flush()
                if digest(temporary) != asset["sha256"]:
                    raise RuntimeError(
                        f"Checksum failure for {asset['url']}; refusing asset."
                    )
                temporary.replace(archive)
            finally:
                temporary.unlink(missing_ok=True)
    if digest(archive) != asset["sha256"]:
        raise RuntimeError(
            f"Checksum failure for cached {archive}; remove this archive and retry."
        )
    return archive


def component_files(archive: Path, asset: dict):
    """Yield only the pinned component's files from a verified upstream archive."""
    with tarfile.open(archive) as bundle:
        for member in bundle:
            if not member.name.startswith(asset["prefix"]):
                continue
            relative = Path(member.name.removeprefix(asset["prefix"]))
            if relative.is_absolute() or ".." in relative.parts:
                raise RuntimeError(f"Unsafe archive path: {member.name}")
            # Rust component manifests are installation recipes, not binaries.
            if relative == Path("manifest.in") or member.isdir():
                continue
            yield bundle, member, Path(asset["destination"]) / relative


def materialize(archive: Path, asset: dict, base: Path, *, fresh: bool) -> None:
    count = 0
    for bundle, member, relative in component_files(archive, asset):
        count += 1
        target = base / relative
        if not target.parent.resolve().is_relative_to(base.resolve()):
            raise RuntimeError(f"Binary path escapes installation: {target}")
        if member.issym():
            link = Path(member.linkname)
            if link.is_absolute() or not (
                target.parent / link
            ).resolve().is_relative_to(base):
                raise RuntimeError(f"Unsafe binary symlink: {member.name}")
            if fresh:
                target.parent.mkdir(parents=True, exist_ok=True)
                target.symlink_to(link)
            elif not target.is_symlink() or os.readlink(target) != member.linkname:
                raise RuntimeError(f"Cached binary symlink mismatch: {target}")
        elif member.isfile():
            source = bundle.extractfile(member)
            if source is None:
                raise RuntimeError(f"Missing archive contents: {member.name}")
            with source:
                if fresh:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    with target.open("wb") as output:
                        shutil.copyfileobj(source, output)
                    target.chmod(member.mode & 0o777)
                elif (
                    target.is_symlink()
                    or not target.is_file()
                    or digest(target)
                    != hashlib.file_digest(source, "sha256").hexdigest()
                    or (target.stat().st_mode & 0o777) != (member.mode & 0o777)
                ):
                    raise RuntimeError(
                        f"Cached binary mismatch: {target}; remove the installation and retry."
                    )
        else:
            raise RuntimeError(f"Unsupported binary archive member: {member.name}")
    if count == 0:
        raise RuntimeError(f"Missing required component {asset['prefix']} in {archive}")


def environment(base: Path) -> dict[str, str]:
    env = os.environ.copy()
    # The proof compiler owns its flags. Suppress inherited application wrappers
    # and linker settings without overriding the flags Kani subsequently adds.
    for name in ("RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
        env.pop(name, None)
    env["CARGO_ENCODED_RUSTFLAGS"] = ""
    env["CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER"] = "cc"
    env["PATH"] = f"{base}/bin:{base}/toolchain/bin:{env.get('PATH', '')}"
    env["LD_LIBRARY_PATH"] = f"{base}/toolchain/lib:{base}/lib"
    env["RUSTUP_TOOLCHAIN"] = str(base / "toolchain")
    env["CARGO_NET_OFFLINE"] = "true"
    env["CARGO_BUILD_JOBS"] = "2"
    return env


def validate_components(base: Path, pins: dict) -> None:
    required = [
        "bin/kani-driver",
        "bin/kani-compiler",
        "bin/cbmc",
        "bin/goto-cc",
        "bin/goto-instrument",
        "bin/goto-analyzer",
        "bin/kissat",
        "toolchain/bin/cargo",
        "toolchain/bin/rustc",
        "lib/libstd.rlib",
        "lib/libkani.rlib",
        "lib/libkani_macros.so",
        "rust-toolchain-version",
    ]
    for name in required:
        if not (base / name).is_file():
            raise RuntimeError(f"Missing required binary component: {base / name}")
    result = subprocess.run(
        [str(base / "toolchain/bin/rustc"), "--version"],
        env=environment(base),
        check=True,
        capture_output=True,
        text=True,
    )
    if result.stdout.strip() != pins["rustc"]:
        raise RuntimeError(f"Wrong bundled rustc: {result.stdout.strip()}")


def installation(pins: dict, cache: Path, *, install: bool) -> Path:
    base = cache / f"kani-{pins['version']}"
    if base.is_symlink():
        raise RuntimeError(f"Binary installation must not be a symlink: {base}")
    fresh = not base.exists()
    if fresh and not install:
        raise RuntimeError("Missing binary Kani installation; run make install-kani.")
    archives = [
        (archive_path(cache, asset, download=install), asset)
        for asset in [*pins["assets"], pins["checker"]]
    ]
    if fresh:
        cache.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=cache, prefix="unpack-") as temporary:
            staging = Path(temporary)
            for archive, asset in archives:
                materialize(archive, asset, staging, fresh=True)
            validate_components(staging, pins)
            staging.rename(base)
    else:
        for archive, asset in archives:
            materialize(archive, asset, base, fresh=False)
        validate_components(base, pins)
    return base


def invoke(
    base: Path, args: list[str], *, cargo: bool, expected: str | None = None
) -> int:
    result = subprocess.run(
        ["cargo-kani" if cargo else "kani", *args],
        executable=str(base / "bin/kani-driver"),
        env=environment(base),
        check=False,
        capture_output=True,
        text=True,
    )
    print(result.stdout, end="", flush=True)
    print(result.stderr, end="", file=sys.stderr, flush=True)
    if result.returncode:
        return result.returncode
    if expected is not None and not (
        f"Checking harness {expected}..." in result.stdout
        and re.search(
            r"Complete - 1 successfully verified harnesses, 0 failures, 1 total\.",
            result.stdout,
        )
    ):
        raise RuntimeError(
            f"Verifier did not report the expected successful proof: {expected}"
        )
    return 0


def checker_environment(base: Path) -> dict[str, str]:
    env = environment(base)
    env["PYTHONPATH"] = str(base / "checker")
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    env["UV_COMPILE_BYTECODE"] = "0"
    return env


def prepare_checker(base: Path, cache: Path) -> None:
    python = cache / "checker-env/bin/python"
    if not python.exists():
        subprocess.run(
            ["uv", "venv", "--python", "3.14", str(python.parent.parent)],
            check=True,
            env=checker_environment(base),
        )
    subprocess.run(
        [
            "uv",
            "pip",
            "sync",
            "--python",
            str(python),
            "--only-binary",
            ":all:",
            str(PINS.with_name("prover-constraints.txt")),
        ],
        check=True,
        env=checker_environment(base),
    )


def check_version(base: Path, cache: Path, pins: dict) -> int:
    python = cache / "checker-env/bin/python"
    if not python.is_file():
        raise RuntimeError("Missing prepared version checker; run make install-kani.")
    # Run the pinned Python source directly: no wheel builder, cargo installer,
    # or implicit environment setup is involved in a proof gate.
    command = shlex.join([sys.executable, str(Path(__file__).resolve()), "version"])
    return subprocess.run(
        [
            str(python),
            "-m",
            "rust_prover_tools.cli",
            "kani",
            "check-version",
            "--repo-root",
            str(ROOT),
            "--version-file",
            str(PINS.with_name("VERSION")),
            "--expected-version",
            pins["version"],
            "--kani-command",
            command,
        ],
        env=checker_environment(base),
        check=False,
    ).returncode


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command",
        choices=("install", "version", "check-version", "cargo", "standalone"),
    )
    parser.add_argument("--prover-ref")
    options, args = parser.parse_known_args()
    started = time.monotonic()
    pins, cache = configuration()
    if options.prover_ref and options.prover_ref != pins["checker"]["ref"]:
        raise RuntimeError("Makefile and version-checker source pins disagree.")
    base = installation(pins, cache, install=options.command == "install")
    if options.command == "install":
        prepare_checker(base, cache)
        # A real proof demonstrates the compiler/backend/library combination;
        # its code is separate from the unsupported HashSet lifecycle experiment.
        smoke = cache / "smoke"
        smoke.mkdir(exist_ok=True)
        source = smoke / "install-smoke.rs"
        shutil.copyfile(PINS.with_name("install-smoke.rs"), source)
        result = invoke(
            base,
            [str(source), "--output-format", "terse"],
            cargo=False,
            expected="binary_installation_smoke",
        )
        print(
            f"Binary installation and smoke proof: {time.monotonic() - started:.2f}s",
            flush=True,
        )
        return result
    if options.command == "check-version":
        return check_version(base, cache, pins)
    if options.command == "version":
        return invoke(base, ["--version"], cargo=True)
    return invoke(base, args, cargo=options.command == "cargo")


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (
        OSError,
        RuntimeError,
        subprocess.SubprocessError,
        tarfile.TarError,
    ) as error:
        print(f"Kani binary setup failed: {error}", file=sys.stderr)
        sys.exit(1)
