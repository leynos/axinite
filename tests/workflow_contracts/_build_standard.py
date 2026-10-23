"""Judging the development build standard from the files that carry it.

The standard is spread over four places that must agree: the Cargo
configuration (`.cargo/config.toml`), the toolchain pin (`rust-toolchain.toml`),
the Makefile recipes that invoke Cargo (`_build_standard_make`), and the
workflows that provision the tools and, for coverage and release, opt out of it
(`_build_standard_workflows`). Each judge is a pure function of text or parsed
data that returns a list of violations, and the test modules do two things with
it: assert the repository has none, and apply a representative mutation to a
copy and assert the judge reports it. A judge that cannot fail is not a
contract, so every judge has a mutation that fails it.

This module holds the shared constants, the TOML readers and the judge for the
configuration and the toolchain pin.

Run via ``make test-workflow-contracts``.
"""

from __future__ import annotations

import re
import tomllib
import typing as typ
from pathlib import Path

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Iterable, Mapping

REPOSITORY_ROOT = Path(__file__).resolve().parents[2]

#: The parallel frontend and the Linux-only linker flag, as the standard
#: spells them after `normalize`.
THREADS_FLAG = "-Zthreads=8"
MOLD_FLAG = "-Clink-arg=-fuse-ld=mold"
CRANELIFT_FLAG = "-Zcodegen-backend=cranelift"
FAST_FLAGS = (THREADS_FLAG, MOLD_FLAG)

#: Triples the effective-flag model is evaluated for: the Linux hosts the
#: standard is for, the platforms it must not leak ELF flags into, and the
#: WebAssembly target the Makefile builds for.
PLATFORMS: dict[str, str] = {
    "x86_64-unknown-linux-gnu": "linux",
    "aarch64-unknown-linux-gnu": "linux",
    "aarch64-apple-darwin": "macos",
    "x86_64-pc-windows-msvc": "windows",
    "wasm32-wasip2": "wasi",
}

#: Components the pinned toolchain must supply for the standard to work.
REQUIRED_COMPONENTS = (
    "rustfmt",
    "clippy",
    "rust-analyzer",
    "llvm-tools-preview",
)
DATED_NIGHTLY = re.compile(r"^nightly-\d{4}-\d{2}-\d{2}$")


def read_toml(relative: str, *, root: Path = REPOSITORY_ROOT) -> dict[str, typ.Any]:
    """Return one repository TOML file, parsed.

    Parameters
    ----------
    relative
        Repository-relative path.
    root
        Repository root, overridable for a mutated copy.

    Returns
    -------
    dict
        The parsed document.
    """
    return tomllib.loads((root / relative).read_text(encoding="utf-8"))


def normalize(flags: Iterable[str]) -> list[str]:
    """Return rustflags as single tokens, joining ``-C x`` and ``-Z x``.

    Cargo reads ``-C`` and its value as one token or two; the standard is
    written both ways in the estate, so they are compared normalized.

    Parameters
    ----------
    flags
        The tokens of one ``rustflags`` source, or a split ``RUSTFLAGS``.

    Returns
    -------
    list of str
        Tokens with a bare ``-C``/``-Z`` merged into its value.
    """
    tokens = [token for token in flags if token]
    merged: list[str] = []
    index = 0
    while index < len(tokens):
        token = tokens[index]
        if token in ("-C", "-Z") and index + 1 < len(tokens):
            merged.append(token + tokens[index + 1])
            index += 2
            continue
        merged.append(token)
        index += 1
    return merged


def split_flags(text: str) -> list[str]:
    """Return a ``RUSTFLAGS`` string as normalized tokens."""
    return normalize(text.split())


def table_flags(value: object) -> list[str]:
    """Return one ``rustflags`` value, an array or a string, as tokens."""
    if isinstance(value, str):
        return split_flags(value)
    if isinstance(value, list):
        return normalize(str(item) for item in value)
    return []


def os_of_key(key: str) -> str | None:
    """Return the operating system a ``[target.<key>]`` table applies on.

    Only the shapes the standard uses are placed; anything else is ``None``,
    which the judges treat as a violation rather than a guess.
    """
    if key == 'cfg(target_os = "linux")':
        return "linux"
    if key in PLATFORMS:
        return PLATFORMS[key]
    return None


def rustflag_sources(config: Mapping[str, typ.Any]) -> dict[str, list[str]]:
    """Return every ``rustflags`` source in a Cargo configuration.

    Parameters
    ----------
    config
        The parsed ``.cargo/config.toml``.

    Returns
    -------
    dict
        ``build`` and ``target.<key>`` to that source's normalized tokens.
    """
    sources: dict[str, list[str]] = {}
    build = config.get("build", {})
    if isinstance(build, dict) and "rustflags" in build:
        sources["build"] = table_flags(build["rustflags"])
    targets = config.get("target", {})
    if isinstance(targets, dict):
        for key, table in targets.items():
            if isinstance(table, dict) and "rustflags" in table:
                sources[f"target.{key}"] = table_flags(table["rustflags"])
    return sources


def effective_rustflags(
    config: Mapping[str, typ.Any], triple: str, assigned: str | None = None
) -> list[str]:
    """Return the rustflags Cargo would use for ``triple``.

    An assigned ``RUSTFLAGS`` wins outright. Otherwise every matching
    ``[target.*]`` table is joined, and only when none matches does
    ``[build] rustflags`` apply: Cargo selects one source, it does not merge
    them.

    Parameters
    ----------
    config
        The parsed ``.cargo/config.toml``.
    triple
        A key of `PLATFORMS`.
    assigned
        The value of an assigned ``RUSTFLAGS``, or ``None`` when unset.

    Returns
    -------
    list of str
        The tokens the compiler would receive from this mechanism.
    """
    if assigned is not None:
        return split_flags(assigned)
    platform = PLATFORMS[triple]
    sources = rustflag_sources(config)
    matched: list[str] = []
    found = False
    for name, flags in sources.items():
        if not name.startswith("target."):
            continue
        key = name.removeprefix("target.")
        if key == triple or (os_of_key(key) == platform and key.startswith("cfg(")):
            matched.extend(flags)
            found = True
    if found:
        return matched
    return list(sources.get("build", []))


def config_violations(
    config: Mapping[str, typ.Any], toolchain: Mapping[str, typ.Any]
) -> list[str]:
    """Judge `.cargo/config.toml` against the build standard.

    Parameters
    ----------
    config
        The parsed Cargo configuration.
    toolchain
        The parsed ``rust-toolchain.toml``.

    Returns
    -------
    list of str
        One message per violation; empty when the configuration conforms.
    """
    problems: list[str] = []
    sources = rustflag_sources(config)
    if len(sources) < 2:
        problems.append(
            f"expected a [build] and a Linux rustflags source, found {sorted(sources)}"
        )
    for name, flags in sources.items():
        if THREADS_FLAG not in flags:
            problems.append(
                f"{name} lacks {THREADS_FLAG}; Cargo selects one source, not a merge"
            )
        key = name.removeprefix("target.")
        is_linux = name.startswith("target.") and os_of_key(key) == "linux"
        if is_linux and MOLD_FLAG not in flags:
            problems.append(f"{name} applies on Linux but lacks {MOLD_FLAG}")
        if not is_linux and MOLD_FLAG in flags:
            problems.append(f"{name} names the Linux-only {MOLD_FLAG}")
        if name.startswith("target.") and os_of_key(key) is None:
            problems.append(f"{name} is a target key these contracts cannot place")
        if CRANELIFT_FLAG in flags:
            problems.append(
                f"{name} selects Cranelift, which this repository refuses on "
                "recorded evidence (docs/developers-guide.md, Cranelift)"
            )
    for triple, platform in PLATFORMS.items():
        flags = effective_rustflags(config, triple)
        if THREADS_FLAG not in flags:
            problems.append(
                f"{triple}: effective rustflags {flags} lack {THREADS_FLAG}"
            )
        if (MOLD_FLAG in flags) != (platform == "linux"):
            problems.append(
                f"{triple}: mold must be present exactly on Linux, got {flags}"
            )
    # The recorded Cranelift exception and the configuration must agree: while
    # the guide refuses the backend, no profile may select it. This is the
    # repository's own state, not a prohibition on codegen-backend keys: a
    # profile naming LLVM, or a future measured adoption that removes the
    # exception, passes through the same judge by editing both together.
    profiles = config.get("profile", {})
    for name, table in profiles.items() if isinstance(profiles, dict) else []:
        selected = table.get("codegen-backend") if isinstance(table, dict) else None
        if selected == "cranelift":
            problems.append(
                f"profile.{name} selects Cranelift, which this repository "
                "refuses on recorded evidence (docs/developers-guide.md, Cranelift)"
            )
    channel = str(toolchain.get("toolchain", {}).get("channel", ""))
    if not DATED_NIGHTLY.match(channel):
        problems.append(
            f"the parallel frontend needs a dated nightly pin, found {channel!r}"
        )
    problems.extend(toolchain_violations(toolchain))
    return problems


def toolchain_violations(toolchain: Mapping[str, typ.Any]) -> list[str]:
    """Judge the components and targets `rust-toolchain.toml` must carry."""
    table = toolchain.get("toolchain", {})
    components = set(table.get("components", []))
    problems = [
        f"rust-toolchain.toml lacks the {name} component"
        for name in REQUIRED_COMPONENTS
        if name not in components
    ]
    if "wasm32-wasip2" not in table.get("targets", []):
        problems.append("rust-toolchain.toml lacks the wasm32-wasip2 target")
    return problems
