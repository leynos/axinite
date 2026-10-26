# Rust boundary verification and unsafe inventory

## Status and scope

This document records the unsafe-code inventory for Axinite before and after
[ADR 0015](adrs/adr-0015-audited-unsafe-boundaries.md), what the boundary
contract checks, and exactly what Miri does and does not cover. It concerns Rust
`unsafe` only. It is anchored to base commit `4841a72a` (the pull request #364
head, which already removed the test-only environment mutation) and to the
original measurement on `9290dbc1`.

No row below is a proof of soundness. "Covered" means that the named tool ran
the named path to completion in the recorded configuration and found no
undefined behaviour on the inputs the tests supplied.

## Inventory before and after

Table 1. Unsafe sites by class, before and after this change.

| Class | Description                                    | On `9290dbc1` | On `4841a72a` | After | Disposition                                      |
| ----- | ---------------------------------------------- | ------------- | ------------- | ----- | ------------------------------------------------ |
| A     | `wit-bindgen` expansions in 14 component roots | 30            | 30            | 30    | Confined to `src/bindings.rs` in each crate      |
| B     | Test-only `set_var` and `remove_var`           | 44            | 0             | 0     | Removed by the #364 environment injection        |
| C     | Production `set_var("DATABASE_BACKEND")`       | 1             | 1             | 0     | Default moved into `EnvContext::capture_ambient` |
| D     | `SecretValue` zeroing in `Drop`                | 1             | 1             | 0     | `#[derive(Zeroize, ZeroizeOnDrop)]`              |
| E     | `libc::geteuid`                                | 1             | 1             | 0     | `rustix::process::geteuid`                       |
| F     | `openat2` syscall, `zeroed`, `from_raw_fd`     | 3             | 3             | 0     | `rustix::fs::openat2` returning `OwnedFd`        |
| G     | Environment mutation in `testing::test_utils`  | 3             | 0             | 0     | Removed by the #364 environment injection        |
| Total |                                                | 83            | 36            | 30    |                                                  |

_Table 1: Unsafe sites by class (counts of lint reports under
`unsafe_code = "forbid"`, as in the original measurement)._

The 30 remaining sites are all macro-generated. They are the three kinds the
original measurement found: 14 `unsafe fn` canonical-ABI declarations, 14
`unsafe(export_name)` attributes, and 2 `unsafe` blocks (in `tools-src/github`
and `tools-src/web-search`). The static contract proves that no handwritten
`unsafe` token exists in any boundary crate outside `src/bindings.rs`, and the
compiler proves that the safe crates have none at all.

The word `unsafe` also appears in `src/tools/builder/templates/sources.rs`,
inside string literals that are templates for generated tool source. That is
text, not code the compiler sees, and the lexer behind the contract ignores it.

## Dependencies that carry unsafe code on the repository's behalf

Removing the in-tree sites moved their obligations to dependencies. They are
trusted, not verified, here.

Table 2. Trusted dependencies that replaced in-tree unsafe code.

| Dependency       | Replaces                                       | What is trusted                                                                                                                                                                |
| ---------------- | ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `rustix` 1.1.4   | `libc::syscall(SYS_openat2)` and `geteuid`     | Correct `open_how` layout, syscall number, and `OwnedFd` construction on the `linux_raw` backend.                                                                              |
| `zeroize` 1.8    | The manual volatile-free zeroing in `Drop`     | Volatile overwrite of the `String` buffer that the optimizer may not elide, and `ZeroizeOnDrop` coverage.                                                                      |
| `dotenvy` 0.15.7 | `.env` format parsing through `from_read_iter` | The mutating loader APIs remain in the dependency but Axinite does not call them. `clippy::disallowed_methods` bans them; the normal-dependency source scan is recorded below. |
| `wit-bindgen`    | (unchanged) canonical-ABI shims                | Pinned at 0.36 and 0.41 in the component manifests; the host's tests load the built components.                                                                                |

## Runtime environment mutation scan (issue #468)

On 2026-10-25, the locked normal dependency graph contained 612 distinct
packages. It was captured with:

```sh
cargo tree --locked -e normal --prefix none --format '{p}' \
  > /tmp/axinite-468-normal-tree.txt
cargo tree --locked -e normal -i dotenvy
cargo vendor --locked --versioned-dirs /tmp/axinite-468-vendor
```

The vendored source scan selected the exact name/version pairs in that tree,
then searched every Rust source file in each matching directory, including
tests, documentation examples and build scripts, for `env::set_var` and
`env::remove_var`. It found 71 textual hits across the 612 packages. There are
no Axinite calls to either function. The three Axinite matches are comments:
`src/config/runtime_support.rs:249`, `src/setup/wizard/provider_flows.rs:130`
and `src/setup/wizard/provider_vendors.rs:208`.

The dependency matches are accounted for below. Build scripts are listed
separately because they run in Cargo's build process and are not part of the
Axinite runtime call path.

| Source matches                                                                                                                                                                                                                                                                        | Disposition                                                                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `aws-lc-sys-0.45.0/builder/cmake_builder.rs:138,142,712`; `builder/main.rs:93,102,113,115,229,244`                                                                                                                                                                                    | Build-script support code; not linked into Axinite at runtime.                                                                                                                         |
| `blake3-1.8.5/build.rs:313`                                                                                                                                                                                                                                                           | Build script; not linked into Axinite at runtime.                                                                                                                                      |
| `rquickjs-sys-0.9.0/build.rs:161,163,189,190,195`                                                                                                                                                                                                                                     | Build script; not linked into Axinite at runtime.                                                                                                                                      |
| `async-process-2.5.0/tests/std.rs:367,369`; `deadpool-0.12.3/tests/managed_config.rs:22,30-31`; `deadpool-postgres-0.14.1/tests/postgres.rs:227,236-237`; `zbus-5.17.0/tests/basic.rs:413,420`                                                                                        | Dependency tests only.                                                                                                                                                                 |
| `blocking-1.6.2/src/lib.rs:1006-1022`; `crossterm-0.29.0/src/style/types/colored.rs:311-317`; `home-0.5.12/src/windows.rs:66-80`; `refinery-core-0.8.16/src/config.rs:436,458`; `rustyline-18.0.1/src/tty/mod.rs:252,255`; `tracing-subscriber-0.3.23/src/fmt/fmt_layer.rs:1617-1637` | `#[cfg(test)]` code only.                                                                                                                                                              |
| `dotenvy-0.15.7/src/parse.rs:524,531`; `tests/test-variable-substitution.rs:10-11`; `tests/common/mod.rs:7`; `tests/integration/util/testenv.rs:297,301,316`                                                                                                                          | `dotenvy` tests only.                                                                                                                                                                  |
| `clap_builder-4.6.0/src/builder/arg.rs:2069,2099-2100,2136,2160,2184`; `nix-0.31.3/src/env.rs:32`; `wasmtime-internal-core-48.0.7/src/error/error.rs:317`                                                                                                                             | Documentation examples or comments only.                                                                                                                                               |
| `dotenvy-0.15.7/src/iter.rs:35,52`                                                                                                                                                                                                                                                    | Definitions of `Iter::load` and `Iter::load_override`. Axinite uses only `from_read_iter`; the mutating methods and other dotenvy mutators are banned by `clippy::disallowed_methods`. |
| `clap_complete-4.6.5/src/env/mod.rs:227`                                                                                                                                                                                                                                              | The environment-completion API removes its trigger variable when `CompleteEnv::try_complete` runs. Axinite has no `CompleteEnv` reference or call.                                     |
| `nix-0.31.3/src/env.rs:52`                                                                                                                                                                                                                                                            | Fallback implementation inside unsafe `nix::env::clearenv`; Axinite has no `clearenv` reference or call.                                                                               |
| `openssl-probe-0.2.1/src/lib.rs:58`                                                                                                                                                                                                                                                   | Environment write inside unsafe `try_init_openssl_env_vars`; no caller exists in the scanned dependency sources or Axinite.                                                            |

The normal-tree reverse-dependency query reports `axinite` as the only runtime
consumer of `dotenvy`. The source scan found no call from Axinite to the
mutating iterator methods. `clippy.toml` also bans `dotenvy::dotenv`,
`dotenv_override`, `from_path`, `from_path_override`, `from_filename`,
`from_filename_override`, `from_read`, `from_read_override`, `var`, `vars`,
`Iter::load` and `Iter::load_override`, each with the reason
`mutates the process environment; use DotenvLayers`.

The runtime snapshot reads the ambient environment in `capture_ambient_inputs`,
using `std::env::vars_os()` so non-Unicode process values are preserved for
named controls. It also captures `dirs::home_dir()` there as the platform
home-directory fallback when no home variable supplies one. Configuration reads
then use `EnvContext`. The remaining process-only reads serve host state,
explicit test controls, build-directory overrides (`CARGO_TARGET_DIR`,
`AXINITE_TOOLS_SRC`, and `AXINITE_CHANNELS_SRC`), the container worker token
(`AXINITE_WORKER_TOKEN`), and the PostgreSQL test harness. Host state includes
`HOME` and `XDG_RUNTIME_DIR` for shell forwarding and Docker socket discovery.
These values are captured with `EnvContext::capture_process()` and are never
loaded from `.env`. None of the reads writes to the environment. The existing
scoped `clippy::disallowed_methods` expectation covers only that snapshot
boundary. `#![forbid(unsafe_code)]` remains in place.

## What the boundary contract checks

`scripts/check_unsafe_boundaries.py` runs two halves. Its static half is also a
unit-tested part of `make test-workflow-contracts`.

Static half (no compiler):

- Every `Cargo.toml` is inventoried or exempt, and the root workspace's
  `members` and `exclude` match the inventory.
- Each safe crate resolves `unsafe_code` to `forbid` and carries
  `#![forbid(unsafe_code)]` in its library, binary and build-script roots.
- Each boundary crate resolves `unsafe_code` to `deny` in a lint table of its
  own, never by `workspace = true`.
- Each boundary crate declares `mod bindings;` behind exactly one
  `#[expect(unsafe_code, reason = "wit-bindgen component export ABI")]`.
- No `unsafe` token, `no_mangle`, `export_name`, `link_section` or
  `unsafe_code` lint attribute appears in any other source of a boundary crate,
  test targets included. Comments and string literals are ignored.
- `src/bindings.rs` contains only `use super::Type;`, `wit_bindgen::generate!`
  and `export!(Type);`, and the two types agree.

Compiler half (`make boundary-contract`):

- `crates/ambient-fs`, `tools-src/web-search` and `channels-src/discord` are
  copied to a scratch workspace. Four unsafe forms are appended together to
  `src/lib.rs`, a synthesized `tests/future_target.rs`, and every existing test
  target, and `cargo check --all-targets` must reject each target and name
  every form in its diagnostics.
- A control run strips the policy from the copy and must accept the first
  probe.

Fault sensitivity: `tests/workflow_contracts/unsafe_boundary_test.py` applies
each deviation named in `_unsafe_boundary_faults.py` to a disposable copy of
the real inputs. The faults remove or weaken a `forbid`, mix workspace
inheritance with overrides, remove a crate-root attribute, add unsafe code to
guest, library and integration-test sources, add ABI attributes outside the
bindings, remove or reword the `expect`, add a second `expect` or an `allow`,
add handwritten content to the bindings file, add an unlisted crate, member or
exclusion, and delete or reclassify an inventoried crate. Every fault must be
reported, and the unmutated copy must be clean.
`unsafe_boundary_probes_test.py` drives the compiler half through a recorded
compiler seam, including a lenient compiler and a probe that cannot fail.

## Miri coverage

`make miri` runs `cargo miri test` under the dated nightly pinned by
`MIRI_TOOLCHAIN` in the `Makefile`, with `-Zmiri-disable-isolation` so that
file-system calls are forwarded. The `miri` component is installed in the
dedicated `Miri` job of `.github/workflows/rust-boundaries.yml` only.

Table 3. What Miri covers and what it does not.

| Target                                                  | Covered by Miri                                                                                                                                                                                                                                                                                              | Not covered, and why                                                                                                                                                                          |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `crates/ambient-fs` `tests/file_operations.rs`          | Every wrapper function and the `File` and `OpenOptions` descriptor wrappers over real files                                                                                                                                                                                                                  | Advisory locking and `copy`: `fs4` calls `flock` and `std::fs::copy` calls `copy_file_range`, neither of which Miri implements. Those two tests are ignored with `#[cfg_attr(miri, ignore)]`. |
| `tools-src/web-search` `src/bindings/abi_round_trip.rs` | On `i686-unknown-linux-gnu`, with Stacked Borrows off: the generated tool-world glue. `description`, `schema` and `execute` are lifted from allocator-owned buffers, lowered into the return area, and released by `__post_return_*`. Allocation, bounds, alignment, initialization and leak checks stay on. | Paths that call a host import, because imports are `unreachable` on a non-WebAssembly target. The aliasing models (see "Findings in the generated glue").                                     |
| `channels-src/discord` `src/bindings/abi_round_trip.rs` | The same configuration for the channel-world glue: `on_broadcast` (13 flattened arguments, an optional string, an empty list, a `result` return area) and `on_status`.                                                                                                                                       | As above; the other callbacks log through a host import.                                                                                                                                      |
| The other component crates' host unit tests             | Pure guest logic: parsing, URL encoding, validation and JSON handling                                                                                                                                                                                                                                        | Their generated glue is compiled but not executed. Six Google tools and the Slack tool have no host tests yet.                                                                                |
| Root `axinite` crate                                    | Nothing                                                                                                                                                                                                                                                                                                      | See "Root crate" below.                                                                                                                                                                       |
| `rustix::fs::openat2`                                   | Nothing                                                                                                                                                                                                                                                                                                      | Miri does not model the `openat2` syscall. Native tests in `src/skills/file_read/tests` cover the kernel behaviour.                                                                           |

_Table 3: Miri coverage._

### Findings in the generated glue

Interpreting the ABI round trips found three facts about `wit-bindgen`'s
output. None is a defect in repository code, and the repository cannot change
generated code, so each shapes how the round trips run.

1. **Aliasing.** Both of Miri's aliasing models reject the string lowering.
   The generator lowers an owned string with `into_boxed_slice()`, takes
   `as_ptr()` (a pointer derived from a shared reference), forgets the box, and
   stores that pointer in the return area; `__post_return_*` later frees the
   allocation through it. Stacked Borrows reports the later read of the string
   as a retag of an invalidated tag, and Tree Borrows reports the deallocation
   as a write through a `Frozen` tag. Releases 0.36.0, 0.41.0 and 0.51.0 of
   `wit-bindgen-rust` all emit the pattern (`Instruction::StringLower` in
   `bindgen.rs`). On WebAssembly it is benign, because the host reads and frees
   linear memory without Rust's aliasing rules, but it means the round trips
   cannot pass with an aliasing model on. They run with
   `-Zmiri-disable-stacked-borrows`, only in the two crates and only for the
   tests whose names contain `abi_round_trip`; every other test keeps it on.
2. **Pointer width.** The generator lays the return area out for WebAssembly's
   4-byte pointers. On a 64-bit host target the 0.36 glue under-aligns it, and
   the standard library's debug assertions or Miri report a misaligned
   dereference. The round trips therefore build only when
   `target_pointer_width = "32"`, and `make miri` runs them for
   `i686-unknown-linux-gnu`, whose pointers match.
3. **Zero-length buffers.** `wit_bindgen::rt::cabi_realloc` returns an
   integer-to-pointer cast for an empty request, which Miri flags under strict
   provenance. The tests build empty strings and lists with `NonNull::dangling`
   instead.

Revisit all three when `wit-bindgen` is upgraded: if a release derives the
lowered pointer from a mutable or raw source, the round trips can run with the
aliasing model on, and the `Stacked Borrows off` configuration and this section
should go.

### Root crate

`make miri` does not interpret the root `axinite` crate, for three reasons.

- It has no unsafe code left. Miri finds undefined behaviour in executed unsafe
  operations; the root crate's remaining operations are safe, and the compiler
  already proves that with `forbid`.
- The memory-sensitive type, `SecretValue`, now delegates zeroing to
  `zeroize`'s derive. Its native tests assert `ZeroizeOnDrop` at compile time,
  redaction, and copy independence. Interpreting `zeroize` itself would test a
  dependency, not this repository.
- Building the crate for Miri compiles about 1,300 source files and the whole
  dependency graph (Wasmtime, libSQL, TLS, the Docker client) with the Miri
  driver, to execute a handful of safe tests. The cost is hours of compile time
  on a shared build pool for no new evidence.

Extracting small pure kernels (secret newtypes, path confinement, host
allow-listing) into their own crates would make both Miri and the bounded model
checking below practical; that is a follow-up, not part of this change.

## Follow-ups

- Kani harnesses for the pure confinement rules (path validation, host and
  domain allow-listing) and Verus proofs of any extracted kernel are possible
  follow-ups. This change deliberately adds neither.
- Seven component crates have no host unit tests (the six Google tools and the
  Slack tool). Adding pure-logic tests there extends Miri coverage without
  changing the contract.
