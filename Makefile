CARGO_RESOLVED := $(shell command -v cargo 2>/dev/null || printf '%s' "$$HOME/.cargo/bin/cargo")
# Some CI environments export CARGO as an empty string; treat that as unset.
ifeq ($(strip $(value CARGO)),)
override CARGO := $(CARGO_RESOLVED)
endif
# Keep resolved executable paths as one shell argument, including paths with
# spaces, literal dollar signs, or shell metacharacters. The replacement is
# the POSIX single-quote escape sequence for an embedded single quote.
shell_quote = '$(subst ','"'"',$(1))'
CARGO_COMMAND := $(call shell_quote,$(value CARGO))
NEXTEST ?= $(CARGO_COMMAND) nextest
BUNX ?= $(shell command -v bunx 2>/dev/null || printf '%s' "$$HOME/.bun/bin/bunx")
TEST_FEATURES ?= --features test-helpers,embedded-postgres
NEXTEST_PROFILE ?= default
MARKDOWNLINT_BASE ?= origin/main
CARGO_AUDIT_SUBCOMMAND ?= audit
ifeq ($(strip $(value CARGO_AUDIT)),)
CARGO_AUDIT_COMMAND := $(CARGO_COMMAND) $(call shell_quote,$(value CARGO_AUDIT_SUBCOMMAND))
else
CARGO_AUDIT_COMMAND := $(value CARGO_AUDIT)
endif
WHITAKER ?= whitaker
# The development build standard (concordat rule `rust-build-defaults`): the
# `mold` linker and the parallel `rustc` frontend (`-Zthreads=8`) on the pinned
# nightly. `.cargo/config.toml` carries both, so a bare `cargo build` takes them
# too and there is no slower path to choose. The Cranelift backend is refused;
# see the "Cranelift" section of docs/developers-guide.md. The toolchain is not
# pinned separately: the standard uses the nightly in `rust-toolchain.toml`.
RUST_TOOLCHAIN_FILE ?= rust-toolchain.toml
MOLD_VERSION_FILE ?= tools/mold/VERSION
MOLD_SHA256SUMS_FILE ?= tools/mold/SHA256SUMS
BUILD_TOOLS_PREFIX ?= $(HOME)/.local
# Exported rather than interpolated into recipes: Make hands an exported
# variable to the child process directly, so a path containing a quote cannot
# break the command line the shell parses.
export RUST_TOOLCHAIN_FILE MOLD_VERSION_FILE MOLD_SHA256SUMS_FILE BUILD_TOOLS_PREFIX
# `make install-build-tools` unpacks the pinned `mold` here. Putting its `bin/`
# first makes the pinned release win over a distribution one, for the check and
# for `-fuse-ld=mold` alike. It follows CARGO_RESOLVED above on purpose: Cargo
# is looked up on the caller's PATH, not on this one.
export PATH := $(BUILD_TOOLS_PREFIX)/bin:$(PATH)

# No development recipe assigns `RUSTFLAGS`, so `cargo` reads the standard from
# `.cargo/config.toml` on every such route. Cargo selects a single `rustflags`
# source, and an assigned `RUSTFLAGS` outranks every `rustflags` table in that
# file. A recipe that assigned one would therefore have to restate the standard
# (`-Zthreads=8` and, on Linux, `-Clink-arg=-fuse-ld=mold`). Only the routes
# that exclude it on purpose assign `RUSTFLAGS`:
#
# - Release and packaging builds take neither the parallel frontend nor
#   `mold`. Assigning `RUSTFLAGS` at all, even to an inherited empty value,
#   displaces the configuration's `rustflags` tables, which is the whole
#   mechanism. The release profile names no codegen backend, so nothing else
#   is needed. `RELEASE_RUSTFLAGS` is that assignment.
# - Whitaker lints under the toolchain its installer manages, so it takes none
#   of the standard. Its `RUSTFLAGS="-D warnings"` literal displaces the
#   configuration's `rustflags` (see `lint-whitaker`).
#
# tests/workflow_contracts/build_standard_make_test.py evaluates every Cargo
# invocation these recipes emit and holds all of the above.
RELEASE_RUSTFLAGS = RUSTFLAGS="$${RUSTFLAGS-}"
NIXIE ?= nixie
UV ?= uv
UV_ENV = UV_CACHE_DIR=.uv-cache UV_TOOL_DIR=.uv-tools
# Pin the resolved commit so uv does not need to refresh a movable tag.
TYPOS_CONFIG_BUILDER_REVISION ?= c8a4f95d7cf7f6a1b7517f2775d122d47d5721eb
TYPOS_CONFIG_BUILDER = $(UV_ENV) $(UV) tool run --python 3.14 --from \
	"git+https://github.com/leynos/typos-config-builder.git@$(TYPOS_CONFIG_BUILDER_REVISION)" \
	typos-config-builder
WASM_SHARED_TARGET_DIR ?= $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target/wasm-extensions)
LIBSQL_TRANSPORT_TARGET_DIR := $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target)
GITHUB_TOOL_MANIFEST := tools-src/github/Cargo.toml
GITHUB_TOOL_WASM_TARGET := wasm32-wasip2
# Keep audit ignores centralized and remove each one as soon as the triggering
# transitive dependency is upgraded.
# RUSTSEC-2025-0141: bincode 1.3.3 is unmaintained via libsql. Track removal
# in https://github.com/leynos/axinite/issues/211.
# RUSTSEC-2024-0370: proc-macro-error 1.0.4 is unmaintained via
# rstest-bdd-macros. Track removal in
# https://github.com/leynos/axinite/issues/212.
# RUSTSEC-2026-0235: rkyv 0.7.46 is an optional rust_decimal dependency. No
# Axinite feature enables rust_decimal's rkyv feature; remove when rust_decimal
# no longer records rkyv <0.8.17 in its published dependency metadata.
# RUSTSEC-2026-0258: h2 0.3.27 is required by libsql 0.9's remote-replica
# client stack. Remote replicas are supported, so remove when libsql no longer
# requires h2 <0.4.16.
# RUSTSEC-2026-0192: ttf-parser 0.25.1 is unmaintained, via pdf-extract 0.12
# -> lopdf 0.42. pdf-extract 0.12.1 still requires lopdf ^0.42, and only lopdf
# 0.45 moves to skrifa, so no fixed path exists; remove when pdf-extract
# depends on lopdf without ttf-parser. scripts/verify_audit_ignore_paths.py
# refuses this ignore if ttf-parser gains any other dependency path.
# kuchikikiki 0.9.2 is yanked via readabilityrs. cargo-audit exposes no
# advisory ID to ignore for this warning; track removal in
# https://github.com/leynos/axinite/issues/214.
AUDIT_FLAGS ?= \
	--ignore RUSTSEC-2025-0141 \
	--ignore RUSTSEC-2024-0370
RUST_DECIMAL_AUDIT_FLAGS := \
	--ignore RUSTSEC-2026-0235
LIBSQL_AUDIT_FLAGS := \
	--ignore RUSTSEC-2026-0258
PDF_EXTRACT_AUDIT_FLAGS := \
	--ignore RUSTSEC-2026-0192

.PHONY: all build install-build-tools check-build-tools install install-with-overrides sync-local-wasm-overrides build-github-tool-wasm fmt check-fmt typecheck lint lint-clippy lint-whitaker markdownlint spelling nixie audit rust-audit test test-workspace test-github-tool test-libsql-transport lint-libsql-transport test-cargo test-matrix test-matrix-cargo test-workflow-contracts clean frontend-install frontend-build frontend-verify frontend-check frontend-test frontend-full frontend-stub

# `make fmt` and `make check-fmt` call mdtablefix directly. `--git` selects the
# Markdown files Git tracks and `--include-untracked` adds the untracked files
# Git does not ignore, so a new document is formatted before it is staged.
# Both modes need mdtablefix 0.6.0 or later; CI pins the version at the
# install-mdtablefix step.
MDLINT ?= $(shell command -v markdownlint-cli2 2>/dev/null || printf '%s' "$$HOME/.bun/bin/markdownlint-cli2")
MDTABLEFIX ?= mdtablefix
MDTABLEFIX_SELECT = --git --include-untracked
MDTABLEFIX_RULES = --wrap --renumber --breaks --ellipsis --fences

all: check-fmt lint test spelling

# Build the workspace as the standard development build: every target, with the
# test features, so a build that passes here is one `make test` can start from.
build: check-build-tools
	$(CARGO_COMMAND) build --all --all-targets $(TEST_FEATURES)

# Provision the pinned build tools (the `mold` linker and the pinned nightly
# with its components). Idempotent; CI runs it before every suite that
# compiles. The check below is the same list as a read-only probe.
install-build-tools:
	./scripts/install-build-tools.sh

# Fail with an installation hint, before Cargo runs, when a tool the standard
# needs is missing or drifted from its pin. Never falls back to a slower build.
check-build-tools:
	./scripts/check-build-tools.sh

BUN ?= $(shell command -v bun 2>/dev/null || printf '%s' "$$HOME/.bun/bin/bun")
FRONTEND_DIR := web-src
FRONTEND_DIST := $(FRONTEND_DIR)/dist
FRONTEND_EMBED_DIR := src/channels/web/static/solid

frontend-install:
	cd $(FRONTEND_DIR) && $(BUN) install --frozen-lockfile

# Build the SolidJS app and refresh the embedded copy served by the gateway.
frontend-build: frontend-install
	cd $(FRONTEND_DIR) && $(BUN) run build
	rsync -a --delete $(FRONTEND_DIST)/ $(FRONTEND_EMBED_DIR)/

# Fail when the committed embedded assets are stale relative to web-src.
frontend-verify: frontend-build
	git diff --exit-code -- $(FRONTEND_EMBED_DIR) || 		{ echo "error: $(FRONTEND_EMBED_DIR) is stale; commit the output of 'make frontend-build'" >&2; exit 1; }

# Static checks and unit suites for the browser workspace. `semantic`
# covers the classlist, semgrep, and stylelint rules and fetches semgrep
# through uvx on first use.
frontend-check: frontend-install
	cd $(FRONTEND_DIR) && $(BUN) run check:fmt && $(BUN) run lint && $(BUN) run check:types && $(BUN) run semantic

frontend-test: frontend-check
	cd $(FRONTEND_DIR) && $(BUN) run test && $(BUN) run test:a11y && $(BUN) run lint:ftl-vars

# The mockup's full verification chain: Tailwind compile check, lint,
# typecheck, unit + a11y + Fluent + semantic suites, the workspace
# Playwright spec (browsers must be installed), and moz-fluent-lint.
frontend-full: frontend-install
	cd $(FRONTEND_DIR) && $(BUN) run verify:full

# Daemon-free stub runtime: Bun mock API (HTTP + SSE + /api/features) plus a
# preview server for the built SPA on http://127.0.0.1:2020.
frontend-stub: frontend-install
	cd $(FRONTEND_DIR) && $(BUN) run dev

install:
	./scripts/build-wasm-extensions.sh
	$(RELEASE_RUSTFLAGS) $(CARGO_COMMAND) install --path .

install-with-overrides: install sync-local-wasm-overrides

sync-local-wasm-overrides:
	./scripts/sync-local-wasm-overrides.sh

build-github-tool-wasm:
	$(RELEASE_RUSTFLAGS) $(CARGO_COMMAND) build --manifest-path $(GITHUB_TOOL_MANIFEST) --release --target $(GITHUB_TOOL_WASM_TARGET)

# Format Rust and Markdown sources with the estate-wide formatter.
fmt:
	$(CARGO_COMMAND) fmt --all
	$(CARGO_COMMAND) fmt --manifest-path $(GITHUB_TOOL_MANIFEST) --all
	$(MDTABLEFIX) --in-place $(MDTABLEFIX_SELECT) $(MDTABLEFIX_RULES)
	$(MDLINT) --fix "**/*.md"

check-fmt:
	$(CARGO_COMMAND) fmt --all -- --check
	$(CARGO_COMMAND) fmt --manifest-path $(GITHUB_TOOL_MANIFEST) --all -- --check
	$(MDTABLEFIX) --check $(MDTABLEFIX_SELECT) $(MDTABLEFIX_RULES)

typecheck: check-build-tools
	$(CARGO_COMMAND) check --all --benches --tests --examples $(TEST_FEATURES)
	$(CARGO_COMMAND) check --all --benches --tests --examples --no-default-features --features libsql-test-helpers
	$(CARGO_COMMAND) check --all --benches --tests --examples --all-features $(TEST_FEATURES)
	$(CARGO_COMMAND) check --manifest-path $(GITHUB_TOOL_MANIFEST) --tests

lint: lint-clippy lint-whitaker

lint-clippy: check-build-tools
	$(CARGO_COMMAND) clippy --all --benches --tests --examples $(TEST_FEATURES) -- -D warnings
	$(CARGO_COMMAND) clippy --all --benches --tests --examples --no-default-features --features libsql-test-helpers -- -D warnings
	$(CARGO_COMMAND) clippy --all --benches --tests --examples --all-features $(TEST_FEATURES) -- -D warnings
	$(CARGO_COMMAND) clippy --manifest-path $(GITHUB_TOOL_MANIFEST) --tests -- -D warnings

# Cover the TLS member's additional feature configuration with strict lints.
lint-libsql-transport: check-build-tools
	$(CARGO_COMMAND) clippy --target-dir $(call shell_quote,$(LIBSQL_TRANSPORT_TARGET_DIR)) --locked --package hyper-rustls@0.25.0 --all-targets --all-features -- -D warnings

# Whitaker runs under the toolchain its installer manages, not the repository's
# pin, so it takes none of the development standard. The literal `RUSTFLAGS`
# is the boundary: it displaces the `rustflags` tables, leaving out the
# parallel frontend and `mold`, and nothing else in the configuration reaches
# it. `RUSTFLAGS` is assigned to deny warnings, as the other gates do through
# Clippy's `-- -D warnings`.
lint-whitaker: check-build-tools
	RUSTFLAGS="-D warnings" $(WHITAKER) --all -- --all-targets --all-features
	RUSTFLAGS="-D warnings" $(WHITAKER) --all --manifest-path $(GITHUB_TOOL_MANIFEST) -- --tests

markdownlint: spelling
	MARKDOWNLINT_BASE="$(MARKDOWNLINT_BASE)" ./scripts/lint-changed-markdown.sh "$(BUNX)"

spelling:
	$(TYPOS_CONFIG_BUILDER) gate --repository . --scope all

nixie:
	$(NIXIE) --no-sandbox

audit: rust-audit

# Workspace members, including the TLS integration patch, share the root lockfile.
rust-audit:
	find . \
		\( -path '*/target/*' -o -path '*/node_modules/*' -o -path '*/.venv/*' -o -path './crates/*' \) -prune -o \
		-name Cargo.toml -exec sh -c 'set -e; audit_command=$$1; shift; for manifest do \
			manifest_dir=$$(dirname "$$manifest"); \
			printf "Auditing Rust manifest %s\n" "$$manifest"; \
			if [ -f "$$manifest_dir/Cargo.lock" ]; then \
				python3 scripts/verify_audit_ignore_paths.py "$$manifest_dir/Cargo.lock"; \
				(cd "$$manifest_dir" && eval "$$audit_command" $(AUDIT_FLAGS) $(RUST_DECIMAL_AUDIT_FLAGS) $(LIBSQL_AUDIT_FLAGS) $(PDF_EXTRACT_AUDIT_FLAGS)); \
			else \
				(cd "$$manifest_dir" && eval "$$audit_command" $(AUDIT_FLAGS)); \
			fi; \
		done' sh $(call shell_quote,$(value CARGO_AUDIT_COMMAND)) {} +

# The whole suite, for a developer. CI runs the workspace, GitHub tool, and
# libSQL TLS suites on separate lanes; this target runs all three locally.
test: test-workspace test-github-tool test-libsql-transport

# The workspace suite alone. `tools-src/github` is excluded from the workspace,
# so `--workspace` does not reach it; the WASM build is still needed here
# because the metadata and schema tests load the artefact it produces.
test-workspace: check-build-tools
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace $(TEST_FEATURES) --profile $(NEXTEST_PROFILE)

# The GitHub tool crate's own suite. It is the only test work `--workspace`
# never covers, which is why it is the one lane that survives the CI
# de-duplication.
test-github-tool: check-build-tools
	$(CARGO_COMMAND) test --manifest-path $(GITHUB_TOOL_MANIFEST)

test-cargo: check-build-tools
	$(MAKE) build-github-tool-wasm
	$(CARGO_COMMAND) test $(TEST_FEATURES)
	$(CARGO_COMMAND) test --manifest-path $(GITHUB_TOOL_MANIFEST)

test-matrix: check-build-tools
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace $(TEST_FEATURES) --profile $(NEXTEST_PROFILE)
	$(NEXTEST) run --workspace --no-default-features --features libsql-test-helpers --profile $(NEXTEST_PROFILE)
	$(NEXTEST) run --workspace --features postgres,libsql-test-helpers,html-to-markdown,embedded-postgres --profile $(NEXTEST_PROFILE)
	$(CARGO_COMMAND) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

test-matrix-cargo: check-build-tools
	$(MAKE) build-github-tool-wasm
	$(CARGO_COMMAND) test $(TEST_FEATURES) -- --nocapture
	$(CARGO_COMMAND) test --no-default-features --features libsql-test-helpers -- --nocapture
	$(CARGO_COMMAND) test --features postgres,libsql-test-helpers,html-to-markdown,embedded-postgres -- --nocapture
	$(CARGO_COMMAND) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

# Validate the mutation-testing caller workflow contract.
test-workflow-contracts:
	uv run --python '>=3.12' --with 'pytest>=8' --with 'pyyaml>=6' --with 'hypothesis>=6' pytest tests/workflow_contracts -q

clean:
	$(CARGO_COMMAND) clean
	$(CARGO_COMMAND) clean --manifest-path $(GITHUB_TOOL_MANIFEST)
	rm -rf $(WASM_SHARED_TARGET_DIR)

# Exercise the TLS member with HTTP/2 enabled as well as the SDK default HTTP/1 path.
test-libsql-transport: check-build-tools
	$(CARGO_COMMAND) test --target-dir $(call shell_quote,$(LIBSQL_TRANSPORT_TARGET_DIR)) --locked --package hyper-rustls@0.25.0 --all-features --lib --examples
	$(CARGO_COMMAND) test --target-dir $(call shell_quote,$(LIBSQL_TRANSPORT_TARGET_DIR)) --locked --package hyper-rustls@0.25.0 --all-features --doc
	$(CARGO_COMMAND) test --target-dir $(call shell_quote,$(LIBSQL_TRANSPORT_TARGET_DIR)) --locked --package hyper-rustls@0.25.0 --doc
	$(CARGO_COMMAND) test --target-dir $(call shell_quote,$(LIBSQL_TRANSPORT_TARGET_DIR)) --locked --package hyper-rustls@0.25.0 --no-default-features --features rustls-native-certs,tokio-runtime,http1 --doc
