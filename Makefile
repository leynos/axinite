CARGO ?= cargo
NEXTEST ?= cargo nextest
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
	$(CARGO) check --all --benches --tests --examples
	$(CARGO) check --all --benches --tests --examples --no-default-features --features libsql
	$(CARGO) check --all --benches --tests --examples --all-features
	$(CARGO) check --manifest-path $(GITHUB_TOOL_MANIFEST) --tests

lint:
	$(CARGO) clippy --all --benches --tests --examples -- -D warnings
	$(CARGO) clippy --all --benches --tests --examples --no-default-features --features libsql -- -D warnings
	$(CARGO) clippy --all --benches --tests --examples --all-features -- -D warnings
	$(CARGO) clippy --manifest-path $(GITHUB_TOOL_MANIFEST) --tests -- -D warnings

test:
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)

test-cargo:
	$(MAKE) build-github-tool-wasm
	$(CARGO) test
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)

test-matrix:
	$(MAKE) build-github-tool-wasm
	$(NEXTEST) run --workspace
	$(NEXTEST) run --workspace --no-default-features --features libsql
	$(NEXTEST) run --workspace --features postgres,libsql,html-to-markdown
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

test-matrix-cargo:
	$(MAKE) build-github-tool-wasm
	$(CARGO) test -- --nocapture
	$(CARGO) test --no-default-features --features libsql -- --nocapture
	$(CARGO) test --features postgres,libsql,html-to-markdown -- --nocapture
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

clean:
	$(CARGO) clean
	$(CARGO) clean --manifest-path $(GITHUB_TOOL_MANIFEST)
	rm -rf $(WASM_SHARED_TARGET_DIR)
