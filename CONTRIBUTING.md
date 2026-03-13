# Contributing

## Getting Started

```bash
git clone https://github.com/nearai/ironclaw.git
cd ironclaw
./scripts/dev-setup.sh
```

This installs the Rust toolchain, WASM targets, git hooks, and runs initial checks.

## Development Workflow

```bash
cargo fmt                                                    # format
cargo clippy --all --benches --tests --examples --all-features  # lint (zero warnings)
cargo test                                                   # unit tests
cargo test --features integration                            # + PostgreSQL tests
```

## Code Style

- Zero clippy warnings policy
- No `.unwrap()` or `.expect()` in production code (tests are fine)
- Use `thiserror` for error types, map errors with context
- Prefer `crate::` for cross-module imports
- Comments for non-obvious logic only

See `AGENTS.md` for the canonical style and contributor guidelines. The root
`CLAUDE.md` file remains only as a compatibility redirect for older workflows.

## Feature Parity Requirement

Changes that affect a tracked capability must update `FEATURE_PARITY.md` in the
same branch.

### Required before opening a PR

1. Review the relevant parity rows in `FEATURE_PARITY.md`.
2. Update status/notes if behavior changed.
3. Include the `FEATURE_PARITY.md` diff in your commit when applicable.

## Review Tracks

All PRs follow a risk-based review process:

<!-- markdownlint-disable MD013 MD060 -->
| Track | Scope | Requirements |
| ----- | ----- | ------------ |
| **A** | Docs, tests, chore, dependency bumps | 1 approval + CI green |
| **B** | Features, refactors, new tools/channels | 1 approval + CI green + test evidence |
| **C** | Security (`src/safety/`, `src/secrets/`), runtime (`src/agent/`, `src/worker/`), database schema, CI workflows | 2 approvals + rollback plan documented |
<!-- markdownlint-enable MD013 MD060 -->

Select the appropriate track in the PR template based on the files and behaviour
the change touches.

## Database Changes

IronClaw uses dual-backend persistence (PostgreSQL + libSQL). All new
persistence features must support both backends. See `src/db/CLAUDE.md`.

## Adding Dependencies

Run `cargo deny check` before adding new dependencies to verify licence
compatibility and check for known advisories.
