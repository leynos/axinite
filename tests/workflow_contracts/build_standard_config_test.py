"""Contracts for the Cargo configuration half of the development build standard.

`.cargo/config.toml` is what a bare ``cargo build`` reads, so it is where the
standard is observable without Make. These contracts hold the config and the
toolchain pin to the estate's `rust-build-defaults` clauses: the parallel
frontend in every `rustflags` source, the Linux-only linker flag, no Cranelift
selection while the guide records the exception, and the components the pinned
toolchain must install.

Each judge in `_build_standard` is also driven with a representative mutation,
and must reject it: a contract that cannot fail is not a contract.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import collections.abc as cabc
import copy
import typing as typ

import pytest

from _build_standard import (
    MOLD_FLAG,
    PLATFORMS,
    THREADS_FLAG,
    config_violations,
    effective_rustflags,
    read_toml,
    rustflag_sources,
)

CONFIG = ".cargo/config.toml"
TOOLCHAIN = "rust-toolchain.toml"


@pytest.fixture(scope="module")
def config() -> dict[str, typ.Any]:
    """Return the parsed Cargo configuration."""
    return read_toml(CONFIG)


@pytest.fixture(scope="module")
def toolchain() -> dict[str, typ.Any]:
    """Return the parsed toolchain pin."""
    return read_toml(TOOLCHAIN)


def test_the_repository_conforms(
    config: dict[str, typ.Any], toolchain: dict[str, typ.Any]
) -> None:
    """The committed configuration and pin raise no violation."""
    assert config_violations(config, toolchain) == []


def test_the_rustflags_sources_were_found(config: dict[str, typ.Any]) -> None:
    """The judge is not vacuous: a build source and a Linux table exist."""
    sources = rustflag_sources(config)
    assert "build" in sources, (
        "the [build] rustflags source is the default for every platform"
    )
    assert any(name.startswith("target.") for name in sources), (
        "a Linux target source is required"
    )


@pytest.mark.parametrize("triple", sorted(PLATFORMS), ids=sorted(PLATFORMS))
def test_every_platform_gets_the_frontend_and_only_linux_gets_mold(
    config: dict[str, typ.Any], triple: str
) -> None:
    """Cargo's single-source selection leaves no platform without the frontend."""
    flags = effective_rustflags(config, triple)
    assert THREADS_FLAG in flags, f"{triple} lost {THREADS_FLAG}: {flags}"
    assert (MOLD_FLAG in flags) == (PLATFORMS[triple] == "linux"), (
        f"{triple}: the ELF linker flag must reach Linux and nothing else: {flags}"
    )


def test_the_environment_block_is_preserved(config: dict[str, typ.Any]) -> None:
    """The embedded-PostgreSQL settings the tests read survive the standard."""
    environment = config.get("env", {})
    for name in ("PG_VERSION_REQ", "PG_EXTENSIONS", "PG_EMBED_ROOT"):
        assert name in environment, f"[env] lost {name}"


def test_linux_triple_tables_select_clang(config: dict[str, typ.Any]) -> None:
    """Both supported Linux host architectures use the compiler-driver linker."""
    target_tables = config["target"]
    for triple in ("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"):
        table = target_tables[triple]
        assert "rustflags" not in table, (
            f"keep {triple} flags in the cfg(target_os) table only"
        )
        assert table.get("linker") == "clang", (
            f"-fuse-ld=mold is a compiler-driver flag for {triple}"
        )


Mutation = cabc.Callable[[dict[str, typ.Any], dict[str, typ.Any]], None]
LINUX = 'cfg(target_os = "linux")'


def _remove(items: list[str], item: str) -> None:
    """Remove ``item`` if present, so a mutated file still yields a verdict."""
    if item in items:
        items.remove(item)


MUTATIONS: dict[str, Mutation] = {
    "frontend-removed-from-linux": lambda c, t: c["target"][LINUX].update(
        rustflags=[MOLD_FLAG]
    ),
    "frontend-removed-from-build": lambda c, t: c["build"].update(rustflags=[]),
    "mold-removed": lambda c, t: c["target"][LINUX].update(rustflags=[THREADS_FLAG]),
    "mold-leaks-to-build": lambda c, t: c["build"].update(
        rustflags=[THREADS_FLAG, MOLD_FLAG]
    ),
    "triple-table-splits-sources": lambda c, t: c["target"][
        "x86_64-unknown-linux-gnu"
    ].update(rustflags=[MOLD_FLAG]),
    "cranelift-selected-for-dev": lambda c, t: (
        c.setdefault("profile", {})
        .setdefault("dev", {})
        .update({"codegen-backend": "cranelift"})
    ),
    "cranelift-selected-for-release": lambda c, t: (
        c.setdefault("profile", {})
        .setdefault("release", {})
        .update({"codegen-backend": "cranelift"})
    ),
    "cranelift-through-rustflags": lambda c, t: c["build"].update(
        rustflags=[THREADS_FLAG, "-Zcodegen-backend=cranelift"]
    ),
    "llvm-tools-dropped": lambda c, t: _remove(
        t["toolchain"]["components"], "llvm-tools-preview"
    ),
    "channel-floats": lambda c, t: t["toolchain"].update(channel="nightly"),
    "channel-stable": lambda c, t: t["toolchain"].update(channel="stable"),
    "wasm-target-dropped": lambda c, t: t["toolchain"].update(targets=[]),
}
MUTATION_NAMES = sorted(MUTATIONS)


@pytest.mark.parametrize("name", MUTATION_NAMES, ids=MUTATION_NAMES)
def test_each_mutation_is_detected(
    name: str, config: dict[str, typ.Any], toolchain: dict[str, typ.Any]
) -> None:
    """Breaking the standard one way at a time must be reported."""
    mutated_config, mutated_toolchain = copy.deepcopy(config), copy.deepcopy(toolchain)
    MUTATIONS[name](mutated_config, mutated_toolchain)
    assert config_violations(mutated_config, mutated_toolchain), (
        f"the mutation {name!r} passed the configuration contract"
    )


def test_the_mutations_executed() -> None:
    """Prove the parametrized cases exist, so none can be discovered away."""
    assert len(MUTATION_NAMES) == 12
