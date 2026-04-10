# Testing abstractions guide

This document describes the crate-wide testing abstractions available in the
`ironclaw::testing` module and when to use each one.

## Overview

The testing module provides several complementary abstractions for different
testing scenarios:

| Abstraction | Purpose | Use when |
|-------------|---------|----------|
| `TestHarnessBuilder` | Full integration testing with real database | Testing actual persistence with a real database |
| `CapturingStore` | Unit testing without database | Verifying interactions without a real database |
| `NullDatabase` | Baseline test double | Creating baseline test doubles or custom mocks |

## TestHarnessBuilder

Located in: `crate::testing::TestHarnessBuilder`

The `TestHarnessBuilder` constructs a fully-wired `AgentDeps` with a real
libSQL-backed database (when the `libsql` feature is enabled). This is the
correct choice for integration-style tests that need to verify actual
persistence behaviour.

```rust
use ironclaw::testing::TestHarnessBuilder;

#[tokio::test]
async fn test_something() {
    let harness = TestHarnessBuilder::new().build().await;
    // use harness.deps, harness.db, etc.
}
```

**When to use:** Choose `TestHarnessBuilder` when your test needs to verify
actual database persistence or when testing components that require a real
`Database` trait implementation.

**Do not mix with:** `CapturingStore`. The harness uses its own database
internally; mixing it with `CapturingStore` will cause confusing behaviour.

## CapturingStore

Located in: `crate::testing::CapturingStore`

`CapturingStore` is a decorator wrapper around `NullDatabase` that records all
status updates and events for later inspection. It implements the `Database`
trait and can be used anywhere a database is required.

```rust
use ironclaw::testing::CapturingStore;

#[tokio::test]
async fn captures_calls() {
    let store = CapturingStore::new();
    // Pass store.clone() to components that need a Database
    // ... exercise the system under test ...

    // Later, inspect captured calls:
    let status = store.calls().last_status.lock().await.clone();
}
```

**Related types:**
- `StatusCall` / `StatusCallWithId` — Captured status update calls
- `EventCall` / `EventCallWithId` — Captured event calls with full history

**When to use:** Choose `CapturingStore` for unit tests that must not hit a
real database but need to verify that persistence calls were made correctly.

**Do not mix with:** The full `TestHarnessBuilder`. Use `CapturingStore` with
manually-constructed components, not the full harness.

## NullDatabase

Located in: `crate::testing::NullDatabase`

`NullDatabase` is a no-op database implementation that returns empty defaults
for all operations. It serves as a baseline for test doubles that need to
override only specific methods.

```rust
use ironclaw::testing::NullDatabase;

let db = NullDatabase::new();
// All operations return Ok(default_value)
```

**When to use:** Use `NullDatabase` as a base for custom mocks when you need
fine-grained control over specific database operations.

## Worker harness

Located in: `crate::testing::worker_harness`

The worker harness provides helpers for constructing `Worker` instances in
tests, including:

- `make_worker()` — Build a Worker with the given tools
- `make_worker_with_capturing_store()` — Build a Worker with a CapturingStore
- `TerminalMethod` — Helper enum for driving terminal state transitions

```rust
use ironclaw::testing::worker_harness::{make_worker, TerminalMethod};

let worker = make_worker(vec![]).await;
TerminalMethod::Completed.apply_transition(&worker).await;
```

**When to use:** Use the worker harness when testing `Worker` behavior
specifically.

## Choosing the right abstraction

```plaintext
Need to test persistence? ──Yes──► TestHarnessBuilder
         │
         No
         │
         ▼
Need to verify calls? ────Yes───► CapturingStore
         │
         No
         │
         ▼
Writing a custom mock? ───Yes───► NullDatabase (as base)
```

## Additional resources

- `crate::testing::TestHarnessBuilder` — Full harness builder
- `crate::testing::null_db::{NullDatabase, CapturingStore, EventCall, StatusCall}` —
  Database test doubles
