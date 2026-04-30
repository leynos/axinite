# Architectural decision record (ADR) 010: Consolidate bootstrap rename helpers in `src/bootstrap/migration.rs`
## Status

Accepted.

## Date

2026-04-29


## Context and Problem Statement

Bootstrap migration code had two functions for renaming files to the
`.migrated` suffix:

- `rename_bootstrap_to_migrated`, which returned `io::Result<()>`
- `rename_to_migrated`, which returned `()`

Both functions performed the same filesystem operation. The difference was in
how they exposed errors to callers, not in what they did to the filesystem.

That duplication created a maintenance risk. Divergent error-handling behaviour
could silently emerge if one copy was updated without the other, especially as
bootstrap-to-environment, JSON sidecar, disk-to-database legacy settings, and
legacy bootstrap migrations all rely on the same non-fatal rename semantics.

## Decision drivers

- **Single source of filesystem behaviour.** The migration code should have one
  implementation for applying the `.migrated` suffix.
- **Explicit non-fatal handling.** Call sites that intentionally warn and
  continue should make that choice locally rather than inheriting it from a
  helper that silently swallows errors.
- **Preserve operator diagnostics.** Rename failures should still be logged at
  warning level, so operators can diagnose partially migrated bootstrap files.
- **Avoid misleading success logs.** Success-level information should only be
  emitted after the underlying rename operation has actually succeeded.

## Decision outcome / proposed direction

Remove `rename_bootstrap_to_migrated` and `mark_legacy_migrated`.

Retain a single helper:

The following Rust signature names the retained helper and shows that callers
receive an `io::Result<()>` from the rename operation.

```rust
pub(super) fn rename_to_migrated(path: &Path) -> io::Result<()>
```

`rename_to_migrated` logs rename failures at `WARN` level and propagates the
`io::Error` to callers. This gives each caller a clear choice: propagate the
error with `?`, inspect it, or explicitly discard it when a migration path is
best-effort.

`rename_legacy_bootstrap` is the only call site that treats legacy bootstrap
renames as non-fatal while also emitting a success `INFO` log. It tests
`rename_to_migrated(...).is_ok()` and only emits the success log when that call
returns `Ok`.

All other non-fatal migration paths also use explicit `let _ = ...` discards:

- bootstrap-to-environment migration
- JSON sidecar migration
- disk-to-database legacy settings migration

## Consequences

- A single implementation reduces maintenance cost and the risk of behavioural
  drift.
- Callers must explicitly opt out of error propagation rather than having
  errors silently swallowed inside the helper.
- The success `INFO` log in `rename_legacy_bootstrap` is emitted only when the
  rename actually succeeds.
- Existing warn-and-continue behaviour at all call sites is preserved.


## Related references

- Issue `#33`: [chore(bootstrap): deduplicate migration helpers](https://github.com/leynos/axinite/issues/33).
- PR `#166`: [Issue #33: Deduplicate bootstrap migration rename helpers](https://github.com/leynos/axinite/pull/166).
- Implementation:
  [`src/bootstrap/migration.rs`](../src/bootstrap/migration.rs), which defines
  `rename_legacy_bootstrap`.
- See also: [`docs/contents.md`](contents.md).

# Architectural decision record (ADR) 010: Consolidate bootstrap rename helpers in `src/bootstrap/migration.rs`
# Architectural decision record (ADR) 010: Consolidate bootstrap rename helpers in `src/bootstrap/migration.rs`
# Architectural decision record (ADR) 010: Consolidate bootstrap rename helpers in `src/bootstrap/migration.rs`

# Architectural decision record (ADR) 010: Consolidate bootstrap rename helpers in `src/bootstrap/migration.rs`
