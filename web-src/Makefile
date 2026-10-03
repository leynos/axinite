MDLINT ?= markdownlint-cli2
NIXIE ?= nixie

.PHONY: all build clean fmt check-fmt lint typecheck test test-a11y test-e2e lint-ftl-vars semantic ff markdownlint nixie

all: check-fmt lint typecheck test

build:
	bun run build

clean:
	rm -rf dist test-results tmp playwright-report

fmt:
	bun run fmt

check-fmt:
	bun run check:fmt

lint:
	bun run lint

typecheck:
	bun run check:types

test:
	bun run test

test-a11y:
	bun run test:a11y

test-e2e:
	bun run test:e2e

lint-ftl-vars:
	bun run lint:ftl-vars

semantic:
	bun run semantic

ff:
	bun run verify:full

markdownlint:
	$(MDLINT) '**/*.md'

nixie:
	$(NIXIE) --no-sandbox
