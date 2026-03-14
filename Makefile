CARGO ?= cargo
NEXTEST ?= cargo nextest
TEST_FEATURES ?= --features test-helpers
WASM_SHARED_TARGET_DIR ?= $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),target/wasm-extensions)
GITHUB_TOOL_MANIFEST := tools-src/github/Cargo.toml
GITHUB_TOOL_WASM_TARGET := wasm32-wasip2

.PHONY: all build-github-tool-wasm check-fmt typecheck lint test test-cargo test-matrix test-matrix-cargo clean

all: check-fmt typecheck lint test

build-github-tool-wasm:
	$(CARGO) build --manifest-path $(GITHUB_TOOL_MANIFEST) --release --target $(GITHUB_TOOL_WASM_TARGET)

check-fmt:
	$(CARGO) fmt --all -- --check
	$(CARGO) fmt --manifest-path $(GITHUB_TOOL_MANIFEST) --all -- --check

typecheck:
	$(CARGO) check --all --benches --tests --examples $(TEST_FEATURES)
	$(CARGO) check --all --benches --tests --examples --no-default-features --features libsql,test-helpers
	$(CARGO) check --all --benches --tests --examples --all-features $(TEST_FEATURES)
	$(CARGO) check --manifest-path $(GITHUB_TOOL_MANIFEST) --tests

lint:
	$(CARGO) clippy --all --benches --tests --examples $(TEST_FEATURES) -- -D warnings
	$(CARGO) clippy --all --benches --tests --examples --no-default-features --features libsql,test-helpers -- -D warnings
	$(CARGO) clippy --all --benches --tests --examples --all-features $(TEST_FEATURES) -- -D warnings
	$(CARGO) clippy --manifest-path $(GITHUB_TOOL_MANIFEST) --tests -- -D warnings

test:
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace $(TEST_FEATURES)
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)

test-cargo:
	$(MAKE) build-github-tool-wasm
	$(CARGO) test $(TEST_FEATURES)
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)

test-matrix:
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace $(TEST_FEATURES)
	$(NEXTEST) run --workspace --no-default-features --features libsql,test-helpers
	$(NEXTEST) run --workspace --features postgres,libsql,html-to-markdown,test-helpers
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

test-matrix-cargo:
	$(MAKE) build-github-tool-wasm
	$(CARGO) test $(TEST_FEATURES) -- --nocapture
	$(CARGO) test --no-default-features --features libsql,test-helpers -- --nocapture
	$(CARGO) test --features postgres,libsql,html-to-markdown,test-helpers -- --nocapture
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

clean:
	$(CARGO) clean
	$(CARGO) clean --manifest-path $(GITHUB_TOOL_MANIFEST)
	rm -rf $(WASM_SHARED_TARGET_DIR)
