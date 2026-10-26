---
title: office — Decisions
owner: daniel
last_updated: 2026-10-26
last_validated: 2026-10-26
status: Draft
feature: office
doc_role: decisions
type: design
summary: Why two crates, why clap has no color, how usage errors render, how the gate checks behave, and the office decisions: a CLI orchestrator, approval by tool visibility, the app as user, the shipped stacks, and typed selections.
tags: [office, decisions]
paths: ["Cargo.toml", "crates/**", "Makefile", "plugin/**", "app/**"]
related_features: [office]
related_artifacts: [ORB-13117, ORB-13134, ORB-16585]
---

# office — Decisions

Record non-obvious decisions here by title. Task references carry provenance; superseded decisions remain in place so their original reasoning stays legible. See [CONVENTIONS.md §3](../CONVENTIONS.md#3-what-earns-a-decision-entry) for the admission rule and required `Cost:` line.

## A domain library crate and a CLI crate

**Recorded:** 2026-09 · [ORB-13117]
**Code anchors:** `Cargo.toml` `[workspace] members`; `scripts/check-dependency-direction.sh::policy`

### Context

STD-02@1 §R7 said to start as one crate and split only for a build or
dependency need (STD-02@2 §R7 has since made two crates the default for a new
CLI, for the reason below). A single crate can express the layers as modules with grep
bans. The template, though, exists to be copied by agents into projects that
grow, and the edge that matters most — the domain must not know about
argument parsing, terminals or log subscribers — is exactly "a dependency
edge that must be enforceable", R7's own justification for a split.

### Decision

Two crates: `office-core` (library; protocol domain) and
`office` (binary; composition and surface). Cargo makes clap unreachable
from the core rather than merely banned, and the core is ready to be consumed
by a second surface (MCP server, desktop app) without restructuring. Further
crates still need an R7 reason; the default for new code is a module in one
of these two. A project that is certain to stay a single small binary may
collapse to one crate with STD-02's single-crate check.

### Consequences

- The domain/surface boundary is enforced by the compiler plus
  `check-dependency-direction.sh`, not by review.
- Cost: two manifests, a `[workspace.dependencies]` table, and a crate
  boundary that forces `pub` on the core's API where one crate would use
  `pub(crate)`.

## clap without color or terminal-width features

**Recorded:** 2026-09 · [ORB-13117]
**Code anchors:** `Cargo.toml` `[workspace.dependencies] clap`

### Context

STD-01 R17 requires the color/TTY decision in exactly one place. clap's
default `color` feature and the `wrap_help` feature each query the terminal
themselves, and a terminal-dependent help rendering would also make the help
goldens depend on where they were captured.

### Decision

clap is built with `default-features = false` and without `color` or
`wrap_help`. Help and usage errors are plain text everywhere; styling, if it
is ever wanted, is applied from the sink's decision.

### Consequences

- Help renders byte-identically on every machine, so goldens are stable.
- Cost: no colored help and no wrapping of long help lines to the terminal.
## Usage errors keep clap's rendering

**Recorded:** 2026-09 · [ORB-13117]
**Code anchors:** `crates/office/src/output/mod.rs::finish_parse_error`

### Context

STD-01@1 §R19 says a non-JSON error is a single `error: <message>` line.
clap's usage errors are an `error:` line followed by a tip or did-you-mean
suggestion, the usage line and a pointer to `--help` — the actionable detail
R21 asks for.

### Decision

Deviation from STD-01@1 §R19 for usage errors only: in human mode they keep
clap's multi-line rendering (first line `error: …`, exit 2). In JSON mode they
are one `{"error","code":"usage_error"}` object like every other error.
Command failures follow R19 exactly.

### Consequences

- Wrappers still branch on exit code 2 and the stable `code`.
- Cost: a script reading human-mode stderr gets several lines for a usage
  error and one line for a command failure.
- Update [ORB-13134]: STD-01@2 §R19 allows exactly this rendering, so it is
  no longer a deviation.
## standards-check warns locally and fails in CI

**Recorded:** 2026-09 · [ORB-13117]
**Code anchors:** `Makefile` `standards-check`; `.github/workflows/ci.yml` `STANDARDS_STRICT`

### Context

The adopted standards are vendored into `docs/standards/` by the
constellation's `sync-standards.sh`, which may run after the repository is
created. A fresh copy must still pass `make ci`, but a real project whose
standards were never vendored should not pass CI.

### Decision

Without `docs/standards/check.sh`, `make standards-check` prints a warning
and the command to run, and exits 0 — unless `STANDARDS_STRICT=1`, which CI
sets, and then it fails.

### Consequences

- The template and a just-created project pass `make ci` locally.
- Cost: a local `make ci` can pass while CI fails, until the standards are
  vendored.
## Missing gate tools warn locally and fail in CI

**Recorded:** 2026-09 · [ORB-13134]
**Code anchors:** `Makefile` `missing_tool`, `test`, `goldens`, `deny`, `ci-fast`

### Context

STD-04@1 §R10 says a gate whose tool is missing fails, and §R12 puts every
parity check CI runs into the fast local gate, except those that need a
build. `cargo-nextest` (per-test timeouts, `--no-tests=fail`) and
`cargo-deny` are not installed everywhere a copy of this template is built.

### Decision

Deviation from STD-04@1 §R10 for local runs only: without `cargo-deny` the
supply-chain check is skipped, and without `cargo-nextest` the tests run
under `cargo test`, each with a warning naming the install command. Under
`CI=true` (set by GitHub Actions) either missing tool fails the gate, and CI
installs both at pinned versions. `ci-fast` runs formatting, the structure
checks and `standards-check`. Two checks stay out of it under STD-04@1 §R12
because they need a build: the help and output goldens (`make goldens`) and
Cargo.lock freshness (`--locked` on every cargo build).

### Consequences

- A fresh clone passes `make ci` with only cargo installed, and CI cannot
  pass with a gate missing.
- Cost: a local green `make ci` without those tools is weaker than CI's.

## The orchestrator is the user's agent CLI, not a built agent

**Recorded:** 2026-10-26
**Code anchors:** `plugin/.orbit-plugin/skills/orchestrator/SKILL.md`; `specs/orchestrator.md`

### Context

The first draft of the spec built an orchestrator agent into the app
server, with a Chat screen. The user's real workflow already has an agent
CLI open (Muse Code by default; Claude Code and Codex also speak MCP), and
Daniel's own constellation setup is exactly an agent session supervising
Orbit.

### Decision

Ship no agent loop. The orchestrator is the user's agent CLI running the
`office-orchestrator` skill over Orbit MCP at agent authority. What this
project builds is the skill, the tool surface and its guardrails. The web
app has no Chat screen.

### Consequences

- One conversation surface, on the user's chosen provider; the demo keeps
  it on Meta models through Muse Code.
- Cost: behaviour depends on the host CLI's skill and hook support, so
  skill discovery and prompt hooks are day-1 checks, and the selection
  pull rule must work without any hook.

## Approval is enforced by tool visibility, not by instructions

**Recorded:** 2026-10-26
**Code anchors:** `plugin/.orbit-plugin/plugin.yaml` (`mcp_scope` per tool); `specs/plugin-surface.md`

### Context

"Propose, don't impose" could be a skill instruction. Instructions are
advisory: the same agent can be told, or can decide, to write directly.
Orbit plugin tools declare `mcp_scope`, and `none` keeps a tool off MCP
while `orbit tool run` can still call it.

### Decision

The approval and write tools (`knowledge.write`, `proposal.accept`,
`artifact.promote`, `selection.put`, `reindex`) are `mcp_scope: none`.
Only the app — acting as the user through `orbit tool run` — can call
them. The orchestrator can propose; it cannot approve or write.

### Consequences

- The guardrail holds regardless of prompt wording.
- Cost: an interactive agent CLI with shell access could call
  `orbit tool run` itself. The backstops are its shell approval prompt and
  the audit trail; both are stated, not hidden, in
  [2_design.md](./2_design.md#7-concerns--honest-limitations).

## The app acts as the user through `orbit tool run`

**Recorded:** 2026-10-26
**Code anchors:** `app/src/server/`; `specs/web-app.md`

### Context

The app needs every tool, including the MCP-hidden ones. Orbit's HTTP API
serves its own dashboard and exposes plugin tools only as read-only panel
sources; MCP at agent authority deliberately excludes approval and
dispatch.

### Decision

The app server shells to `orbit tool run --json` as the user, and owns
the dispatch pass (`orbit run ship <ids>` every 30 seconds for
`owner:agent` backlog tasks). The office workspace never runs an auto
drain, so `owner:you` to-dos and `proposed` proposals are never picked up
by automation.

### Consequences

- One authority model: the app is the user, the CLI agent is an agent.
- Cost: a subprocess per tool call, and the app must run as a login
  service for dispatch to continue with the browser closed.

## Rust backend and JavaScript app, not the spec's proposed stacks

**Recorded:** 2026-10-26
**Code anchors:** `Cargo.toml`, `crates/`; `app/package.json`

### Context

Spec §5 proposed a Python (`uv`) plugin for the quickest path to audio,
vision and HTTP SDKs, and a TypeScript web app. Daniel directed the
shipped stacks on 2026-10-26: Rust, from the constellation Rust CLI
template, and JavaScript for the web.

### Decision

Deviation from the spec's proposed stacks: the backend is the two-crate
Rust workspace generated from `constellation-works/rust-cli-template`
(`office-core` + `office`), and the web app is plain JavaScript with no
build step. Model and transcription calls go over plain HTTPS from Rust;
no Python SDK is required.

### Consequences

- The template's standards, gates and goldens govern the backend from the
  first commit.
- Cost: no Python SDK convenience for the Meta APIs; request shapes are
  hand-built and must be verified on day 1 (spec §18.5).

## Selections are typed references, and private selections carry no quote

**Recorded:** 2026-10-26
**Code anchors:** `specs/selection-sharing.md`

### Context

The user points at app content and refers to it in the agent CLI. A DOM
path would break with each UI change and mean nothing to the agent; a
copied quote from a `private` source would send private text to a hosted
model, bypassing the privacy routing.

### Decision

A selection is a typed reference (`object`, `anchor`, `view`) plus a
quote and its `sha256` — except for private objects, where the quote is
omitted and the agent must re-read through the office tools. Selections
live in `plugin_state/selections.sqlite`, expire after 15 minutes or one
use, and the tray holds at most five.

### Consequences

- The reference survives UI changes, and private material stays behind
  the routing rule.
- Cost: the agent spends a tool call to read private content the user is
  already looking at.

## office-store is its own crate, structured after orbit-store

**Recorded:** 2026-10-26
**Code anchors:** `crates/office-store/`; `scripts/check-dependency-direction.sh` (`office-store` policy); [specs/store.md](./specs/store.md)

### Context

The Library needs a file driver, a rebuildable SQLite FTS index, atomic
writes, path safety and schema migrations. Orbit's persistence crate
(`orbit-store`, with `orbit-common`'s SQLite helpers) already embodies
that machinery, but Orbit is a labelled dependency whose code must not be
vendored into this repo. The alternative shapes were a persistence module
inside `office-core` (the template's original pattern) or this separate
crate.

### Decision

Create `crates/office-store`, mirroring orbit-store's structure —
contracts, `fs` mechanisms, private file/SQLite drivers behind a
repository facade, one writer connection, ordered migration registry —
re-implemented at office scale. It depends on no internal crate; the
`office` composition layer is allowed to depend on it and will open it
from the call context when the first Library tool lands.

### Consequences

- The persistence boundary is compiler-enforced, and `office-core` stays
  I/O-free.
- Cost: a third crate and a second typed error boundary to translate at
  the surface when the wiring lands. orbit-store machinery this crate
  omits (read pool, forward-compatible open, contention classes) must be
  re-added deliberately if scale ever needs it.

## Template decisions not carried over

The template's note-store decisions (persistence module, one module per noun, audit guard, `--json` alias, list envelope, store lock, store format registry, owner-only store state, malformed-note handling, environment fallbacks) were removed with the note domain. They return, rewritten for the office domain, only when the feature they govern lands.

## Task References

- [ORB-16585] — files the orchestrator assignment: rules, go/no-go, coordination, demo and submission for the office-work entry.
- [ORB-13117] — created the Rust CLI template this workspace was generated from.
- [ORB-13134] — hardened the template's gates and goldens.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
