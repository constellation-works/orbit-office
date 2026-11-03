.PHONY: help build test goldens fmt fmt-check clippy deny deny-policy structure standards-check ci-fast ci-lint ci baseline gateway-check app-deps app-typecheck app-test app-audit app-check app-build app-install app-install-test plugin-export plugin-check plugin-install install clean qa-exec

CARGO ?= cargo
# Every build uses the committed Cargo.lock as is; a stale lock fails instead
# of being rewritten (STD-05 R23).
LOCKED := --locked
BINARY := office
INSTALL_BIN_DIR ?= $(HOME)/.cargo/bin
# `make goldens UPDATE=1` rewrites the golden files instead of comparing.
UPDATE ?=
# qa-exec fixture (docs/qa-exec.md): TOOL is the full tool name and INPUT a
# file holding that tool's input as one JSON object. QA_EXEC_ROOT reuses a
# fixture-created root for a chained call; QA_EXEC_OFFICE_BIN overrides the
# freshly built binary (used by the end-to-end test, which cargo has just
# built); ROOT is accepted as an alias for QA_EXEC_ROOT.
TOOL ?=
INPUT ?=
QA_EXEC_ROOT ?=
ROOT ?=
QA_EXEC_OFFICE_BIN ?=
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
	@echo "  make app-build        Build and stamp the development app"
	@echo "  make app-install      Build HEAD, update the registered app and verify its commit"
	@echo "  make plugin-install   Update the app and Orbit office plugin from the same HEAD"
	@echo "                        (PLUGIN_WORKSPACE=<w> also enables it in that workspace"
	@echo "                         and runs office.init there, so new templates and"
	@echo "                         layout migrations arrive with the upgrade)"
	@echo "  make qa-exec TOOL=<tool> INPUT=<file>  Run one office tool in a disposable temp root (docs/qa-exec.md)"
	@echo "  make build | fmt | install | clean"

build:
	$(CARGO) build --workspace $(LOCKED)

# Disposable-data QA fixture; see docs/qa-exec.md and the test-fixture
# exception in docs/design/office/4_decisions.md (STD-05@1 §R3-R4,
# ORB-20946). Not an authority route: the context carries only the three
# root keys inside a temp root this target creates, no identity claim,
# no ORBIT_OPERATOR, and Orbit callbacks are unavailable.
qa-exec:
	@test -n "$(TOOL)" || { echo "error: make qa-exec requires TOOL=<tool> (for example TOOL=office.init)." >&2; exit 2; }
	@test -n "$(INPUT)" || { echo "error: make qa-exec requires INPUT=<file> holding the tool input as one JSON object." >&2; exit 2; }
	@test -f "$(INPUT)" || { echo "error: INPUT file '$(INPUT)' does not exist." >&2; exit 2; }
	@if [ -z "$(QA_EXEC_OFFICE_BIN)" ]; then $(CARGO) build $(LOCKED) --bin $(BINARY); fi
	@set -eu; \
	bin="$(QA_EXEC_OFFICE_BIN)"; \
	if [ -z "$$bin" ]; then bin="target/debug/$(BINARY)"; fi; \
	case "$$bin" in /*) ;; *) bin="$$PWD/$$bin" ;; esac; \
	test -x "$$bin" || { echo "qa-exec: error: office binary '$$bin' is not executable." >&2; exit 2; }; \
	supplied="$(QA_EXEC_ROOT)"; \
	if [ -z "$$supplied" ]; then supplied="$(ROOT)"; fi; \
	tmpbase=$$(python3 -c 'import os; print(os.path.realpath(os.environ.get("TMPDIR") or "/tmp"))'); \
	if [ -n "$$supplied" ]; then \
		root=$$(python3 -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$$supplied"); \
		case "$$root" in "$$tmpbase"/*) ;; *) echo "qa-exec: error: root '$$supplied' is outside its temp directory '$$tmpbase'; refusing." >&2; exit 2 ;; esac; \
		test -f "$$root/qa-exec-audit.log" || { echo "qa-exec: error: root '$$supplied' is not a qa-exec fixture root (no qa-exec-audit.log); refusing." >&2; exit 2; }; \
	else \
		root=$$(mktemp -d "$${TMPDIR:-/tmp}/office-qa-exec.XXXXXX"); \
		root=$$(python3 -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$$root"); \
	fi; \
	mkdir -p "$$root/workspace" "$$root/plugin_state" "$$root/home" "$$root/tmp"; \
	audit="qa-exec audit tool=$(TOOL) root=$$root time=$$(date -u +%Y-%m-%dT%H:%M:%SZ) fixture=make-qa-exec"; \
	printf '%s\n' "$$audit" >> "$$root/qa-exec-audit.log"; \
	printf '%s\n' "$$audit"; \
	input="$(INPUT)"; \
	case "$$input" in /*) ;; *) input="$$PWD/$$input" ;; esac; \
	printf '%s\n' \
		'import json, os, sys' \
		'inp, root, tool, out = sys.argv[1:5]' \
		'data = json.load(open(inp, encoding="utf-8"))' \
		'if not isinstance(data, dict): sys.exit("qa-exec: error: INPUT must hold one JSON object; refusing.")' \
		'stack = [data]' \
		'while stack:' \
		'    v = stack.pop()' \
		'    if isinstance(v, dict): stack.extend(v.values())' \
		'    elif isinstance(v, list): stack.extend(v)' \
		'    elif isinstance(v, str):' \
		'        if os.path.isabs(v) and os.path.realpath(v) != root and not os.path.realpath(v).startswith(root + os.sep): sys.exit("qa-exec: error: input path %r is outside the fixture root %r; refusing." % (v, root))' \
		'        if ".." in v.split("/"): sys.exit("qa-exec: error: input path %r escapes the fixture root with ..; refusing." % v)' \
		'envelope = {"schema_version": 1, "tool": tool, "input": data, "context": {"workspace_root": root + "/workspace", "plugin_state": root + "/plugin_state", "library_dir": root + "/workspace/library"}}' \
		'json.dump(envelope, open(out, "w", encoding="utf-8"))' \
		> "$$root/qa_validate.py"; \
	python3 "$$root/qa_validate.py" "$$input" "$$root" "$(TOOL)" "$$root/request.json" || exit 2; \
	status=0; \
	env -i HOME="$$root/home" PATH="$$PATH" LANG=C.UTF-8 TMPDIR="$$root/tmp" ORBIT_BIN=/bin/false "$$bin" orbit-tool < "$$root/request.json" > "$$root/response.json" || status=$$?; \
	cat "$$root/response.json"; \
	printf '\n'; \
	exit $$status

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

# The supply-chain policy without advisories: licenses, bans and sources only
# change with Cargo.toml or Cargo.lock, so the result is a function of the
# tree. Advisories are published outside the repo and can turn an unchanged
# base red; CI (make ci) still checks them on every PR and on main.
deny-policy:
	@if $(CARGO) deny --version >/dev/null 2>&1; then \
		$(CARGO) deny check bans licenses sources; \
	else \
		$(call missing_tool,cargo-deny,the supply-chain policy check); \
	fi

# nextest enforces per-test timeouts (.config/nextest.toml, STD-03 R21/R22)
# and fails a selection that matches no test (STD-04 R10); plain cargo test
# does neither, so the fallback says so. nextest does not run doctests, so
# they are a second leg; the core crate carries at least one (STD-04 R14).
test:
	$(MAKE) app-install-test
	./scripts/plugin/test-export.sh
	./scripts/plugin/test-plugin-install.sh
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

# Advisory gate (STD-05 R24): fail on known npm advisories in
# app/package-lock.json. Mirrors the audit step in .github/workflows/ci.yml.
app-audit: app-deps
	npm --prefix app audit --audit-level=moderate

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------

# Every permission the manifest requests, so an upgrade that widens a request
# re-consents instead of leaving the plugin disabled.
PLUGIN_NAME := office
PLUGIN_GRANTS ?= --grant 'fs={{workspace}},{{plugin_state}}' --grant network --grant orbit_tools --grant agent_tools
# `make plugin-install PLUGIN_WORKSPACE=office-constellation` also enables the
# plugin in that workspace and runs office.init there, so a later plugin
# version's new templates and layout migrations arrive on the upgrade; left
# empty, the workspace toggle is untouched and office.init is not run.
PLUGIN_WORKSPACE ?=
APP_INSTALL_REVISION ?= HEAD

app-build: app-deps
	npm --prefix app run build
	python3 -B app/install/app_install.py stamp app "development-$$(git rev-parse --verify 'HEAD^{commit}')"

app-install:
	python3 -B app/install/app_install.py install "$(APP_INSTALL_REVISION)"

app-install-test:
	python3 -B -m unittest discover -s app/install -p 'test_*.py'

install:
	$(CARGO) build --release --bin $(BINARY) $(LOCKED)
	install -d $(INSTALL_BIN_DIR)
	install -m 755 target/release/$(BINARY) $(INSTALL_BIN_DIR)/$(BINARY)

# Build HEAD into a fresh export, validate it, then upgrade the installed
# plugin (or add it on first install) and enable it with PLUGIN_GRANTS.
# HEAD, not the working tree: commit first.
plugin-install:
	@command -v orbit >/dev/null 2>&1 || { echo "error: orbit is not installed." >&2; exit 1; }
	@set -eu; \
	rev=$$(git rev-parse --verify 'HEAD^{commit}'); \
	$(MAKE) app-install APP_INSTALL_REVISION="$$rev"; \
	parent=$$(mktemp -d "$${TMPDIR:-/tmp}/office-plugin-install.XXXXXX"); \
	out="$$parent/$(PLUGIN_NAME)-plugin-$$(git rev-parse --short "$$rev")"; \
	trap 'rm -rf "$$parent"' EXIT HUP INT TERM; \
	$(MAKE) -C plugin export OUT="$$out" PLUGIN_REVISION="$$rev"; \
	orbit plugin validate "$$out"; \
	if orbit plugin show $(PLUGIN_NAME) >/dev/null 2>&1; then \
		orbit plugin upgrade $(PLUGIN_NAME) "$$out" $(PLUGIN_GRANTS); \
	else \
		orbit plugin add "$$out"; \
		orbit plugin enable $(PLUGIN_NAME) $(PLUGIN_GRANTS); \
	fi; \
	if [ -n "$(PLUGIN_WORKSPACE)" ]; then \
		orbit plugin enable $(PLUGIN_NAME) --scope workspace --workspace "$(PLUGIN_WORKSPACE)"; \
		echo "plugin-install: running office.init in workspace $(PLUGIN_WORKSPACE)"; \
		init_status=0; \
		if [ -n "$${ORBIT_MANAGED_RUN_CONTEXT:-}" ] || [ "$${ORBIT_TASK_ACTOR_KIND:-}" = "agent" ]; then \
			orbit tool run office.init --workspace "$(PLUGIN_WORKSPACE)" --json --full --input '{}' > "$$parent/office-init.out" || init_status=$$?; \
		else \
			ORBIT_OPERATOR=1 orbit tool run office.init --workspace "$(PLUGIN_WORKSPACE)" --json --full --input '{}' > "$$parent/office-init.out" || init_status=$$?; \
		fi; \
		cat "$$parent/office-init.out"; \
		if [ "$$init_status" -ne 0 ]; then \
			echo "error: office.init failed in workspace $(PLUGIN_WORKSPACE) (exit: $$init_status)." >&2; exit "$$init_status"; \
		fi; \
		if grep -Eq '"ok"[[:space:]]*:[[:space:]]*false' "$$parent/office-init.out"; then \
			echo "error: office.init returned an error envelope in workspace $(PLUGIN_WORKSPACE)." >&2; exit 1; \
		fi; \
	fi; \
	orbit plugin show $(PLUGIN_NAME) | sed -n '1,7p'

clean:
	$(CARGO) clean

# ---------------------------------------------------------------------------
# Web app (TypeScript) and plugin export — not part of the Rust CI gate.
# ---------------------------------------------------------------------------

app-check: app-deps
	npm --prefix app run check

plugin-export:
	$(MAKE) -C plugin export

# Per-candidate plugin validation (ORB-19099): export HEAD to a temp dir and
# run `orbit plugin validate` on the merged result, so a broken plugin tree
# fails the candidate instead of every later task. Warnings pass; errors fail.
# Without `orbit` on PATH this warns (same shape as `missing_tool`) and exits
# 0 even under CI=true, so GitHub CI (which lacks the binary) stays usable.
# Not part of `ci`/`ci-fast`: CI cannot run it without the binary.
plugin-check:
	@if ! command -v orbit >/dev/null 2>&1; then \
		echo "warning: orbit not installed; continuing without the plugin check (CI requires it)." >&2; \
		echo "         install: npm install -g @orbit-tools/cli@0.34.0" >&2; \
		exit 0; \
	fi; \
	set -eu; \
	parent=$$(mktemp -d "$${TMPDIR:-/tmp}/office-plugin-check.XXXXXX"); \
	out="$$parent/export"; \
	trap 'rm -rf "$$parent"' EXIT HUP INT TERM; \
	$(MAKE) -C plugin export OUT="$$out"; \
	status=0; orbit plugin validate "$$out" || status=$$?; \
	rm -rf "$$parent"; trap - EXIT HUP INT TERM; \
	exit $$status
