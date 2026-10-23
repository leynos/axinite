"""The audited crate inventory and the constants the contract checks enforce.

Adding, removing or reclassifying a crate is an explicit decision recorded
here and in `docs/adr-015-audited-unsafe-boundaries.md`.
"""

from __future__ import annotations

import dataclasses
import enum

#: The one reason string the approved bindings module may carry.
EXPECT_REASON = "wit-bindgen component export ABI"


#: The module that contains generated component bindings, per boundary crate.
BINDINGS_MODULE = "bindings"


BINDINGS_FILE = "src/bindings.rs"
BINDINGS_DIRECTORY = "src/bindings/"


#: Directories that never hold first-party sources.
PRUNED_DIRECTORIES: frozenset[str] = frozenset(
    {
        ".git",
        ".cache",
        ".uv-cache",
        ".uv-tools",
        ".venv",
        "node_modules",
        "target",
    }
)


class Role(enum.Enum):
    """What a crate may do with unsafe Rust."""

    SAFE = "safe"
    BOUNDARY = "boundary"


@dataclasses.dataclass(frozen=True, slots=True)
class Crate:
    """One inventoried crate.

    Attributes
    ----------
    path
        Repository-relative directory holding the crate's manifest.
    role
        `SAFE` crates forbid unsafe code; `BOUNDARY` crates deny it and carry
        one reviewed module that may contain it.
    standalone
        Whether the crate is its own Cargo workspace root (excluded from the
        root workspace) rather than a root-workspace member.
    vendored
        Whether the crate is an upstream fork. Vendored crates get the
        manifest lint but keep upstream sources free of local attributes.
    """

    path: str
    role: Role
    standalone: bool = False
    vendored: bool = False


#: Standalone WebAssembly component crates: irreducible ABI boundaries.
WASM_BOUNDARIES: tuple[str, ...] = (
    "channels-src/discord",
    "channels-src/slack",
    "channels-src/telegram",
    "channels-src/whatsapp",
    "tools-src/github",
    "tools-src/gmail",
    "tools-src/google-calendar",
    "tools-src/google-docs",
    "tools-src/google-drive",
    "tools-src/google-sheets",
    "tools-src/google-slides",
    "tools-src/slack",
    "tools-src/telegram",
    "tools-src/web-search",
)


#: Every Cargo crate the repository ships or builds in CI. Adding a crate
#: without an entry here, or removing one, fails the contract: that is the
#: explicit inventory decision ADR 015 requires.
INVENTORY: tuple[Crate, ...] = (
    Crate(".", Role.SAFE),
    Crate("crates/ambient-fs", Role.SAFE),
    Crate("crates/hyper-rustls", Role.SAFE, vendored=True),
    Crate("fuzz", Role.SAFE, standalone=True),
    *(Crate(path, Role.BOUNDARY, standalone=True) for path in WASM_BOUNDARIES),
)


#: Manifests that are deliberately outside the unsafe contract, with the reason.
EXEMPT: dict[str, str] = {
    "tests/workflow_contracts/fixtures/nextest_boundary": (
        "byte-exact nextest fixture compiled only by the workflow contracts"
    ),
    "tools-src/telegram/patches/grammers-crypto": (
        "vendored dependency patch of tools-src/telegram; it declares "
        "`#![deny(unsafe_code)]` upstream and is built inside that boundary "
        "crate's closure"
    ),
}


#: Targets probed by the compiler half. A path is a crate in `INVENTORY`.
PROBED_CRATES: tuple[str, ...] = (
    "crates/ambient-fs",
    "tools-src/web-search",
    "channels-src/discord",
)


#: Deliberate unsafe constructs, each mapped to the phrase rustc uses when the
#: `unsafe_code` lint rejects it. All are appended to a target together, so one
#: compile checks every form, and each phrase must appear in the output.
PROBE_DIAGNOSTICS: dict[str, str] = {
    "pub fn unsafe_probe() { unsafe {} }": "usage of an `unsafe` block",
    "pub unsafe fn unsafe_probe() {}": "declaration of an `unsafe` function",
    "unsafe trait Probe {} unsafe impl Probe for () {}": (
        "implementation of an `unsafe` trait"
    ),
    '#[unsafe(no_mangle)] pub extern "C" fn unsafe_probe() {}': (
        "usage of the unsafe `no_mangle` attribute"
    ),
}
PROBES: tuple[str, ...] = tuple(PROBE_DIAGNOSTICS)

#: Diagnostics proving that the unsafe lint, not some other error, fired.
REJECTION_MARKERS: tuple[str, ...] = (
    "forbid(unsafe_code)",
    "-F unsafe-code",
    "deny(unsafe_code)",
    "-D unsafe-code",
)
