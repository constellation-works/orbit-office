.PHONY: help build test goldens fmt fmt-check clippy deny structure standards-check ci-fast ci-lint ci baseline gateway-check app-deps app-typecheck app-test app-check plugin-export install clean

CARGO ?= cargo
# Every build uses the committed Cargo.lock as is; a stale lock fails instead
# of being rewritten (STD-05 R23).
LOCKED := --locked
BINARY := office
INSTALL_BIN_DIR ?= $(HOME)/.cargo/bin
# `make goldens UPDATE=1` rewrites the golden files instead of comparing.
UPDATE ?=
# `STANDARDS_STRICT=1` makes a missing docs/standards/ a failure (CI sets it).
STANDARDS_STRICT ?=
# GitHub Actions sets CI=true. There a missing gate tool fails the gate;
# locally it warns and skips or falls back (STD-04 R10, decision in
# docs/design/office/4_decisions.md).
CI ?=

# $(call missing_tool,<tool>,<what it provides>): fail under CI, else warn.
define missing_tool
	if [ "$(CI)" = "true" ]; then \
		echo "error: $(1) is not installed; CI requires it for $(2)." >&2; exit 1; \
	fi; \
	echo "warning: $(1) not installed; continuing without $(2) (CI requires it)." >&2; \
	echo "         install: cargo install $(1) --locked" >&2
endef

help:
	@echo "office make targets"
	@echo ""
	@echo "  make ci-fast          Pre-handoff gate, no compile: fmt, structure, standards-check"
	@echo "  make ci-lint          clippy -D warnings + cargo-deny (skipped locally if not installed)"
	@echo "  make test             All tests (nextest when installed, else cargo test) + doctests"
	@echo "  make goldens          Compare --help and output goldens with the built binary"
	@echo "  make goldens UPDATE=1 Regenerate goldens after an intended surface change"
	@echo "  make standards-check  Run the vendored standards check (docs/standards/check.sh)"
	@echo "  make ci               Everything above; what CI runs"
	@echo "  make baseline         Every stack's gate: ci + gateway + app typecheck/tests"
	@echo "  make app-deps         Install app/node_modules from the lockfile when missing or stale"
	@echo "  make build | fmt | install | clean"

build:
	$(CARGO) build --workspace $(LOCKED)

# ---------------------------------------------------------------------------
# Gates
# ---------------------------------------------------------------------------

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

# Source-only structure checks: dependency direction (STD-02 R1-R6), unit-test
# module reachability (STD-02 R19), terminal/stream ownership (STD-01 R17),
# and the web app's import boundaries (ARCHITECTURE.md "Web app").
structure:
	./scripts/check-dependency-direction.sh
	./scripts/check-test-modules.sh
	./scripts/check-terminal-guard.sh
	python3 -I ./scripts/check-app-boundaries.py

clippy:
	$(CARGO) clippy --workspace --all-targets $(LOCKED) -- -D warnings

# Supply chain (STD-02 R23, STD-05 R23/R24): advisories, licenses, sources.
deny:
	@if $(CARGO) deny --version >/dev/null 2>&1; then \
		$(CARGO) deny check; \
	else \
		$(call missing_tool,cargo-deny,the supply-chain check); \
	fi

# nextest enforces per-test timeouts (.config/nextest.toml, STD-03 R21/R22)
# and fails a selection that matches no test (STD-04 R10); plain cargo test
# does neither, so the fallback says so. nextest does not run doctests, so
# they are a second leg; the core crate carries at least one (STD-04 R14).
test:
	@if $(CARGO) nextest --version >/dev/null 2>&1; then \
		$(CARGO) nextest run --workspace --all-targets $(LOCKED) --no-tests=fail; \
	else \
		$(call missing_tool,cargo-nextest,per-test timeouts and the zero-test check); \
		$(CARGO) test --workspace --all-targets $(LOCKED); \
	fi
	$(CARGO) test --workspace --doc $(LOCKED)

# Goldens of the shipped surface (STD-01 R24): captured from the built binary.
goldens:
	@if $(CARGO) nextest --version >/dev/null 2>&1; then \
		UPDATE_GOLDENS=$(if $(UPDATE),1,0) $(CARGO) nextest run -p $(BINARY) --test goldens $(LOCKED) --no-tests=fail; \
	else \
		$(call missing_tool,cargo-nextest,the zero-test check); \
		UPDATE_GOLDENS=$(if $(UPDATE),1,0) $(CARGO) test -p $(BINARY) --test goldens $(LOCKED); \
	fi

# The constellation standards this repo adopted are vendored under
# docs/standards/ by operations/scripts/sync-standards.sh, which also writes
# check.sh. Until then this warns and passes, so a fresh copy of the template
# builds; with STANDARDS_STRICT=1 (set in CI) a missing vendor fails.
standards-check:
	@if [ -f docs/standards/check.sh ]; then \
		sh docs/standards/check.sh; \
	elif [ "$(STANDARDS_STRICT)" = "1" ]; then \
		echo "error: standards not vendored yet (no docs/standards/check.sh)." >&2; \
		echo "       from the constellation root: operations/scripts/sync-standards.sh --adopt STD-01,STD-02,STD-03,STD-04,STD-05 <this repo>" >&2; \
		exit 1; \
	else \
		echo "warning: standards not vendored yet (no docs/standards/check.sh); skipping." >&2; \
		echo "         from the constellation root: operations/scripts/sync-standards.sh --adopt STD-01,STD-02,STD-03,STD-04,STD-05 <this repo>" >&2; \
	fi

# Pre-handoff gate for agents: no compile, and every parity check CI runs
# that needs no build (STD-04 R12). Goldens need the binary, so they are
# their own target.
ci-fast: fmt-check structure standards-check

ci-lint: clippy deny

# Full pass. Keep aligned with .github/workflows/ci.yml.
ci: ci-fast ci-lint test goldens

# Every stack's gate, as one command: the local prediction of the three
# required CI checks on main (ci, app, gateway). This is the list recorded
# as baseline_commands for the workspace.
baseline: ci gateway-check app-typecheck app-test

gateway-check:
	$(MAKE) -C gateway check test

# A fresh worktree has no app/node_modules. npm ci rewrites
# node_modules/.package-lock.json, so this reinstalls only when that is
# missing or older than the lockfile.
app-deps: app/node_modules/.package-lock.json

app/node_modules/.package-lock.json: app/package-lock.json
	npm --prefix app ci --prefer-offline --no-audit --no-fund

app-typecheck: app-deps
	npm --prefix app run check

app-test: app-deps
	npm --prefix app test

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------

install:
	$(CARGO) build --release --bin $(BINARY) $(LOCKED)
	install -d $(INSTALL_BIN_DIR)
	install -m 755 target/release/$(BINARY) $(INSTALL_BIN_DIR)/$(BINARY)

clean:
	$(CARGO) clean

# ---------------------------------------------------------------------------
# Web app (TypeScript) and plugin export — not part of the Rust CI gate.
# ---------------------------------------------------------------------------

app-check: app-deps
	npm --prefix app run check

plugin-export:
	$(MAKE) -C plugin export
