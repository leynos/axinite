CARGO ?= $(shell command -v cargo 2>/dev/null || printf '%s' "$$HOME/.cargo/bin/cargo")
NEXTEST ?= $(CARGO) nextest
BUNX ?= $(shell command -v bunx 2>/dev/null || printf '%s' "$$HOME/.bun/bin/bunx")
TEST_FEATURES ?= --features test-helpers
NEXTEST_PROFILE ?= default
MARKDOWNLINT_BASE ?= origin/main
CARGO_AUDIT ?= $(CARGO) audit
WHITAKER ?= whitaker
NIXIE ?= nixie
UV ?= uv
UV_ENV = UV_CACHE_DIR=.uv-cache UV_TOOL_DIR=.uv-tools
RUST_PROVER_TOOLS_COMMIT := 2a9884d6e88a7821c7c94d256268aaa114534982
RUFF_VERSION ?= 0.15.12
PATHSPEC_VERSION ?= 1.1.1
TYPOS_VERSION ?= 1.48.0
TYPOS_CONFIG_BUILDER_COMMIT := d6da92f02240a79a945c835f69bdd08a888da1d0
TYPOS_CONFIG_BUILDER_SOURCE := git+https://github.com/leynos/typos-config-builder.git@$(TYPOS_CONFIG_BUILDER_COMMIT)
TYPOS_CONFIG_BUILDER := $(UV_ENV) $(UV) tool run --python 3.14 \
	--from "$(TYPOS_CONFIG_BUILDER_SOURCE)" typos-config-builder
SPELLING_PY_SRCS := \
	scripts/typos_rollout_check.py scripts/tests/test_typos_rollout_check.py
SPELLING_PY_TESTS := scripts/tests/test_typos_rollout_check.py
SPELLING_COVERAGE_ARGS := --cov=typos_rollout_check --cov-fail-under=90
SPELLING_HELPER_PYTEST = PYTHONPATH=scripts $(UV_ENV) $(UV) run --no-project \
	--python 3.14 --with pathspec==$(PATHSPEC_VERSION) --with pytest==9.0.2 \
	--with pytest-cov==7.0.0 python -m pytest
WASM_SHARED_TARGET_DIR ?= $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target/wasm-extensions)
REPAIR_CLAIMS_MANIFEST := verification/repair-claims/Cargo.toml
GITHUB_TOOL_MANIFEST := tools-src/github/Cargo.toml
GITHUB_TOOL_WASM_TARGET := wasm32-wasip2
# Keep audit ignores centralized and remove each one as soon as the triggering
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
# RUSTSEC-2026-0185: quinn-proto 0.11.14 remote memory exhaustion in
# out-of-order stream reassembly. Track removal in
# https://github.com/leynos/axinite/issues/210.
# RUSTSEC-2025-0141: bincode 1.3.3 is unmaintained via libsql. Track removal
# in https://github.com/leynos/axinite/issues/211.
# RUSTSEC-2024-0370: proc-macro-error 1.0.4 is unmaintained via
# rstest-bdd-macros. Track removal in
# https://github.com/leynos/axinite/issues/212.
# RUSTSEC-2025-0134: rustls-pemfile 2.2.0 is unmaintained via the libsql TLS
# chain. Track removal in https://github.com/leynos/axinite/issues/213.
# RUSTSEC-2026-0235: rkyv 0.7.46 is an optional rust_decimal dependency. No
# Axinite feature enables rust_decimal's rkyv feature; remove when rust_decimal
# no longer records rkyv <0.8.17 in its published dependency metadata.
# RUSTSEC-2026-0258: h2 0.3.27 is required by libsql 0.9's remote-replica
# client stack. Remote replicas are supported, so remove when libsql no longer
# requires h2 <0.4.16.
# kuchikikiki 0.9.2 is yanked via readabilityrs. cargo-audit exposes no
# advisory ID to ignore for this warning; track removal in
# https://github.com/leynos/axinite/issues/214.
AUDIT_FLAGS ?= \
	--ignore RUSTSEC-2026-0049 \
	--ignore RUSTSEC-2026-0098 \
	--ignore RUSTSEC-2026-0099 \
	--ignore RUSTSEC-2026-0104 \
	--ignore RUSTSEC-2026-0185 \
	--ignore RUSTSEC-2025-0141 \
	--ignore RUSTSEC-2024-0370 \
	--ignore RUSTSEC-2025-0134
RUST_DECIMAL_AUDIT_FLAGS := \
	--ignore RUSTSEC-2026-0235
LIBSQL_AUDIT_FLAGS := \
	--ignore RUSTSEC-2026-0258

.PHONY: install-kani check-kani-version kani test-kani-tooling lint-kani-tooling

.PHONY: all install install-with-overrides sync-local-wasm-overrides build-github-tool-wasm fmt check-fmt typecheck lint lint-clippy lint-whitaker markdownlint spelling spelling-phrase-check spelling-config spelling-config-write spelling-helper-test nixie audit rust-audit test test-cargo test-matrix test-matrix-cargo test-workflow-contracts clean

# Kani requires the binary-only `make install-kani` prerequisite.
# The retained HashSet compatibility failure deliberately keeps this gate red.
all: check-fmt lint test spelling kani

install-kani:
	python3 tools/kani/binary_tool.py install --prover-ref "$(RUST_PROVER_TOOLS_COMMIT)"

check-kani-version:
	python3 tools/kani/binary_tool.py check-version --prover-ref "$(RUST_PROVER_TOOLS_COMMIT)"

kani: check-kani-version
	python3 tools/kani/run_proofs.py

lint-kani-tooling:
	$(UV_ENV) $(UV) tool run --no-build ruff@$(RUFF_VERSION) format --check tools/kani/*.py tests/workflow_contracts/kani_binary_test.py
	$(UV_ENV) $(UV) tool run --no-build ruff@$(RUFF_VERSION) check tools/kani/*.py tests/workflow_contracts/kani_binary_test.py

test-kani-tooling:
	$(UV_ENV) $(UV) run --no-project --no-build --python 3.14 --with pytest==9.0.2 --with pyyaml==6.0.3 \
		python -m pytest tests/workflow_contracts/kani_binary_test.py -q

install:
	./scripts/build-wasm-extensions.sh
	$(CARGO) install --path .

install-with-overrides: install sync-local-wasm-overrides

sync-local-wasm-overrides:
	./scripts/sync-local-wasm-overrides.sh

build-github-tool-wasm:
	$(CARGO) build --manifest-path $(GITHUB_TOOL_MANIFEST) --release --target $(GITHUB_TOOL_WASM_TARGET)

# Format Rust and Markdown sources with the estate-wide formatter.
fmt:
	$(CARGO) fmt --all
	$(CARGO) fmt --manifest-path $(GITHUB_TOOL_MANIFEST) --all
	$(CARGO) fmt --manifest-path $(REPAIR_CLAIMS_MANIFEST) --all
	rustfmt --edition 2024 tools/kani/*.rs
	mdformat-all

check-fmt:
	$(CARGO) fmt --all -- --check
	$(CARGO) fmt --manifest-path $(GITHUB_TOOL_MANIFEST) --all -- --check
	$(CARGO) fmt --manifest-path $(REPAIR_CLAIMS_MANIFEST) --all -- --check
	rustfmt --edition 2024 --check tools/kani/*.rs

typecheck:
	$(CARGO) check --all --benches --tests --examples $(TEST_FEATURES)
	$(CARGO) check --all --benches --tests --examples --no-default-features --features libsql-test-helpers
	$(CARGO) check --all --benches --tests --examples --all-features $(TEST_FEATURES)
	$(CARGO) check --manifest-path $(GITHUB_TOOL_MANIFEST) --tests
	$(CARGO) check --manifest-path $(REPAIR_CLAIMS_MANIFEST) --all-targets --locked

lint: lint-clippy lint-whitaker lint-kani-tooling

lint-clippy:
	$(CARGO) clippy --all --benches --tests --examples $(TEST_FEATURES) -- -D warnings
	$(CARGO) clippy --all --benches --tests --examples --no-default-features --features libsql-test-helpers -- -D warnings
	$(CARGO) clippy --all --benches --tests --examples --all-features $(TEST_FEATURES) -- -D warnings
	$(CARGO) clippy --manifest-path $(GITHUB_TOOL_MANIFEST) --tests -- -D warnings
	$(CARGO) clippy --manifest-path $(REPAIR_CLAIMS_MANIFEST) --all-targets --locked -- -D warnings

lint-whitaker:
	RUSTFLAGS="-D warnings" $(WHITAKER) --all -- --all-targets --all-features
	RUSTFLAGS="-D warnings" $(WHITAKER) --all --manifest-path $(GITHUB_TOOL_MANIFEST) -- --tests
	RUSTFLAGS="-D warnings" $(WHITAKER) --all --manifest-path $(REPAIR_CLAIMS_MANIFEST) -- --tests

markdownlint: spelling
	MARKDOWNLINT_BASE="$(MARKDOWNLINT_BASE)" ./scripts/lint-changed-markdown.sh "$(BUNX)"

spelling: spelling-phrase-check
	@git ls-files -z | xargs -0 -r env $(UV_ENV) \
		$(UV) tool run typos@$(TYPOS_VERSION) --config typos.toml --force-exclude --hidden

spelling-phrase-check: spelling-config
	@PYTHONPATH=scripts $(UV_ENV) $(UV) run --no-project --python 3.14 \
		scripts/typos_rollout_check.py --repository .

spelling-config: spelling-helper-test
	@git ls-files --error-unmatch typos.toml >/dev/null
	@$(TYPOS_CONFIG_BUILDER) --repository . --check

spelling-config-write: spelling-helper-test
	@$(TYPOS_CONFIG_BUILDER) --repository .

spelling-helper-test:
	@$(UV_ENV) $(UV) tool run ruff@$(RUFF_VERSION) format --isolated --target-version py313 --check $(SPELLING_PY_SRCS)
	@$(UV_ENV) $(UV) tool run ruff@$(RUFF_VERSION) check --isolated --target-version py313 $(SPELLING_PY_SRCS)
	@$(SPELLING_HELPER_PYTEST) $(SPELLING_PY_TESTS) -c /dev/null --rootdir=. -p no:cacheprovider $(SPELLING_COVERAGE_ARGS)

nixie:
	$(NIXIE) --no-sandbox

audit: rust-audit

# crates/ holds root-workspace members; they share the root Cargo.lock and
# are audited through the root manifest, so the per-directory sweep skips
# them (cargo-audit needs a lockfile beside the manifest it audits).
rust-audit:
	find . \
		\( -path '*/target/*' -o -path '*/node_modules/*' -o -path '*/.venv/*' -o -path './crates/*' -o -path './.kani-binaries/*' \) -prune -o \
		-name Cargo.toml -exec sh -c 'set -e; for manifest do \
			manifest_dir=$$(dirname "$$manifest"); \
			printf "Auditing Rust manifest %s\n" "$$manifest"; \
			if [ -f "$$manifest_dir/Cargo.lock" ]; then \
				python3 scripts/verify_audit_ignore_paths.py "$$manifest_dir/Cargo.lock"; \
				(cd "$$manifest_dir" && $(CARGO_AUDIT) $(AUDIT_FLAGS) $(RUST_DECIMAL_AUDIT_FLAGS) $(LIBSQL_AUDIT_FLAGS)); \
			else \
				(cd "$$manifest_dir" && $(CARGO_AUDIT) $(AUDIT_FLAGS)); \
			fi; \
		done' sh {} +

test:
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace $(TEST_FEATURES) --profile $(NEXTEST_PROFILE)
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)
	$(CARGO) test --manifest-path $(REPAIR_CLAIMS_MANIFEST) --locked
	$(MAKE) test-kani-tooling

test-cargo:
	$(MAKE) build-github-tool-wasm
	$(CARGO) test $(TEST_FEATURES)
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)
	$(CARGO) test --manifest-path $(REPAIR_CLAIMS_MANIFEST) --locked
	$(MAKE) test-kani-tooling

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

# Validate the mutation-testing caller workflow contract.
test-workflow-contracts:
	uv run --with 'pytest>=8' --with 'pyyaml>=6' --with 'hypothesis>=6' pytest tests/workflow_contracts -q

clean:
	$(CARGO) clean
	$(CARGO) clean --manifest-path $(GITHUB_TOOL_MANIFEST)
	$(CARGO) clean --manifest-path $(REPAIR_CLAIMS_MANIFEST)
	rm -rf $(WASM_SHARED_TARGET_DIR)
