# Architectural decision record (ADR) 0015: Audited unsafe boundaries

## Status

Accepted. Every library, test, helper and binary target in the root workspace
forbids unsafe Rust. The only unsafe code in the repository is the generated
WebAssembly component binding glue, confined to one `bindings` module in each
of fourteen standalone component crates that deny, rather than forbid, unsafe
code. A repository contract enforces the allow-list, and an isolated Miri
workflow interprets the host-runnable paths around those boundaries.

## Date

2026-10-23.

## Context and problem statement

The estate lint policy is `unsafe_code = "forbid"`. Measuring the repository
against that policy found 83 unsafe sites, classified in the table below. This
ADR records how Axinite reaches a state where `forbid` holds wherever it can,
and where it cannot, how the exception is kept small, named and enforced.

| Class | Sites | Origin                                                         | Disposition                                      |
| ----- | ----- | -------------------------------------------------------------- | ------------------------------------------------ |
| A     | 30    | `wit_bindgen::generate!`/`export!` expansions in 14 WASM roots | Irreducible; confined to a `bindings` module     |
| B     | 44    | Test-only `set_var`/`remove_var`                               | Removed by the environment-injection work (#333) |
| C     | 1     | Production `set_var("DATABASE_BACKEND")`                       | Removed; the default moves into the `EnvContext` |
| D     | 1     | `SecretValue` zeroing in `Drop`                                | Removed; `zeroize` derive replaces it            |
| E     | 1     | `libc::geteuid`                                                | Removed; `rustix::process::geteuid`              |
| F     | 3     | `openat2` syscall wrapper (`zeroed`, `syscall`, `from_raw_fd`) | Removed; `rustix::fs::openat2`                   |
| G     | 3     | Environment mutation in `testing::test_utils`                  | Removed by the environment-injection work (#333) |

_Table 1: The 83 unsafe sites found under `unsafe_code = "forbid"`, by class._

Classes B and G were already gone on the base of this change (pull request
number 364, "Eliminate in-process environment mutation from tests"). A scan of
the base found only class A and the C, D, E and F sites in the root crate;
`src/tools/builder/templates/sources.rs` contains the word `unsafe` only inside
string literals that are templates for generated tools, which is not Rust
source the compiler sees.

The reference design is cuprum's ADR 011 (commit `33bff5db`), which split a
crate into a safe policy crate that forbids unsafe code, a small native crate
that owns every raw-resource obligation, and a thin foreign-function interface
(FFI) integration crate whose generated ABI glue is an audited trust boundary.
It enforces the split with an allow-list script and compiler probes, and
verifies the boundary with Miri where Miri can run. Axinite adopts the same
shape, scaled to what Axinite actually needs.

## Decision drivers

- `forbid` cannot be lowered or overridden by an inner `allow` or `expect`
  (compiler error E0453), so a crate either forbids or hosts reasoned unsafe
  code; it cannot do both.
- The WebAssembly component export ABI has no safe formulation: the host calls
  the exported symbols directly.
- The smallest in-repository unsafe surface is the easiest to audit.
- Behaviour must not change. In particular `skill_read_file` confines reads
  with `openat2` and `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`, and callers rely
  on its error mapping.
- The property must hold before the final lint-configuration layer lands the
  workspace lint table, and survive it.

## Requirements

### Functional requirements

- No handwritten unsafe code in the root workspace.
- Adding a crate, or changing a crate's unsafe role, is an explicit decision
  that a test enforces.
- Unsafe code outside the approved bindings modules fails the build and fails
  the contract.

### Technical requirements

- Safe crates forbid `unsafe_code` for every Cargo target, including targets
  Cargo discovers in future.
- Boundary crates declare their own complete lint table.
- The compiler, not only a text scan, proves rejection.

## Options considered

### Routes for classes E and F (`geteuid` and `openat2`)

#### Option 1: a safe dependency, `rustix`

`rustix` is already in `Cargo.lock` (1.1.4, through `cap-std`, `fs4` and
others), implements `fs::openat2` returning an `OwnedFd`, and implements
`process::geteuid`. On Linux it issues syscalls directly through its
`linux_raw` backend, so it adds no libc call to audit. The repository then
carries no in-tree unsafe for either operation.

#### Option 2: an in-tree boundary crate, `crates/axinite-native-io`

Modelled on `cuprum-native-io`: unsafe APIs with documented caller obligations,
`#![deny(unsafe_op_in_unsafe_fn)]`, `SAFETY` comments, and ownership tests over
real descriptors. It would hold the same three unsafe sites as today (zeroed
`open_how`, `syscall`, `from_raw_fd`) plus `geteuid`, add a crate, and need its
own audit.

| Topic                               | Option 1: `rustix`                  | Option 2: boundary crate              |
| ----------------------------------- | ----------------------------------- | ------------------------------------- |
| In-tree unsafe sites                | 0                                   | 4 plus tests that need descriptors    |
| New crates                          | 0                                   | 1, plus an inventory entry            |
| `RESOLVE_*` flags and error mapping | Identical (named flags, same errno) | Identical (raw constants)             |
| Descriptor ownership                | `OwnedFd` from a safe API           | `from_raw_fd` under a caller contract |
| Miri can execute the syscall        | No                                  | No                                    |
| Trust placed outside the repository | `rustix`, widely used, in the lock  | `libc`, already trusted               |

_Table 2: Comparison of the two routes._

`crates/ambient-fs` is not the home for either route. It confines ambient
`std::fs` access for the Whitaker `no_std_fs_operations` lint and contains no
unsafe code; `rustix::fs::openat2` is not `std::fs`, so the call site needs no
exclusion in `dylint.toml`. A future boundary crate would sit beside it as its
own inventory entry rather than inside it, so that the safe crate keeps
forbidding unsafe code in every target.

### Layout for the fourteen WebAssembly component crates (class A)

#### Option A: a `bindings` module in each crate

Move `generate!` and `export!` into `src/bindings.rs`. The crate sets
`unsafe_code = "deny"` in its own lint table, and `lib.rs` declares the module
behind one
`#[expect(unsafe_code, reason = "wit-bindgen component export ABI")]`. All
handwritten logic lives in other modules and is unsafe-free.

#### Option B: a safe guest-logic crate plus a thin bindings crate per component

Fourteen extra crates, each pair joined by a trait boundary. The export symbols
must still live in the `cdylib` that is the component, so the logic crate would
need the bindings crate's generated traits as a dependency and the cdylib would
remain the boundary regardless. The split adds 14 manifests, 14 lock files and
14 inventory entries and moves no unsafe code out of the component.

#### Option C: lower to `deny` and scatter `expect` attributes

Nothing ties an `expect` to the generated code, and an inner `allow` is
undetectable without a text scan.

#### Option D: exempt the component crates from the policy

Leaves them with no rule at all, so handwritten unsafe code could appear
anywhere in them.

### Classes C and D

For D (`SecretValue` zeroing), `secrecy::SecretString` is not `Clone` without a
`CloneableSecret` wrapper and hides the `&str` that the credential injector
needs, whereas `zeroize`'s derive keeps `Clone`, `as_str` and a redacted
`Debug` and replaces the manual `Drop` with volatile writes. For C
(`set_var("DATABASE_BACKEND")`), a scoped-environment crate such as `temp-env`
would only move the unsafe call out of the tree; injecting the default into the
`EnvContext` snapshot removes the mutation.

## Decision outcome

### Policy

- Every target of every root-workspace crate, and of the standalone `fuzz`
  crate, forbids `unsafe_code`. The crate carries
  `[lints.rust] unsafe_code = "forbid"` in its manifest, which Cargo applies to
  library, binary, build script, test, benchmark and example targets alike,
  including targets added later. First-party library, binary and build-script
  roots also carry `#![forbid(unsafe_code)]`, so the prohibition is visible
  beside the code.
- The fourteen component crates are the only boundary crates, and each is
  confined as in Option A.
- Classes C, D, E and F are removed at source, E and F by Option 1. If a future
  need arises for a syscall `rustix` does not wrap, a dedicated boundary crate
  in the style of `cuprum-native-io` is the sanctioned route: a new inventory
  entry with the role `boundary`, a `deny` table, unsafe functions with
  documented caller obligations, `#![deny(unsafe_op_in_unsafe_fn)]`, and Miri
  tests over real resources.

### How the policy composes with the final lint configuration

The final lint-configuration layer will add a `[workspace.lints.rust]` table
with `unsafe_code = "forbid"`. At that point each safe crate replaces its
`[lints.rust]` table with `[lints] workspace = true`; the contract resolves
either form. Cargo rejects `[lints] workspace = true` next to local lint keys,
so a crate that needs a different level cannot inherit and then override one
lint. Therefore:

- A boundary crate never uses `[lints] workspace = true`. It declares its own
  complete lint table, which is the workspace table with `unsafe_code` lowered
  to `deny`. Today the component crates have no other lints, so their table is
  one line. When the lint layer lands, the boundary tables must be generated
  from the workspace table plus that single difference, and the contract
  continues to require `deny` and to reject `workspace = true`.
- The contract's scratch workspace copies the root `[workspace.lints*]`
  tables, so a member that inherits them is probed under them.

### The allow-list

| Crate path                  | Workspace  | Role     | Unsafe permitted in     |
| --------------------------- | ---------- | -------- | ----------------------- |
| `.` (`axinite`)             | root       | safe     | nowhere                 |
| `crates/ambient-fs`         | root       | safe     | nowhere                 |
| `crates/hyper-rustls`       | root       | safe     | nowhere (vendored fork) |
| `fuzz`                      | standalone | safe     | nowhere                 |
| `channels-src/discord`      | standalone | boundary | `src/bindings.rs`       |
| `channels-src/slack`        | standalone | boundary | `src/bindings.rs`       |
| `channels-src/telegram`     | standalone | boundary | `src/bindings.rs`       |
| `channels-src/whatsapp`     | standalone | boundary | `src/bindings.rs`       |
| `tools-src/github`          | standalone | boundary | `src/bindings.rs`       |
| `tools-src/gmail`           | standalone | boundary | `src/bindings.rs`       |
| `tools-src/google-calendar` | standalone | boundary | `src/bindings.rs`       |
| `tools-src/google-docs`     | standalone | boundary | `src/bindings.rs`       |
| `tools-src/google-drive`    | standalone | boundary | `src/bindings.rs`       |
| `tools-src/google-sheets`   | standalone | boundary | `src/bindings.rs`       |
| `tools-src/google-slides`   | standalone | boundary | `src/bindings.rs`       |
| `tools-src/slack`           | standalone | boundary | `src/bindings.rs`       |
| `tools-src/telegram`        | standalone | boundary | `src/bindings.rs`       |
| `tools-src/web-search`      | standalone | boundary | `src/bindings.rs`       |

_Table 3: The unsafe boundary allow-list, enforced by
`scripts/check_unsafe_boundaries.py`._

Two manifests are exempt, with their reasons in the script:
`tests/workflow_contracts/fixtures/nextest_boundary` (a byte-exact fixture only
the workflow contracts compile) and
`tools-src/telegram/patches/grammers-crypto` (a vendored dependency patch that
already declares `#![deny(unsafe_code)]` and is built inside a boundary crate's
closure).

### The component boundary module

Each component crate has this shape:

```rust
// lib.rs
#[expect(unsafe_code, reason = "wit-bindgen component export ABI")]
mod bindings;

use bindings::{exports, near};
```

```rust
// bindings.rs
use super::WebSearchTool;

wit_bindgen::generate!({ world: "sandboxed-tool", path: "../../wit/tool.wit" });

export!(WebSearchTool);
```

The `use bindings::{exports, near}` re-export keeps every existing path in the
guest code (`exports::near::agent::tool::Guest`, `crate::near::agent::host`)
unchanged. The `expect` fails loudly if the generator ever stops emitting
unsafe code, so the approval cannot outlive its need. `export!` stays in the
bindings module because the macro that `generate!` defines is scoped there.

The contract requires the bindings file to contain only the `use super::Type;`,
`generate!` and `export!` statements and for `export!` to name the imported
type. It rejects `unsafe`, `no_mangle`, `export_name`, `link_section` and any
`unsafe_code` lint attribute in every other source of the crate.

### The contract

`scripts/check_unsafe_boundaries.py` has two halves, both run by
`make boundary-contract`, and the first also by `make test-workflow-contracts`.

- Static: every `Cargo.toml` in the repository is inventoried or exempt; the
  root workspace's `members` and `exclude` match the inventory; each safe crate
  forbids unsafe code in its resolved lint table, and each boundary crate
  denies it in a table of its own; no `unsafe`, ABI attribute or lint override
  appears outside the bindings module; the bindings module holds only generated
  bindings.
- Compiler: `crates/ambient-fs`, `tools-src/web-search` and
  `channels-src/discord` are copied to a scratch workspace with a synthesized
  `tests/future_target.rs`. Four unsafe forms (an unsafe block, an unsafe
  function, an unsafe trait with an implementation, and `#[unsafe(no_mangle)]`)
  are appended to each target together, and `cargo check --all-targets` must
  reject the target and name every form. A control run with the policy stripped
  must accept the probe, so a probe that cannot fail is itself a failure.

Fault sensitivity is covered by `unsafe_boundary_test.py` in
`tests/workflow_contracts`, which applies forty-odd deviations to disposable
copies of the real inputs (removing a `forbid`, weakening it, adding unsafe
code to a guest module, adding an unlisted crate, and so on) and requires each
to be reported.

### Verification

Miri is the verifier for this layer. `make miri` runs a pinned dated nightly
with the `miri` component (installed in the Miri CI job only) over three kinds
of target:

- the `ambient-fs` behavioural tests, the only descriptor-owning wrapper left
  in the repository;
- the host unit tests of the component crates that have them; and
- ABI round trips through the generated glue in `tools-src/web-search`
  (tool world) and `channels-src/discord` (channel world). A test-only module
  under `src/bindings/` plays the host: it allocates arguments as
  `cabi_realloc` would, calls the generated `_export_*_cabi` shims, decodes the
  return area and calls `__post_return_*`. That module is part of the audited
  boundary, so the `expect` covers its `unsafe`, and the contract lets it hold
  `unsafe` while rejecting exports and lint overrides there. The glue is
  written for 4-byte pointers, so the round trips build only for 32-bit targets
  and `make miri` runs them for `i686-unknown-linux-gnu`. Both of Miri's
  aliasing models reject wit-bindgen's string lowering (it frees through a
  pointer derived from a shared reference), so these tests, and only these, run
  with Stacked Borrows off; the verification document records the finding.

The contract also requires `MIRI_COMPONENT_ROOTS` in the `Makefile` to list
every component crate that has host tests, so a new test cannot silently miss
Miri. The root crate is not interpreted: it has no unsafe code, and building it
for Miri costs hours to run safe tests. What Miri does and does not cover is
recorded in
[Rust boundary verification and unsafe inventory](../rust-boundary-verification.md).
Kani and Verus are possible follow-ups; this ADR does not add them.

## Known risks and limitations

- `deny` is weaker than `forbid`: an inner `#[allow(unsafe_code)]` compiles.
  The static contract rejects every `unsafe_code` attribute outside the one
  declaration, and the final lint layer's `allow_attributes` denial is a second
  barrier.
- The generated glue is verified only where a round trip exists. Miri runs the
  glue for one tool crate and one channel crate with no host imports, not for
  the other twelve crates, and it cannot execute any path that calls a host
  import. The rest rests on wit-bindgen's pinned versions (0.36 and 0.41) and
  on the host's integration tests that load the built components.
- `rustix` is a new direct dependency. It is already in the lock and widely
  used, and the Linux backend issues syscalls directly, but it is trusted code
  that the compiler cannot check for us.
- Resolved by issue #468: Axinite parses `.env` files with
  `dotenvy::from_read_iter` into replaceable `DotenvLayers` snapshots and
  merges them into `EnvContext`. Startup and async config reloads no longer
  call a mutating `dotenvy` API, and `clippy::disallowed_methods` prevents
  those APIs from returning. Configuration code sees dotenv values; direct
  process reads and child processes do not. CWD-file values can no longer
  substitute into the separate bootstrap file. Spawn sites now pass required
  credentials through their explicit environment seams. The dependency scan and
  remaining `set_var` hits are recorded in
  [Rust boundary verification and unsafe inventory](../rust-boundary-verification.md).
- `EnvContext::capture_ambient` now applies the libSQL autodetection, so a
  developer whose `~/.axinite/axinite.db` exists and who sets no
  `DATABASE_BACKEND` sees `libsql` in every ambient snapshot, which is what the
  process environment showed after startup before. Code that reads the process
  environment directly no longer sees the autodetected value.

## Consequences

- A change that adds an unsafe block anywhere in the root workspace fails the
  compiler, not just a review.
- Adding a crate requires editing the inventory in
  `scripts/unsafe_boundaries/inventory.py` and Table 3, which a reviewer sees.
- The fourteen component crates gain one small file each and no behaviour
  change.

## Related decisions

This ADR follows cuprum's ADR 011, "Audited Rust safety boundaries" (commit
`33bff5dbc68eac170848d5ef317c377a3198342c`), and builds on the environment
injection work in pull request #364 (issue #333).
