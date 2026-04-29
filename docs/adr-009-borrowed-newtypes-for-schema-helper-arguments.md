# ADR 009 — Borrowed newtypes for schema helper arguments

## Status

Accepted

## Date

2026-04-29

## Context

`src/tools/tool/schema_helpers.rs` contained a high proportion of generic
`&str`-typed function arguments. Issue `#38` reported 47.1% generic string
arguments across nine functions, above the 39.0% threshold. Generic string
parameters obscure intent, weaken invariants, and make call sites and tests
harder to read and validate.

The schema helper code also needs to preserve existing validation error text
exactly. The refactor therefore cannot introduce display wrappers or path
formatting changes that alter operator-visible diagnostics.

## Decision

Introduce explicit newtype wrappers for schema helper arguments:
`ParamName<'a>`, `SchemaPath`, and `ToolName<'a>`.

`ParamName<'a>` and `ToolName<'a>` are borrowed wrappers over `&'a str`.
Each type:

- carries a single `&'a str` field and is `Copy`;
- implements `From<&'a str>` and `From<&'a String>` so existing string call
  sites require no change;
- provides `as_str()`, `AsRef<str>`, and `Display` implementations that return
  the underlying string, preserving existing error message text exactly; and
- is re-exported publicly from `src/tools/mod.rs`.

`SchemaPath` is an owned wrapper over `String`. It implements `From<&str>`,
`From<&String>`, and `From<String>`, and provides `as_str()`, `AsRef<str>`, and
`Display` implementations that return the underlying path. The path is owned so
`SchemaPath::child(segment)` can produce the dot-joined child path used when
descending into nested schema nodes.

`ToolName<'a>` additionally implements `From<ToolName<'a>> for SchemaPath`
because the strict-schema validator (`validate_strict_schema`) roots its
recursive path at the tool name.

An internal `PropertyName<'a>` newtype is not introduced at this stage because
the current implementation derives per-property paths directly through
`SchemaPath::child(segment)`.

## Consequences

- The percentage of generic string-typed arguments in the module falls below
  the 39.0% threshold, satisfying the acceptance criteria of issue `#38`.
- Existing call sites continue to compile unchanged because helper signatures
  accept both `&str` and `&String` through the relevant conversion traits.
- Error message text is unaffected because all newtypes delegate `Display` and
  `as_str()` to the underlying string value.
- New call sites gain type-checked argument positions, reducing the risk of
  transposing a parameter name with a schema path.
- Borrow provenance is explicit for borrowed parameter names and tool names,
  while schema paths remain safe to pass through recursive descent because they
  own generated child paths.
