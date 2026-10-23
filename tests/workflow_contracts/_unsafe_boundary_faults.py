"""The deviations the unsafe-boundary contract must report (ADR 015).

Each `Fault` applies one deviation to a disposable copy of the real contract
inputs and names the message that must report it. `mutate` fails closed when
the fragment it replaces stops matching exactly once, so a refactor that moves
the policy cannot silently turn a fault case into a no-op.
"""

from __future__ import annotations

import shutil
import typing as typ
from pathlib import Path

if typ.TYPE_CHECKING:  # pragma: no cover - typing only
    from collections.abc import Callable

WEB_SEARCH = "tools-src/web-search"
DISCORD = "channels-src/discord"
GITHUB = "tools-src/github"


def mutate(root: Path, relative: str, old: str, new: str) -> None:
    """Apply one fault to one file, failing closed if the fragment moved."""
    path = root / relative
    text = path.read_text(encoding="utf-8")
    assert text.count(old) == 1, (
        f"{relative}: fault fragment {old!r} no longer matches exactly once"
    )
    path.write_text(text.replace(old, new), encoding="utf-8")


def append(root: Path, relative: str, text: str) -> None:
    """Append source text to a file, creating it when absent."""
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    existing = path.read_text(encoding="utf-8") if path.exists() else ""
    path.write_text(existing + "\n" + text + "\n", encoding="utf-8")


class Fault(typ.NamedTuple):
    """One deviation and the message that must report it."""

    name: str
    apply: Callable[[Path], None]
    expected: str


def _edit(relative: str, old: str, new: str) -> Callable[[Path], None]:
    """Return a fault that mutates one fragment of one file."""
    return lambda root: mutate(root, relative, old, new)


def _add(relative: str, text: str) -> Callable[[Path], None]:
    """Return a fault that appends source text to a file."""
    return lambda root: append(root, relative, text)


def _remove_dir(relative: str) -> Callable[[Path], None]:
    """Return a fault that deletes a crate directory."""
    return lambda root: shutil.rmtree(root / relative)


FORBID = '[lints.rust]\nunsafe_code = "forbid"'
DENY = '[lints.rust]\nunsafe_code = "deny"'
ROOT_ATTRIBUTE = "#![forbid(unsafe_code)]"
EXPECT = '#[expect(unsafe_code, reason = "wit-bindgen component export ABI")]'

FAULTS: tuple[Fault, ...] = (
    # A safe crate stops forbidding unsafe code.
    Fault(
        "root-forbid-weakened-to-warn",
        _edit("Cargo.toml", FORBID, '[lints.rust]\nunsafe_code = "warn"'),
        "must forbid",
    ),
    Fault(
        "root-forbid-weakened-to-deny", _edit("Cargo.toml", FORBID, DENY), "must forbid"
    ),
    Fault(
        "ambient-fs-forbid-removed",
        _edit("crates/ambient-fs/Cargo.toml", FORBID, ""),
        "must forbid",
    ),
    Fault(
        "hyper-rustls-forbid-removed",
        _edit("crates/hyper-rustls/Cargo.toml", FORBID, ""),
        "must forbid",
    ),
    Fault(
        "fuzz-forbid-weakened",
        _edit("fuzz/Cargo.toml", FORBID, '[lints.rust]\nunsafe_code = "allow"'),
        "must forbid",
    ),
    Fault(
        "root-lib-attribute-removed",
        _edit("src/lib.rs", ROOT_ATTRIBUTE, ""),
        "src/lib.rs lacks",
    ),
    Fault(
        "root-main-attribute-removed",
        _edit("src/main.rs", ROOT_ATTRIBUTE, ""),
        "src/main.rs lacks",
    ),
    Fault(
        "build-script-attribute-removed",
        _edit("build.rs", ROOT_ATTRIBUTE, ""),
        "build.rs lacks",
    ),
    Fault(
        "ambient-fs-attribute-removed",
        _edit("crates/ambient-fs/src/lib.rs", ROOT_ATTRIBUTE, ""),
        "src/lib.rs lacks",
    ),
    Fault(
        "attribute-only-in-a-comment",
        _edit(
            "crates/ambient-fs/src/lib.rs", ROOT_ATTRIBUTE, "// #![forbid(unsafe_code)]"
        ),
        "src/lib.rs lacks",
    ),
    Fault(
        "lint-inheritance-mixed-with-overrides",
        _edit(
            "crates/ambient-fs/Cargo.toml",
            "[lints.rust]",
            "[lints]\nworkspace = true\n\n[lints.rust]",
        ),
        "mixed with local lint overrides",
    ),
    Fault(
        "lint-inherited-without-a-workspace-forbid",
        _edit(
            "crates/ambient-fs/Cargo.toml",
            FORBID,
            "[lints]\nworkspace = true",
        ),
        "must forbid",
    ),
    # A boundary crate stops denying, or inherits instead of declaring.
    Fault(
        "boundary-forbid-instead-of-deny",
        _edit(f"{WEB_SEARCH}/Cargo.toml", DENY, FORBID),
        "must deny",
    ),
    Fault(
        "boundary-allow-instead-of-deny",
        _edit(f"{DISCORD}/Cargo.toml", DENY, '[lints.rust]\nunsafe_code = "allow"'),
        "must deny",
    ),
    Fault(
        "boundary-inherits-workspace-lints",
        _edit(f"{GITHUB}/Cargo.toml", DENY, "[lints]\nworkspace = true"),
        "must deny",
    ),
    # Unsafe code outside the bindings module.
    Fault(
        "unsafe-block-in-lib",
        _add(f"{WEB_SEARCH}/src/lib.rs", "fn probe() { unsafe {} }"),
        "`unsafe` outside",
    ),
    Fault(
        "unsafe-fn-in-guest-module",
        _add(f"{GITHUB}/src/schema.rs", "pub unsafe fn probe() {}"),
        "`unsafe` outside",
    ),
    Fault(
        "unsafe-in-integration-test",
        _add(f"{WEB_SEARCH}/tests/probe.rs", "fn probe() { unsafe {} }"),
        "tests/probe.rs: `unsafe` outside",
    ),
    Fault(
        "unsafe-impl",
        _add(f"{DISCORD}/src/lib.rs", "unsafe impl Send for Probe {}"),
        "`unsafe` outside",
    ),
    Fault(
        "no-mangle-outside-bindings",
        _add(
            f"{WEB_SEARCH}/src/lib.rs", '#[unsafe(no_mangle)] extern "C" fn probe() {}'
        ),
        "ABI attribute",
    ),
    Fault(
        "export-name-outside-bindings",
        _add(
            f"{WEB_SEARCH}/src/lib.rs",
            '#[export_name = "probe"] extern "C" fn probe() {}',
        ),
        "ABI attribute",
    ),
    # The expect is removed, reworded, duplicated or accompanied by an allow.
    Fault(
        "expect-declaration-removed",
        _edit(f"{WEB_SEARCH}/src/lib.rs", EXPECT + "\n", ""),
        "must declare the bindings module",
    ),
    Fault(
        "expect-reason-reworded",
        _edit(f"{WEB_SEARCH}/src/lib.rs", "wit-bindgen component export ABI", "needed"),
        "must declare the bindings module",
    ),
    Fault(
        "second-expect-added",
        _add(
            f"{WEB_SEARCH}/src/lib.rs",
            '#[expect(unsafe_code, reason = "x")] mod extra {}',
        ),
        "lint override",
    ),
    Fault(
        "allow-added",
        _add(f"{DISCORD}/src/lib.rs", "#[allow(unsafe_code)] mod extra {}"),
        "lint override",
    ),
    Fault(
        "cfg-attr-allow-added",
        _add(
            f"{DISCORD}/src/lib.rs",
            "#[cfg_attr(test, allow(unsafe_code))] mod extra {}",
        ),
        "lint override",
    ),
    Fault(
        "crate-level-allow-added",
        _add(f"{DISCORD}/src/lib.rs", "#![allow(unsafe_code)]"),
        "lint override",
    ),
    # The bindings module holds more than generated bindings.
    Fault(
        "handwritten-function-in-bindings",
        _add(f"{WEB_SEARCH}/src/bindings.rs", "pub fn helper() {}"),
        "handwritten content",
    ),
    Fault(
        "handwritten-unsafe-in-bindings",
        _add(f"{WEB_SEARCH}/src/bindings.rs", "pub unsafe fn helper() {}"),
        "handwritten",
    ),
    Fault(
        "export-names-a-different-type",
        _edit(
            f"{WEB_SEARCH}/src/bindings.rs",
            "export!(WebSearchTool);",
            "export!(Other);",
        ),
        "export! must name",
    ),
    Fault(
        "export-missing-from-bindings",
        _edit(f"{WEB_SEARCH}/src/bindings.rs", "export!(WebSearchTool);", ""),
        "exactly one",
    ),
    # The Miri ABI test module is the only other place `unsafe` may appear.
    Fault(
        "undeclared-module-under-bindings",
        _add(f"{WEB_SEARCH}/src/bindings/extra.rs", "pub fn probe() { unsafe {} }"),
        "not declared as a 32-bit test module",
    ),
    Fault(
        "abi-test-module-loses-cfg-test",
        _edit(
            f"{WEB_SEARCH}/src/bindings.rs",
            '#[cfg(all(test, target_pointer_width = "32"))]\nmod abi_round_trip;',
            "mod abi_round_trip;",
        ),
        "not declared as a 32-bit test module",
    ),
    Fault(
        "abi-test-module-exports-a-symbol",
        _add(
            f"{WEB_SEARCH}/src/bindings/abi_round_trip.rs",
            '#[unsafe(no_mangle)] extern "C" fn probe() {}',
        ),
        "ABI attribute in a test module",
    ),
    Fault(
        "abi-test-module-relaxes-the-lint",
        _add(f"{WEB_SEARCH}/src/bindings/abi_round_trip.rs", "#![allow(unsafe_code)]"),
        "unsafe_code lint override",
    ),
    # `make miri` must interpret every component crate that has host tests.
    Fault(
        "miri-root-dropped-from-the-makefile",
        _edit(
            "Makefile",
            "channels-src/discord channels-src/slack",
            "channels-src/slack",
        ),
        "has host tests but is missing",
    ),
    Fault(
        "miri-root-is-not-a-boundary-crate",
        _edit(
            "Makefile",
            "tools-src/telegram tools-src/web-search",
            "tools-src/telegram tools-src/web-search crates/ambient-fs",
        ),
        "is not a boundary crate",
    ),
    Fault(
        "miri-root-has-no-tests",
        _edit(
            "Makefile",
            "tools-src/telegram tools-src/web-search",
            "tools-src/gmail tools-src/telegram tools-src/web-search",
        ),
        "has no host tests",
    ),
    Fault(
        "abi-root-dropped-from-the-makefile",
        _edit(
            "Makefile",
            "MIRI_ABI_ROOTS := channels-src/discord tools-src/web-search",
            "MIRI_ABI_ROOTS := channels-src/discord",
        ),
        "missing from Makefile MIRI_ABI_ROOTS",
    ),
    Fault(
        "abi-root-without-a-round-trip-module",
        _edit(
            "Makefile",
            "MIRI_ABI_ROOTS := channels-src/discord tools-src/web-search",
            "MIRI_ABI_ROOTS := channels-src/discord tools-src/web-search "
            "tools-src/github",
        ),
        "declares no ABI round trips",
    ),
    # Membership: a crate appears, disappears or changes shape without a decision.
    Fault(
        "unlisted-crate-added",
        _add("crates/new-crate/Cargo.toml", '[package]\nname = "new-crate"'),
        "no unsafe-boundary inventory decision",
    ),
    Fault(
        "unlisted-wasm-component-added",
        _add(
            "tools-src/new-tool/Cargo.toml", '[package]\nname = "new-tool"\n[workspace]'
        ),
        "no unsafe-boundary inventory decision",
    ),
    Fault(
        "unlisted-workspace-member",
        _edit(
            "Cargo.toml",
            'members = [".", "crates/ambient-fs", "crates/hyper-rustls"]',
            'members = [".", "crates/ambient-fs", "crates/hyper-rustls", '
            '"crates/new-crate"]',
        ),
        "is not an inventoried root-workspace crate",
    ),
    Fault(
        "inventoried-member-dropped",
        _edit("Cargo.toml", '"crates/ambient-fs", ', ""),
        "inventoried crate 'crates/ambient-fs' is missing",
    ),
    Fault(
        "unlisted-workspace-exclusion",
        _edit(
            "Cargo.toml", '    "fuzz",\n', '    "fuzz",\n    "tools-src/new-tool",\n'
        ),
        "is not an inventoried standalone root",
    ),
    Fault(
        "inventoried-crate-deleted",
        _remove_dir("tools-src/gmail"),
        "inventoried crate has no manifest",
    ),
    Fault(
        "standalone-root-loses-its-workspace",
        _edit(f"{DISCORD}/Cargo.toml", "[workspace]", ""),
        "declares no [workspace] table",
    ),
)
