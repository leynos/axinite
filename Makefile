CARGO ?= $(shell command -v cargo 2>/dev/null || printf '%s' "$$HOME/.cargo/bin/cargo")
NEXTEST ?= $(CARGO) nextest
BUNX ?= $(shell command -v bunx 2>/dev/null || printf '%s' "$$HOME/.bun/bin/bunx")
TEST_FEATURES ?= --features test-helpers
NEXTEST_PROFILE ?= default
MARKDOWNLINT_BASE ?= origin/main
CARGO_AUDIT ?= $(CARGO) audit
WASM_SHARED_TARGET_DIR ?= $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target/wasm-extensions)
GITHUB_TOOL_MANIFEST := tools-src/github/Cargo.toml
GITHUB_TOOL_WASM_TARGET := wasm32-wasip2
# Keep audit ignores centralised and remove each one as soon as the triggering
# transitive dependency is upgraded.
# RUSTSEC-2026-0049: affected crate rustls-webpki 0.102.8, via libsql
# 0.9.30 -> hyper-rustls 0.25 -> rustls 0.22. CRL distribution point matching
# is not used directly; remove when libsql no longer pulls rustls-webpki
# <0.103.10.
# RUSTSEC-2026-0098: affected crate rustls-webpki 0.102.8, via the same
# libsql TLS chain. URI name-constraint handling is accepted only for this
# transitive dependency; remove when libsql no longer pulls rustls-webpki
# <0.103.12.
# RUSTSEC-2026-0099: affected crate rustls-webpki 0.102.8, via the same
# libsql TLS chain. Wildcard/name-constraint handling is accepted only for
# this transitive dependency; remove when libsql no longer pulls rustls-webpki
# <0.103.12.
# RUSTSEC-2026-0104: affected crate rustls-webpki 0.102.8, via the same
# libsql TLS chain. axinite does not parse CRLs directly; remove when libsql
# no longer pulls rustls-webpki <0.103.13.
# RUSTSEC-2026-0149: wasmtime-wasi WASI path_open(TRUNCATE) bypass. Temporary
# ignore until wasmtime >=44.0.2 / >=45.0.0 is published on crates.io.
AUDIT_FLAGS ?= \
	--ignore RUSTSEC-2026-0049 \
	--ignore RUSTSEC-2026-0098 \
	--ignore RUSTSEC-2026-0099 \
	--ignore RUSTSEC-2026-0104 \
	--ignore RUSTSEC-2026-0149

.PHONY: all install install-with-overrides sync-local-wasm-overrides build-github-tool-wasm check-fmt typecheck lint markdownlint audit rust-audit test test-cargo test-matrix test-matrix-cargo clean

all: check-fmt lint test

install:
	./scripts/build-wasm-extensions.sh
	$(CARGO) install --path .

install-with-overrides: install sync-local-wasm-overrides

sync-local-wasm-overrides:
	./scripts/sync-local-wasm-overrides.sh

build-github-tool-wasm:
	$(CARGO) build --manifest-path $(GITHUB_TOOL_MANIFEST) --release --target $(GITHUB_TOOL_WASM_TARGET)

check-fmt:
	$(CARGO) fmt --all -- --check
	$(CARGO) fmt --manifest-path $(GITHUB_TOOL_MANIFEST) --all -- --check

typecheck:
	$(CARGO) check --all --benches --tests --examples $(TEST_FEATURES)
	$(CARGO) check --all --benches --tests --examples --no-default-features --features libsql-test-helpers
	$(CARGO) check --all --benches --tests --examples --all-features $(TEST_FEATURES)
	$(CARGO) check --manifest-path $(GITHUB_TOOL_MANIFEST) --tests

lint:
	$(CARGO) clippy --all --benches --tests --examples $(TEST_FEATURES) -- -D warnings
	$(CARGO) clippy --all --benches --tests --examples --no-default-features --features libsql-test-helpers -- -D warnings
	$(CARGO) clippy --all --benches --tests --examples --all-features $(TEST_FEATURES) -- -D warnings
	$(CARGO) clippy --manifest-path $(GITHUB_TOOL_MANIFEST) --tests -- -D warnings

markdownlint:
	MARKDOWNLINT_BASE="$(MARKDOWNLINT_BASE)" ./scripts/lint-changed-markdown.sh "$(BUNX)"

audit: rust-audit

rust-audit:
	find . \
		\( -path '*/target/*' -o -path '*/node_modules/*' -o -path '*/.venv/*' \) -prune -o \
		-name Cargo.toml -exec sh -c 'set -e; for manifest do \
			manifest_dir=$$(dirname "$$manifest"); \
			printf "Auditing Rust manifest %s\n" "$$manifest"; \
			(cd "$$manifest_dir" && $(CARGO_AUDIT) $(AUDIT_FLAGS)); \
		done' sh {} +

test:
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace $(TEST_FEATURES) --profile $(NEXTEST_PROFILE)
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)

test-cargo:
	$(MAKE) build-github-tool-wasm
	$(CARGO) test $(TEST_FEATURES)
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)

test-matrix:
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace $(TEST_FEATURES) --profile $(NEXTEST_PROFILE)
	$(NEXTEST) run --workspace --no-default-features --features libsql-test-helpers --profile $(NEXTEST_PROFILE)
	$(NEXTEST) run --workspace --features postgres,libsql-test-helpers,html-to-markdown --profile $(NEXTEST_PROFILE)
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

test-matrix-cargo:
	$(MAKE) build-github-tool-wasm
	$(CARGO) test $(TEST_FEATURES) -- --nocapture
	$(CARGO) test --no-default-features --features libsql-test-helpers -- --nocapture
	$(CARGO) test --features postgres,libsql-test-helpers,html-to-markdown -- --nocapture
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

clean:
	$(CARGO) clean
	$(CARGO) clean --manifest-path $(GITHUB_TOOL_MANIFEST)
	rm -rf $(WASM_SHARED_TARGET_DIR)
