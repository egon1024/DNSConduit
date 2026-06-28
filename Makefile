# Local checks aligned with .github/workflows/ci.yml
#
#   make test        — fmt-check, clippy, and unit tests (CI parity)
#   make performance — optional local benchmarks (not CI; see Future below)
#   make fmt-check   — verify formatting only
#   make clippy      — workspace clippy with warnings denied
#   make unit        — cargo test --workspace
#   make fmt         — apply rustfmt (fix formatting)
#   make build       — cargo build --workspace
#

CARGO ?= cargo
CLIPPY_FLAGS := --workspace --all-targets -- -D warnings

PYTHON ?= python3
DOCS_DIR := operator-docs
DOCS_PORT ?= 8000

.PHONY: help test performance fmt fmt-check clippy unit build docs-serve docs-build docs-gen

help:
	@echo "DNSConduit Makefile targets:"
	@echo "  make test         Run fmt-check, clippy, and unit tests (same order as CI)"
	@echo "  make performance  Run optional local benchmarks (release; not CI)"
	@echo "  make fmt-check    Check formatting (cargo fmt --check)"
	@echo "  make fmt          Apply rustfmt"
	@echo "  make clippy       Run clippy (-D warnings)"
	@echo "  make unit         Run cargo test --workspace"
	@echo "  make build        Build all workspace crates"
	@echo "  make docs-serve   Serve operator-docs/ at http://0.0.0.0:$(DOCS_PORT) (live reload)"
	@echo "  make docs-build   Build operator-docs/ (mkdocs --strict)"

docs-gen:
	@ver=$$(awk -F'"' '/^version = / {print $$2; exit}' Cargo.toml); \
		echo "$${ver:-development}" > $(DOCS_DIR)/.doc-version
	$(PYTHON) $(DOCS_DIR)/scripts/gen_versions_index.py --stub \
		--output $(DOCS_DIR)/docs/versions.md

docs-build: docs-gen
	cd $(DOCS_DIR) && $(PYTHON) -m mkdocs build --strict

docs-serve: docs-gen
	cd $(DOCS_DIR) && $(PYTHON) -m mkdocs serve -a 0.0.0.0:$(DOCS_PORT)

test: fmt-check clippy unit

fmt-check:
	$(CARGO) fmt --all -- --check

fmt:
	$(CARGO) fmt --all

clippy:
	$(CARGO) clippy $(CLIPPY_FLAGS)

unit:
	$(CARGO) test --workspace

performance:
	# Today: Rhai thread-local engine reuse only. See Makefile header "Future".
	$(CARGO) bench -p conduit-script --bench thread_local_runtime --features test-util

build:
	$(CARGO) build --workspace
