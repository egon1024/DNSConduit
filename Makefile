# Local checks aligned with .github/workflows/ci.yml
#
#   make test        — fmt-check, clippy, and unit tests (CI parity)
#   make performance — optional local benchmarks (not CI)
#   make fmt-check   — verify formatting only
#   make clippy      — workspace clippy with warnings denied
#   make unit        — cargo test --workspace
#   make fmt         — apply rustfmt (fix formatting)
#   make build       — cargo build --workspace
#
# Interop Docker harness (local / lab only — not run by GitHub Actions):
#   make interop-image         — build conduit:local (or CONDUIT_IMAGE)
#   make interop-unit          — harness unit tests (no Docker cells)
#   make interop-fingerprint   — print inputs fingerprint (PR freshness)
#   make interop-smoke         — run smoke suite (Docker; does not rewrite results)
#   make interop-auth          — run fixture-auth-a on applicable auth peers
#   make interop-docs          — regenerate operator-docs matrix from latest.json
#   make interop-refresh       — full smoke + auth refresh; write results + docs
#
# Performance harness (local / lab only — not run by GitHub Actions load suites):
#   make perf-unit             — harness unit tests (no live loadgen)
#   make perf-list             — list scenario catalog
#   make perf-run-scale        — run scale suite (CONDUIT= path to binary)
#   make perf-run-shutdown-drain — run shutdown_drain suite
#   make perf-render           — render FROM=… FORMAT=plain|fancy|yaml|json|html
#   make performance           — remains Criterion microbench (distinct from suite run)
#

CARGO ?= cargo
CLIPPY_FLAGS := --workspace --all-targets -- -D warnings

PYTHON ?= python3
DOCS_DIR := operator-docs
DOCS_PORT ?= 8000

# Local interop SUT image (override: make interop-smoke CONDUIT_IMAGE=myrepo/conduit:tag)
CONDUIT_IMAGE ?= conduit:local
DOCKERFILE ?= Dockerfile

# Performance harness binary path (override: make perf-run-scale CONDUIT=./target/release/conduit)
CONDUIT ?= ./target/release/conduit
PERF_FROM ?=
PERF_FORMAT ?= plain

.PHONY: help test performance fmt fmt-check clippy unit build \
	docs-serve docs-build docs-version docs-versions-preview \
	interop-image interop-unit interop-fingerprint interop-smoke interop-auth \
	interop-docs interop-refresh \
	perf-unit perf-list perf-run-scale perf-run-shutdown-drain perf-render

help:
	@echo "DNSConduit Makefile targets:"
	@echo "  make test         Run fmt-check, clippy, and unit tests (same order as CI)"
	@echo "  make performance  Run optional microbenchmarks (release; not CI; not load suites)"
	@echo "  make fmt-check    Check formatting (cargo fmt --check)"
	@echo "  make fmt          Apply rustfmt"
	@echo "  make clippy       Run clippy (-D warnings)"
	@echo "  make unit         Run cargo test --workspace"
	@echo "  make build        Build all workspace crates"
	@echo "  make docs-serve   Serve operator-docs/ at http://0.0.0.0:$(DOCS_PORT) (live reload)"
	@echo "  make docs-build   Build operator-docs/ (mkdocs --strict)"
	@echo "  make docs-versions-preview  Serve global Versions page locally (port 8765)"
	@echo ""
	@echo "Interop harness (local/lab Docker — not executed by GitHub Actions):"
	@echo "  make interop-image        Build $(CONDUIT_IMAGE) from $(DOCKERFILE)"
	@echo "  make interop-unit         Harness unit tests (no Docker cells)"
	@echo "  make interop-fingerprint  Print inputs fingerprint"
	@echo "  make interop-smoke        Run smoke suite (Docker; no results write)"
	@echo "  make interop-auth         Run fixture-auth-a (auth peers; no results write)"
	@echo "  make interop-docs         Regenerate matrix docs from interop/results/latest.json"
	@echo "  make interop-refresh      Smoke + auth; write results and regenerate docs"
	@echo ""
	@echo "Performance harness (local/lab — load suites not run by GitHub Actions):"
	@echo "  make perf-unit            Harness unit tests (no live loadgen)"
	@echo "  make perf-list            List scenario catalog"
	@echo "  make perf-run-scale       Run scale suite (CONDUIT=$(CONDUIT))"
	@echo "  make perf-run-shutdown-drain  Run shutdown_drain suite (CONDUIT=$(CONDUIT))"
	@echo "  make perf-render          Render FROM=run.json FORMAT=$(PERF_FORMAT)"

# Write the header version label only. The Versions list now lives on a single global
# page published to the site root; it is no longer generated per build.
docs-version:
	@ver=$$(awk -F'"' '/^version = / {print $$2; exit}' Cargo.toml); \
		echo "$${ver:-development}" > $(DOCS_DIR)/.doc-version

docs-build: docs-version
	cd $(DOCS_DIR) && $(PYTHON) -m mkdocs build --strict

docs-serve: docs-version
	cd $(DOCS_DIR) && $(PYTHON) -m mkdocs serve -a 0.0.0.0:$(DOCS_PORT)

docs-versions-preview:
	@bash operator-docs/scripts/preview-versions-page.sh

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
	# Today: Rhai thread-local engine reuse only (expand later: dataplane, reload, scrape).
	$(CARGO) bench -p conduit-script --bench thread_local_runtime --features test-util

build:
	$(CARGO) build --workspace

# --- Interop correctness harness (local only) ---------------------------------

interop-image:
	docker build -t $(CONDUIT_IMAGE) -f $(DOCKERFILE) .

interop-unit:
	$(PYTHON) -m unittest discover -s interop/runner -p 'test_*.py'

interop-fingerprint:
	$(PYTHON) -m interop.runner fingerprint

interop-smoke:
	$(PYTHON) -m interop.runner run --suite smoke --conduit-image $(CONDUIT_IMAGE)

interop-auth:
	$(PYTHON) -m interop.runner run --case fixture-auth-a --conduit-image $(CONDUIT_IMAGE)

interop-docs:
	$(PYTHON) -m interop.runner generate-matrix

# Rebuild image, run smoke + graduated full cases across profiles, write results + docs.
interop-refresh: interop-image
	$(PYTHON) -m interop.runner run --suite smoke --profile forward-only \
		--conduit-image $(CONDUIT_IMAGE) --write-results --merge --generate-matrix
	$(PYTHON) -m interop.runner run --case fixture-auth-a --profile forward-only \
		--conduit-image $(CONDUIT_IMAGE) --write-results --merge --generate-matrix
	$(PYTHON) -m interop.runner run --case fixture-auth-nxdomain --profile forward-only \
		--conduit-image $(CONDUIT_IMAGE) --write-results --merge --generate-matrix
	$(PYTHON) -m interop.runner run --suite full --profile forward-only \
		--conduit-image $(CONDUIT_IMAGE) --write-results --merge --generate-matrix
	$(PYTHON) -m interop.runner run --suite full --profile cache-forward \
		--conduit-image $(CONDUIT_IMAGE) --write-results --merge --generate-matrix
	$(PYTHON) -m interop.runner run --suite full --profile forward-split-io \
		--conduit-image $(CONDUIT_IMAGE) --write-results --merge --generate-matrix

# --- Performance harness (local only; docs CI must not invoke load suites) ------

perf-unit:
	PYTHONPATH=. $(PYTHON) -m unittest discover -s perf/runner -p 'test_*.py'

perf-list:
	PYTHONPATH=. $(PYTHON) -m perf.runner list

perf-run-scale:
	PYTHONPATH=. $(PYTHON) -m perf.runner run --conduit $(CONDUIT) --suite scale --render plain

perf-run-shutdown-drain:
	PYTHONPATH=. $(PYTHON) -m perf.runner run --conduit $(CONDUIT) --suite shutdown_drain --render plain

perf-render:
	@test -n "$(PERF_FROM)" || (echo "Set PERF_FROM=path/to/run.json" >&2; exit 1)
	PYTHONPATH=. $(PYTHON) -m perf.runner render --from $(PERF_FROM) --format $(PERF_FORMAT)
