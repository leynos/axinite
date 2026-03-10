CARGO ?= cargo
GITHUB_TOOL_MANIFEST := tools-src/github/Cargo.toml

.PHONY: check-fmt typecheck lint test test-matrix

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
	$(CARGO) test -- --nocapture
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture

test-matrix:
	$(CARGO) test -- --nocapture
	$(CARGO) test --no-default-features --features libsql -- --nocapture
	$(CARGO) test --features postgres,libsql,html-to-markdown -- --nocapture
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST) -- --nocapture
