# Agent Rules

## Feature Parity Update Policy

- Changes to implementation status for any feature tracked in
  `FEATURE_PARITY.md` must update that file in the same branch.
- A PR that changes feature behaviour must check `FEATURE_PARITY.md` for needed
  status updates (`❌`, `🚧`, `✅`, notes, and priorities).

## Code Style and Structure

- **Code is for humans.** Write code with clarity and empathy. Assume a tired
  teammate will need to debug it at 3 a.m.
- **Comment *why*, not *what*.** Explain assumptions, edge cases, trade-offs,
  or complexity. Do not echo the obvious.
- **Clarity over cleverness.** Be concise, but favour explicit over terse or
  obscure idioms.
- **Use functions and composition.** Avoid repetition by extracting reusable
  logic. Prefer declarative code where it remains readable.
- **Small, meaningful functions.** Functions must be clear in purpose, small
  enough to follow, and aligned with single responsibility and CQRS.
- **Clear commit messages.** Commit messages should describe what changed and
  why.
- **Name things precisely.** Use clear, descriptive variable and function
  names. For booleans, prefer names with `is`, `has`, or `should`.
- **Structure logically.** Each file should encapsulate a coherent module.
  Group related code close together.
- **Group by feature, not layer.** Colocate logic, fixtures, helpers, and
  interfaces that serve the same domain concept.
- **Use consistent spelling and grammar.** Comments and documentation should
  use en-GB-oxendict spelling and grammar, except when referring to external
  APIs, commands, filenames, or user-facing text that must retain another
  spelling.
- **Illustrate with clear examples.** Function documentation should include
  examples when examples materially improve understanding.
- **Keep file size manageable.** No single code file should exceed 400 lines.
  Break large dispatch tables, switches, or long test data blocks into smaller
  units where practical.

## Documentation Maintenance

- Use the Markdown files within `docs/` as a knowledge base and source of
  truth for requirements, dependency choices, and architectural decisions.
- Start documentation discovery from `docs/contents.md`, then read
  `docs/welcome-to-axinite.md` for product direction and
  `docs/axinite-architecture-overview.md` for the current runtime shape before
  making broad changes.
- When decisions change, requirements evolve, libraries are added or removed,
  or architectural patterns shift, proactively update the relevant documents in
  `docs/`.
- Documentation must use en-GB-oxendict spelling and grammar, except for
  external names and commands.
- Follow `docs/documentation-style-guide.md` for structure, terminology,
  document types, RFCs, ADRs, and roadmap formatting.

## Change Quality and Committing

- Aim for small, focused, atomic changes. Each commit should represent one
  logical unit of work.
- Before proposing or creating a commit, ensure the change:
  - is validated by relevant unit tests and behavioural tests where applicable
  - includes a regression test when fixing a bug and a regression test is
    appropriate
  - passes relevant test suites
  - passes lint checks
  - passes formatting checks
- Only commit changes that meet the quality gates.
- Write clear, descriptive commit messages that:
  - use the imperative mood in the subject line
  - keep the first line concise
  - include a blank line before the body
  - explain the what and why in the body, with lines wrapped at 72 columns
  - use Markdown when formatted structure is helpful

## Refactoring Heuristics and Workflow

- Regularly assess the codebase for refactoring opportunities, especially when
  work encounters:
  - long functions or methods
  - duplicated code
  - deeply nested or complex conditionals
  - large logic blocks dedicated to deriving a single value
  - primitive obsession or repeated data clumps
  - functions with excessive parameters
  - feature envy
  - shotgun surgery
- After committing a functional change or bug fix, review the surrounding code
  for refactoring opportunities.
- If refactoring is necessary, perform it as a separate, atomic commit after
  the functional change and rerun the relevant gates.

## Rust-Specific Guidance

This repository is written in Rust and uses Cargo for building and dependency
management.

- Run `make all` before committing. It is the preferred aggregate gate for this
  repository and currently runs:
  - `make check-fmt`
  - `make lint`
  - `make test`
- `make typecheck` is available as a standalone quick smoke-check target but
  is not part of `make all` because `cargo clippy` is a strict superset of
  `cargo check`.
- The underlying targets are:
  - `make check-fmt`
    - `cargo fmt --all -- --check`
    - `cargo fmt --manifest-path tools-src/github/Cargo.toml --all -- --check`
  - `make typecheck`
    - `cargo check --all --benches --tests --examples`
    - `cargo check --all --benches --tests --examples`
      `--no-default-features --features libsql-test-helpers`
    - `cargo check --all --benches --tests --examples --all-features`
    - `cargo check --manifest-path tools-src/github/Cargo.toml --tests`
  - `make lint`
    - `cargo clippy --all --benches --tests --examples -- -D warnings`
    - `cargo clippy --all --benches --tests --examples`
      `--no-default-features --features libsql-test-helpers -- -D warnings`
    - `cargo clippy --all --benches --tests --examples --all-features -- -D warnings`
    - `cargo clippy --manifest-path tools-src/github/Cargo.toml --tests -- -D warnings`
  - `make test`
    - `make build-github-tool-wasm`
    - `cargo nextest run --workspace --profile $NEXTEST_PROFILE`
    - `cargo test --manifest-path tools-src/github/Cargo.toml`
- Nextest profiles are configured in `.config/nextest.toml`. The `default`
  profile excludes expensive compile-contract tests (trybuild); the `ci`
  profile runs everything. Pass `NEXTEST_PROFILE=ci` to `make test` or
  `make test-matrix` to include them locally.
- Use `make test-matrix` when a change touches feature combinations,
  database-specific behaviour, or anything that warrants broader confidence
  than the default gate.
- Clippy warnings must be treated as errors.
- Fix warnings in code rather than silencing them.
- Extract helper functions when a function becomes too long.
- Group related parameters into meaningfully named structs when a function has
  too many parameters.
- Where a large error payload is being cloned or moved around frequently,
  consider `Arc` to reduce copies.
- Write unit and behavioural tests for new functionality.
- Every module should begin with a module-level (`//!`) comment explaining the
  module's purpose where that module is non-trivial.
- Document public APIs with Rustdoc comments (`///`).
- Prefer immutable data and avoid unnecessary `mut`.
- Use explicit version ranges in `Cargo.toml` and keep dependencies current.
- Avoid `unsafe` unless absolutely necessary, and document any use with a
  `SAFETY` comment.
- Place function attributes after doc comments.
- Do not use `return` in single-line functions.
- Use predicate functions for conditional criteria with more than two branches.
- Lints must not be silenced except as a last resort.
- Lint suppressions must be tightly scoped and include a clear reason.
- Use `concat!()` to combine long string literals rather than escaping
  newlines with a backslash.
- Prefer single-line function bodies where they remain readable.
- Use newtypes to model domain values and avoid integer soup. Reach for
  `newt-hype` for families of homogeneous wrappers, tuple structs for bespoke
  validation or trait surfaces, and `the-newtype` where shared trait
  forwarding across owned traits materially reduces boilerplate.
- Prefer `cap_std`, `cap_std::fs_utf8`, and `camino` over `std::fs` and
  `std::path` when capability-oriented filesystem access improves correctness.

### Testing

- Use `rstest` fixtures for shared setup.
- Replace duplicated tests with `#[rstest(...)]` parameterized cases.
- Prefer `mockall` for ad hoc mocks and stubs.
- For functionality depending on environment variables, prefer dependency
  injection and the `mockable` crate.
- If mockable cannot be used, environment mutation in tests must be wrapped in
  shared guards and mutexes in a shared `test_utils` or `test_helpers` crate.
  Direct environment mutation in tests is forbidden.

### Dependency Management

- Use SemVer-compatible caret requirements for dependencies in `Cargo.toml`.
- Do not use wildcard (`*`) or open-ended inequality (`>=`) requirements.
- Use tilde requirements (`~`) only when patch-level pinning is required for a
  documented reason.

### Error Handling

- Prefer semantic error enums using `thiserror` for conditions the caller may
  inspect, retry, or map to an HTTP status.
- Use opaque application-boundary errors such as `eyre::Report` for
  human-readable logs, not as public library API types.
- Do not export opaque error types from library boundaries.
- In tests, prefer `.expect(...)` over `.unwrap()` for clearer failures.
- In production code and shared fixtures, avoid `.expect()` and propagate
  errors with `Result` and `?`.
- Keep `expect_used` strict; do not suppress the lint.
- Recognize that `allow-expect-in-tests = true` does not cover helpers outside
  `#[cfg(test)]` or `#[test]`.
- Use `anyhow` or `eyre` with `.context(...)` to preserve backtraces and add
  diagnostic context.
- Update helpers to return errors rather than panicking.
- When consuming fallible fixtures in `rstest`, have the test return `Result`
  and use `?`.

## Build, Run, and Debug

- Use these commands when a task specifically needs direct build or runtime
  access outside the `Makefile` gates:
  - `cargo fmt`
  - `cargo clippy --all --benches --tests --examples --all-features`
  - `cargo test`
  - `cargo test --features integration`
  - `RUST_LOG=ironclaw=debug cargo run`
- For debugging, these logging patterns are the default entry points:
  - `RUST_LOG=ironclaw=trace cargo run`
  - `RUST_LOG=ironclaw::agent=debug cargo run`
  - `RUST_LOG=ironclaw=debug,tower_http=debug cargo run`
- For end-to-end coverage, refer to `tests/e2e/CLAUDE.md`.

## Architecture and Extensibility

- Prefer generic and extensible architectures over hardcoded one-off
  integrations.
- If an implementation choice materially affects the abstraction surface, ask a
  clarifying question rather than baking in an overly narrow design.
- Key extensibility traits in this codebase include:
  - `Database`
  - `Channel`
  - `Tool`
  - `LlmProvider`
  - `SuccessEvaluator`
  - `EmbeddingProvider`
  - `NetworkPolicyDecider`
  - `Hook`
  - `Observer`
  - `Tunnel`
- All I/O is async with Tokio. Use `Arc<T>` for shared state and `RwLock`
  where concurrent read-heavy access is warranted.
- Prefer strong types over strings, especially for domain values, state
  transitions, identifiers, and configuration boundaries.
- Prefer `crate::` for cross-module imports. `super::` is fine for tests and
  tightly local intra-module references.
- Avoid `pub use` re-exports unless you are deliberately exposing an API to
  downstream consumers.
- Comments should explain non-obvious logic, invariants, or trade-offs, not
  restate the code.

## Module Specs and Ownership

- When modifying a module with its own spec, read that spec first. The spec is
  the tiebreaker when code and assumptions drift apart.
- Module-specific initialization logic belongs in the owning module as a public
  factory function, not in `main.rs` or `app.rs`.
- Feature-flag branching should stay inside the module that owns the
  abstraction.
- Current module-spec map:
  - `src/agent/` -> `src/agent/CLAUDE.md`
  - `src/channels/web/` -> `src/channels/web/CLAUDE.md`
  - `src/db/` -> `src/db/CLAUDE.md`
  - `src/llm/` -> `src/llm/CLAUDE.md`
  - `src/setup/` -> `src/setup/README.md`
  - `src/tools/` -> `src/tools/README.md`
  - `src/workspace/` -> `src/workspace/README.md`
  - `tests/e2e/` -> `tests/e2e/CLAUDE.md`

## Repository Structure

- Use `docs/repository-layout.md` as the source of truth for the repository
  layout and high-level directory responsibilities.
- When adding or moving code, preserve the ownership boundaries documented
  there unless the task explicitly requires a structural refactor.

## Database and Persistence Policy

- axinite supports PostgreSQL and libSQL/Turso. New persistence features must
  support both backends unless the user explicitly scopes the change otherwise.
- Read `src/db/CLAUDE.md` before making non-trivial database changes.
- Do not assume backend parity without verifying both implementations.
- Workspace memory and retrieval are part of the core product surface; treat
  persistence, indexing, and hybrid search changes as architectural work, not
  incidental plumbing.

## Channels, Tools, and Skills

- To add a new channel:
  - create `src/channels/my_channel.rs`
  - implement the `Channel` trait
  - add configuration in `src/config/channels.rs`
  - wire it in the channel setup path in `src/app.rs`
- Treat channels, tools, hooks, tunnels, and observers as extension surfaces.
  Prefer composing through their traits over adding special cases to core
  orchestration.
- The skills system extends the agent prompt through `SKILL.md` files. See
  `.claude/rules/skills.md` for detailed skill-loading and trust-model rules.
- Skills currently distinguish trusted versus installed sources and apply tool
  attenuation based on trust and budget. Do not bypass that model casually.
- Workspace identity files such as `AGENTS.md`, `SOUL.md`, `USER.md`,
  `IDENTITY.md`, and related control documents may be injected into model
  context. Treat them as part of the prompt surface.

## Runtime Model and Operations

- The application uses a job state machine with these expected transitions:
  `Pending -> InProgress -> Completed -> Submitted -> Accepted`, with failure
  and stuck-state exits as additional branches.
- Hooks exist at multiple lifecycle points, including inbound, tool-call,
  outbound, session-start, session-end, and response-transformation phases.
  Respect those extension points when changing the interaction pipeline.
- The tunnel subsystem abstracts public exposure through providers such as
  Cloudflare, ngrok, Tailscale, custom commands, and local-only operation.
- The observability subsystem is pluggable, but current live backends are
  intentionally limited. Do not document or imply unsupported observability
  behaviour.
- Current known platform limitations from the inherited project guidance:
  - several domain-specific tools remain stubs
  - integration tests need testcontainers-backed PostgreSQL coverage
  - MCP is request-response only and does not yet support streaming
  - automatic WASM tool-schema extraction remains incomplete
  - built tools still need better capabilities UX
  - tool versioning and rollback are not yet implemented
  - observability backends remain limited

## Configuration Guidance

- Use `.env.example` as the operator-facing index for environment variables.
- For model-provider configuration, read `src/llm/CLAUDE.md` before changing
  provider wiring, defaults, or provider-specific environment handling.
- For workspace and memory behaviour, read `src/workspace/README.md`.
- For setup and onboarding flows, read `src/setup/README.md`.
- For tool runtime details, read `src/tools/README.md`.

## Markdown Guidance

- Validate changed Markdown files with `bunx markdownlint-cli2 <paths>` unless
  the repository later grows a dedicated Make target for Markdown linting.
- Run `git diff --check` after documentation edits.
- When Mermaid diagrams are introduced or modified, validate them with `nixie`
  if available in the environment.
- Markdown paragraphs and bullet points should be wrapped at 80 columns.
- Code blocks should be wrapped at 120 columns.
- Tables and headings should not be wrapped.
- Use dashes (`-`) for list bullets.
- Use GitHub-flavoured Markdown footnotes (`[^1]`) for references and
  footnotes.

## Additional Tooling

The following tools are available in this environment and are useful when the
task warrants them:

- `mbake` for Makefile validation
- `strace` for tracing system calls and signals
- `gdb` for runtime inspection and post-mortem debugging
- `ripgrep` for fast recursive text search
- `ltrace` for tracing dynamic library calls
- `valgrind` for memory debugging and profiling
- `bpftrace` for eBPF-based tracing
- `lsof` for listing open files
- `htop` for interactive process inspection
- `iotop` for I/O monitoring
- `ncdu` for disk usage investigation
- `tree` for directory structure views
- `bat` for syntax-highlighted file reads
- `delta` for syntax-highlighted Git diffs
- `tcpdump` for packet capture and network analysis
- `nmap` for host and service discovery
- `lldb` as an alternative debugger
- `eza` as an enhanced `ls`
- `fzf` for fuzzy selection
- `hyperfine` for benchmarking commands
- `shellcheck` for shell-script linting
- `fd` as an ergonomic `find`
- `checkmake` for Makefile linting
- `srgn` for structural grep and syntax-aware edits
- `difft` (Difftastic) for semantic diffs

## Key Takeaway

These practices exist to keep the repository accurate, testable, maintainable,
and honest about its current state.
