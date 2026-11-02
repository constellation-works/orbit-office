---
title: office — Decisions
owner: daniel
last_updated: 2026-11-01
last_validated: 2026-11-01
status: Draft
feature: office
doc_role: decisions
type: design
summary: Why two crates, why clap has no color, how usage errors render, how the gate checks behave, and the office decisions: a CLI orchestrator, approval by tool visibility, the app as user, the shipped stacks, a PWA rather than a desktop app, typed selections, the Library as its own repository, where settings live, the layout built for parallel work, and how sessions hand off.
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
- Update (2026-10-26): the workspace grew to seven crates for parallel work; see
  [A crate per enforceable edge, a folder per tool, and every shared list written once](#a-crate-per-enforceable-edge-a-folder-per-tool-and-every-shared-list-written-once).
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

## The installer registers Muse Code with `--agent muse`

**Recorded:** 2026-10-29 · [ORB-19751]
**Code anchors:** `install.sh` (`--agent` validation, the `muse` agent case, `edit_muse_settings`, `step_agent`, the uninstall dispatch); `scripts/install/test-install.sh` §23; [specs/install.md](./specs/install.md)

### Context

Muse Code is the default orchestrator provider and the demo provider
([The orchestrator is the user's agent CLI, not a built agent](#the-orchestrator-is-the-users-agent-cli-not-a-built-agent)),
and `orbit-office status` already recognised an Orbit MCP entry in
`${XDG_CONFIG_HOME:-~/.config}/muse/settings.json`. But `install.sh
--agent` accepted only `claude` (its default) or `codex`, so the default
provider could only be registered by hand, while `2_design.md` claimed
the install script wrote the Muse entry. The alternative was a docs-only
correction demoting Muse Code to manual setup.

### Decision

Add `--agent muse`, rather than correcting the docs down. The installer
keeps `claude` as its own `--agent` default and registers one CLI per
run: `claude`, `codex` or `muse`. For Muse Code it splices
`mcpServers.orbit` (`orbit mcp serve`, with the generated `PATH`) into
`${XDG_CONFIG_HOME:-~/.config}/muse/settings.json`, preserving every
other byte and setting, keeping a `.bak`, and recording ownership in
`<data>/muse-mcp.json` so uninstall removes only an installer-owned
entry and keeps a pre-existing identical one. Office skills reach Muse
Code as `office-` plugin links in `$HOME/.agents/skills`, as for the
other CLIs (see [Office skills come from the plugin, not installer
copies](#office-skills-come-from-the-plugin-not-installer-copies)).
Muse Code has no per-command shell deny settings, so no
deny rules are written and the summary reports that, as it does for
Codex. "Default" is now pinned in both senses: Muse Code is the default
orchestrator provider; `claude` is the installer's `--agent` default.

### Consequences

- The documented default provider is installable by the script that
  claims to register it, and `install.sh --help`, `specs/install.md`,
  `specs/orchestrator.md`, `2_design.md` and the orchestrator skill's
  setup reference agree on the registered CLIs and both defaults.
- Cost: a third settings editor in `install.sh` (the Muse JSON splice,
  beside the Claude and Codex paths) and its harness coverage, and Muse
  users get no installer-written deny rules, only the skill's gateway
  instruction.

## Office skills come from the plugin, not installer copies

**Recorded:** 2026-10-30 · [ORB-20228]
**Code anchors:** `scripts/install/lib/agent.sh` (`step_agent`, `migrate_bare_skill_copies`, `install_skill_fallback`); `scripts/install/test-install.sh` §15, §24

### Context

The Orbit plugin links each office skill as `office-<name>` into the
agent's skills directory, tracking the installed plugin. The installer's
agent step also copied every plugin skill under its bare name and never
refreshed an existing destination, so each skill appeared twice and the
copy went stale.

### Decision

The plugin's links are the one source of truth. When the plugin provides
the skills the installer copies nothing. A fallback copy, used only when
no Orbit plugin path provides them, uses the same `office-` names and
refreshes a stale recorded copy. On re-run, receipt-recorded bare-named
copies are removed when unchanged and kept with a warning when modified.

### Consequences

- Each office skill appears once per agent, under its `office-` name.
- Cost: the installer keeps receipt, digest and migration machinery for
  the fallback copies it may still make and for legacy bare copies.

## Approval is enforced by tool visibility, not by instructions

**Recorded:** 2026-10-26
**Code anchors:** `plugin/.orbit-plugin/plugin.yaml` (`mcp_scope` per tool); `specs/plugin-surface.md`

### Context

"Propose, don't impose" could be a skill instruction. Instructions are
advisory: the same agent can be told, or can decide, to write directly.
Orbit plugin tools declare `mcp_scope`, and `none` keeps a tool off MCP
while `orbit tool run` can still call it.

### Decision

The approval and write tools (`init`, `knowledge-write`,
`proposal-accept`, `artifact-promote`, `selection-put`, `config-set`,
`reindex`) are `mcp_scope: none`. Only the app — acting as the user
through `orbit tool run` — can call them, and the install script calls
`init`. The orchestrator can propose; it cannot approve or write.

### Consequences

- The guardrail holds regardless of prompt wording.
- Cost: an interactive agent CLI with shell access could call
  `orbit tool run` itself. The backstops are its shell approval prompt and
  the audit trail; both are stated, not hidden, in
  [2_design.md](./2_design.md#7-concerns--honest-limitations).

## The app acts as the user through `orbit tool run`

**Recorded:** 2026-10-26
**Code anchors:** `app/packages/server/src/`; `specs/web-app.md`

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

## Rust backend (not Python) and a TypeScript app

**Recorded:** 2026-10-26
**Code anchors:** `Cargo.toml`, `crates/`; `app/package.json`, `app/tsconfig.json`

### Context

Spec §5 proposed a Python (`uv`) plugin for the quickest path to audio,
vision and HTTP SDKs. Daniel directed the shipped stacks on 2026-10-26:
Rust, from the constellation Rust CLI template. The web app briefly went
to plain JavaScript and was switched back the same day: most app code
will be agent-written under time pressure, and `tsc --strict` is a
cheaper reviewer than runtime failures, while the switch was still four
stub files.

### Decision

Deviation from the spec's proposed plugin stack only: the backend is the
two-crate Rust workspace generated from
`constellation-works/rust-cli-template` (`office-core` + `office`). The
web app is TypeScript as the spec proposed — strict `tsc`, no bundler or
framework; `tsx` only watches sources in dev. Model and transcription
calls go over plain HTTPS from Rust; no Python SDK is required.

### Consequences

- The template's standards, gates and goldens govern the backend from the
  first commit; the app's gate is `tsc --noEmit` (plus runtime validation
  at the subprocess boundary — types do not validate arriving JSON).
- Cost: no Python SDK convenience for the Meta APIs; request shapes are
  hand-built and must be verified on day 1 (spec §18.5). The app regains
  one build step.

## The Library is its own git repository, and knowledge reaches agents as task artifacts

**Recorded:** 2026-10-26
**Code anchors:** `plugin/.orbit-plugin/plugin.yaml` `permissions.fs.write`; [specs/library.md](./specs/library.md#git)

### Context

The spec first made the workspace repository the Library repository, with
the plugin committing after each write and tasks attaching pages through
`context_files`. Three Orbit facts break that (spec §18.7). Orbit refuses
a plugin write grant on the workspace's `.git`, so the plugin cannot
commit there. Plugin writes to tracked files in the primary checkout
would fail running agent tasks with `primary_checkout_drift`. And runs
work in a worktree pinned at a commit, so they see only what is
committed.

### Decision

The Library is a separate git repository at `library/`, created by
`office.init` inside the plugin's `{{workspace}}/library` write grant;
the workspace repository ignores `/library/`. `office.attach` copies the
chosen pages into the task as task artifacts under `context/`, with the
Library commit they were read at, and agents read them with
`orbit.task.artifact.get`. `context_files` are not used for Library
content.

### Consequences

- The plugin owns its repository end to end, and Library writes never
  disturb Orbit runs.
- Cost: two repositories in one folder; the Library's history is not in
  the workspace's, and a backup must cover both. Attachments are copies,
  so an agent sees the page as of the attach, and `office.attach` becomes
  a mutating tool.

## Plugin config holds only `library_dir`; Library settings live in the Library

**Recorded:** 2026-10-26
**Code anchors:** `plugin/.orbit-plugin/schemas/config.json`; [specs/plugin-surface.md](./specs/plugin-surface.md#config-and-permissions)

### Context

The spec listed retention, sensitivity defaults, packs, signal thresholds
and the open-weights endpoint both in `[plugins.office]` and in
`.office/config.yaml`, which leaves two sources of truth for the same
values.

### Decision

`[plugins.office]` holds only `library_dir`, the one value needed to find
the Library. Every other setting lives in `library/.office/config.yaml`,
written with defaults by `office.init` and changed by the app through
`office.config-set` (validated, then committed). Secrets stay in Orbit's
secret store.

### Consequences

- Settings are versioned with the Library, travel with it, and are read
  from one place.
- Cost: Orbit's config schema can no longer validate these values, so
  the plugin validates `config.yaml` itself on every read, and a hand
  edit that breaks it fails every tool until it is fixed.

## A local web app installable as a PWA; the desktop shell waits

**Recorded:** 2026-10-26
**Code anchors:** `app/`; [specs/web-app.md](./specs/web-app.md#delivery)

### Context

The audience is office workers, and the README's onboarding risk is a
demo that shows a terminal. A desktop shell (Tauri, which suits the Rust
core) would give one installer, a tray icon, native notifications and
native drag-and-drop. It also brings packaging, macOS signing and
notarization and auto-update into a nine-day solo build, none of which
is judged, and the demo video looks the same either way.

### Decision

For the event the app is a local web app made installable as a PWA (a
web app manifest plus a service worker that caches the app shell only;
no offline mode). Installed from Chrome or Edge, or added to the Dock
in Safari, it opens in its own window without browser chrome. A Tauri
shell is post-event work. Until then the UI stays webview-safe: relative
URLs, and no browser-only API without a fallback.

### Consequences

- Citations are plain `http://localhost` links that the browser opens.
  They carry anchors (`#t=…` for meetings, `#L…` for line-based sources).
  The Source screen now opens query-selected sources at those anchors,
  highlighting and scrolling their spans (ORB-18729). The
  "no deep-link scheme" line originally recorded here was superseded on
  2026-10-26 (see "Anchor deep-links for citations" below). The dispatch
  pass is a login service (launchd or a systemd user unit) independent
  of the UI.
- Cost: users install Node 20+ themselves, there is no tray or native
  notification for Needs you, and the later Tauri shell must either ship
  the Node server as a sidecar or move it into the `office` binary.

## Anchor deep-links for citations

**Recorded:** 2026-10-26 (owner decision, superseding the PWA decision's
"no deep-link scheme" consequence)
**Code anchors:** `docs/design/office/specs/web-app.md`; `docs/design/office/specs/selection-sharing.md`

### Context

ORB-18729 (Source view, upload and paste capture) requires citation URLs
that open a source and scroll to the cited span. The PWA decision above
recorded that no deep-link scheme was needed yet. Citations, provenance
links and selection sharing all already address sources by anchor, so a
link that cannot land on the anchor makes the provenance promise weaker
at exactly the moment the user checks it.

### Decision

Citation links are `http://localhost` URLs carrying the source anchor
(`#t=hh:mm:ss` for meetings, `#L<start>-<end>` for line-based bodies).
The Source screen now opens `/?source=<id>` with that fragment, scrolls
and highlights the span, and seeks playable media. The same query-selected
screen accepts `#frame=hh:mm:ss` and `#chapter=<n>`. The query keeps source
identity separate from the anchor, while ordinary navigation hashes continue
to select screens. This ships with ORB-18729 without changing the shared
hash router.
This supersedes the "no deep-link scheme is needed yet" consequence in the
PWA decision; the rest of that decision stands.

### Consequences

- ORB-18729's anchor-link criterion is the required behaviour, not a
  conflict.
- Cost: the URL format is now a contract; changing anchor syntax means
  migrating links the orchestrator has already printed.

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

## office-common is the foundation crate, after orbit-common

**Recorded:** 2026-10-26
**Code anchors:** `crates/office-common/`; `crates/office-store/src/error.rs` (the `From<office_common::Error>` translator)

### Context

Once `office-store` existed, its fs mechanisms (path safety, atomic
writes, hashing) and the SQLite pragma set were exactly the kind of code
every future crate would copy. Orbit solves this with `orbit-common`: a
shared mechanism crate under everything, with domain contracts kept in
the owning crates.

### Decision

Create `crates/office-common` at tier 0: the common `Error`, `fs`
mechanisms, `storage::sqlite::apply_default_pragmas` behind an optional
`sqlite` feature (so non-SQLite consumers do not pull rusqlite), and
small shared types (`Sensitivity`, one definition for the store, the
model router and selection sharing). `office-store`'s fs machinery moved
into it and the store now depends on it, translating its error through
one `From` impl. Nothing internal may depend on a surface crate, and the
foundation depends on nothing internal.

### Consequences

- One definition per shared mechanism, enforced by the dependency-
  direction script.
- Cost: a fourth crate, and each consuming crate carries a small error
  translation at its boundary.

## A crate per enforceable edge, a folder per tool, and every shared list written once

**Recorded:** 2026-10-26
**Code anchors:** [ARCHITECTURE.md](../../../ARCHITECTURE.md); `crates/office-tools/src/registry.rs`; `scripts/check-dependency-direction.sh`; `scripts/check-app-boundaries.py`; `crates/office/tests/plugin_tree.rs`

### Context

The entry is built by many agents at once, dispatched by Orbit with file-footprint locks.
In the template layout every new tool edited the same files: the core's tool table, one
`tools.rs`, the manifest's `tools:` list and the app's single server and web entry files. Two
parallel tasks would conflict on all of them, and Orbit would serialize them. The other
choices were to keep that layout and accept serialized tasks, or to generate the shared
lists at build time (more machinery, and generated files still conflict).

### Decision

- **Crates follow enforceable edges (STD-02@3 §R7).** Four crates are added. `office-tools`
  (the application) depends only on `office-core`, so no tool can reach files, the network
  or Orbit except through a port, and every tool runs against fakes. `office-models` is the
  only crate allowed an HTTP client. `office-orbit` holds the Orbit callbacks. All three are
  edges the dependency check enforces. `office-testkit` is consumed only as a
  dev-dependency, by any crate's tests. `office-core` becomes the pure domain: envelope,
  tool contract types, domain model and port traits. Dispatch and handlers move to
  `office-tools`.
- **One folder per feature, one file per tool,** in each layer: `office-tools/src/tools/
  <family>/`, `app/packages/server/src/features/<f>/`, `app/packages/web/src/screens/<s>/`.
  Each maps to one spec under `specs/`, so a task's footprint is a few disjoint folders.
- **Every shared list is written once, up front.** The scaffold commit adds every planned
  tool to the registry, every family `mod.rs`, a commented block per planned tool in
  `plugin.yaml`, and every server feature and web screen to their routers. Later tasks edit
  only their own files, and a reserved manifest block is uncommented in place, so two
  tasks never touch the same hunk. These files are integrator-owned (ARCHITECTURE.md).
- **Planned is not advertised (STD-01@2 §R36).** A planned tool has no handler: dispatch
  refuses it as `unknown_tool` and the manifest lists only built tools. `plugin_tree.rs`
  checks both. A planned feature mounts no routes and a planned screen is not in the
  navigation.
- **The app is npm workspaces** (`contracts`, `server`, `web`) joined by TypeScript project
  references. Web and server cannot import each other, and feature and screen folders are
  checked by `scripts/check-app-boundaries.py`.

**Deviation from STD-02@3 §R17 (fewest moving parts), scoped to the scaffold.** Planned
tool modules, reserved domain, port and adapter modules, and planned features and screens
exist before the code that uses them. Each holds only its contract (a `ToolSpec`, a feature
or screen record) and its documentation, no logic. Scope: the hackathon build window. Any
module still planned at submission (2026-11-04) is deleted with its registry row, reserved
manifest block and router entry, and its path is added to the retired-paths list.

### Consequences

- Feature tasks run in parallel with disjoint footprints. Only integrator tasks touch the
  shared files.
- Port interfaces and fakes are declared for dispatch before adapters; operations arrive
  with each adapter and its fake, so there are no speculative signatures to undo.
- Cost: seven crates instead of four, and more manifests to keep in step. There are
  reserved modules that do nothing yet (the §R17 deviation). Commented manifest blocks have
  to be kept in their slots. The app gains a build step for project references.

## Session continuity from a brief, a decision log and a digest routine

**Partly superseded 2026-10-30:** transcript extraction below replaces live
per-turn log capture. The rolling handoff, deterministic brief and digest remain.

**Recorded:** 2026-10-26
**Code anchors:** `crates/office-tools/src/tools/sessions/`; [specs/sessions.md](./specs/sessions.md)

### Context

The orchestrator is the user's agent CLI, and each session must pick up
where the last one stopped. The alternatives considered:

- **An Orbit task per session**, as constellation's own orchestrators use.
  In the office workspace, tasks are the user's work: a session task would
  appear on the To-dos screen and inflate the automation ratio, and its
  statuses mean nothing for a session.
- **A session-summary artifact.** Artifacts are approved outputs; asking for
  an approval every session is friction for nothing.
- **An end-of-session summary written by the agent.** Sessions end abruptly
  (the user closes the terminal), so it is often never written. STD-02@3
  §R25 asks that an obligation the system relies on be produced by
  deterministic code, not left to prompt text. The agent's own words are
  still worth keeping, so they survive as a rolling handoff (below) that
  nothing depends on.
- **A daily auto-task on a cheap crew.** It runs at the wrong cadence for a
  handoff, cannot see the CLI's transcript, mints a task a day into the
  To-dos and the metrics, and pays for an agent loop to write one summary.

### Decision

- The narrative is an **append-only decision log**: one `conversation`
  source per session, appended to as decisions, directions and actions
  happen.
- The agent keeps a **rolling handoff**: a `handoff` entry in the log,
  appended anew whenever the plan changes, so the latest is always current.
- **`office.session-brief`**, computed at session start from Orbit and the
  Library with no model call, leads with that handoff and does not depend
  on it.
- The day's summary is the **`office-daily-digest` routine**: one
  deterministic step running `office.digest`, which makes one Muse Spark
  call through the privacy routing and writes a `report` artifact in the
  series `digest-daily`.
- That digest is promoted **without review** (`approved.by: routine`), the
  one exception to "artifacts are approved outputs". It is internal and
  never leaves the machine. `by: routine` keeps it out of first-pass
  acceptance and the automation ratio.

### Consequences

- A session that ends abruptly loses nothing, and the brief cannot invent
  state. The digest is the plugin's own Muse call, so it is new Meta-tooling
  work rather than a pre-existing Orbit crew.
- Cost: conversation sources are append-only rather than write-once, so
  capture gains an `append_to` path. The artifact approval rule has an
  exception that every metric must respect. A day's private items are only
  listed by reference when no open-weights endpoint is configured.

### Daily digest definitions ownership

The daily digest adds only its job and routine entries beside the existing
`spec.definitions` registrations, in addition to enabling its reserved tool
block. This is the operator-authorized narrow exception to the
ARCHITECTURE.md integrator ownership rule for this feature. The one-step job
inlines the deterministic `plugin.tool_call` activity, avoiding another shared
activity registration. The routine uses the accepted routine schema without
`required_tools`; `config.tool: office.digest` is its explicit tool reference.

## About pages start from templates and an onboarding interview, never from stubs

**Recorded:** 2026-10-26
**Code anchors:** `crates/office-core/templates/`; [specs/knowledge-templates.md](./specs/knowledge-templates.md)

### Context

A new Library is empty, so the first agent tasks know nothing about the
user, their people or their projects. The user proposed default About pages
(contacts, user, `project_<name>`, `artifact_locations`, `domain_<name>`,
glossary). The alternatives considered:

- **Empty pages seeded by `office.init`.** A stub would be attached to tasks
  while it says nothing. It would count toward coverage and break the rule
  that every claim cites a source.
- **One contacts page.** Every meeting capture would edit it, so proposals
  would collide on one page. One page also gives no back-links per person.
- **An `artifact_locations` page.** Paths filed apart from their subject
  are not attached when the subject is.
- **`domain_<name>`.** "Domain" is already the page type (`dom-` ids), so
  the ids would read `dom-domain-…`.

### Decision

- `office.init` installs **templates** in `.office/templates/`, compiled into
  the binary. It writes no pages. A page names its template in frontmatter,
  and `office.knowledge-write` checks it against that template.
- The bundled set: `user`, `standing-directions`, `systems`, `person` (one
  page each), `client`, `project`, `topic` (renamed from `domain_<name>`),
  `glossary`, `goals`, `rhythm` and `runbook`. Locations go in the subject's
  `refs`. The `systems` page holds what no single subject owns.
- Each template carries an **attach policy** (`always`, `participants`,
  `tagged`, `matched`, `never`). `office.attach` selects pages by template,
  never by a hard-coded name.
- The first pages come from an **onboarding interview** in the first
  session: answers become `fact` entries in the decision log, then one
  `knowledge_add` proposal per page, accepted together in the app.
- **Contact details stay on the machine.** Person pages stay `normal`, but
  email addresses and phone numbers live only in `contact` refs. Every
  model input and attachment carries the page's model view, which drops
  those values. Making person pages `private` by default was rejected:
  every meeting that names a person would route to the open-weights model,
  which is slower and costs more per call, to protect a few fields the model
  never needs.

### Consequences

- Every About page is cited from its first write, and an agent's first task
  already has the user, their directions and its participants attached.
- Templates are data, so a pack or the user can add one without code.
- Cost: a seventh decision-log entry kind (`fact`), a template check on every
  knowledge write, three more signals, and a model-view rule that every
  model call site must use.

## Agents get a pinned Python, installed with uv

**Recorded:** 2026-10-26
**Code anchors:** `install.sh`; [specs/install.md](./specs/install.md)

### Context

Implementing agents will often write and run Python: cleaning a CSV,
reshaping data, running a `script` artifact. macOS has only the old Python
that comes with the Xcode command line tools, and Windows has none (WSL2
Ubuntu has its own, at whatever version that release ships). The
alternatives considered:

- **Use the system Python.** Its version differs from machine to machine, or
  it is missing. A script that works in the demo could fail on a judge's Mac.
- **Homebrew or the system package manager.** Not every Mac has Homebrew,
  and both install a Python the user's other tools share and upgrade.
- **pyenv or a Python installer.** These need a compiler toolchain or an
  admin prompt, and they change the user's shell profile.

### Decision

The install script installs `uv` at a pinned version, checking the
release archive's SHA-256 (STD-05@1 §R23). It
then uses uv to install a pinned Python minor version, both in the app's
own `tools` folder. Agents reach both through the `PATH` the app service and
the MCP entry set. Nothing touches the system Python or the shell profile.

### Consequences

- Every install runs the same Python. `uv run` gives a script its declared
  dependencies without a shared environment that could drift.
- Cost: about 60 MB more on disk, and network access at install time. Bumping
  `UV_VERSION` or `PYTHON_VERSION` is a deliberate change to the script,
  made together with the checksum.

## Routines run approved scripts through `office.script-run`, and their outputs wait for review

**Recorded:** 2026-10-26
**Code anchors:** `crates/office-tools/src/tools/scripts/run.rs`; [specs/scripts.md](./specs/scripts.md)

### Context

A `script` artifact may run only as a step of a routine whose accepted
proposal named it. Orbit routines run deterministic `plugin.tool_call`
steps and have no generic command step, so something in the plugin must
run the script. Agents also need scratch space that never pollutes the
Library or the workspace repository. The alternatives considered:

- **An agent loop that runs the script.** That pays for a model call to run
  fixed code, and the agent could change what runs.
- **Running from the workspace or the Library.** Scratch files would show up
  as Library changes, or as untracked dirt in the primary checkout.
- **Fetching dependencies on every run.** What runs would change between
  runs without a new approval, and every run would need the network.
- **Promoting outputs without review**, as the digest is. That would be a
  second exception to "artifacts are approved outputs", and script output is
  user data such as a catalog or a cleaned spreadsheet, not an internal
  diary. The user chose review.

### Decision

- **Scratch:** `$ORBIT_SCRATCH_DIR` for agent runs,
  `.orbit/tmp/session-<date>/` for the orchestrator, and run directories
  under `plugin_state/runs/` for plugin tools. All are pruned after 7 days or
  die with their worktree. None is ever in the Library.
- **`office.script-run`** (mutating, `mcp_scope: none`) runs the pinned,
  approved version. It uses the install's pinned Python through `uv run
  --offline`, in its own run directory, with an allowlisted environment, a
  timeout and output caps.
- Dependencies are fetched **once, when the proposal is accepted**. Every
  run after that is offline.
- Outputs are filed as a **`script-output` task** and promoted only when the
  user approves.

### Consequences

- What runs is exactly what was approved, and a run cannot fetch new code.
  Every output still passes the user's approval.
- Cost: the script inherits the plugin's grants, including network `any`,
  until Orbit offers a sandbox for plugin children. The spec says so rather
  than claiming isolation. Accepting a scripted routine needs the network
  once.

## API and cloud CLI calls go through gateway plugins

**Recorded:** 2026-10-26
**Code anchors:** `gateway/`; [specs/gateway.md](./specs/gateway.md)

### Context

Office workers keep API requests, with their keys, in Postman. An agent
with a shell would make the same calls with `curl`, the key in its command
line, and no record of what it sent or got back. Cloud CLIs are worse: the
agent would run with the user's own logins and could change things. The
alternatives considered:

- **A standalone Python CLI** that the agent shells out to. It would need
  its own secret store, grant flow and audit, and the agent would have to
  learn its flags. An Orbit plugin gets `orbit plugin enable` consent,
  `orbit plugin secret set` storage under `~/.orbit`, host-side masking of
  secret values, Orbit's tool-call audit and typed MCP tools.
- **Tools in the `office` plugin.** These calls are not office-specific.
  The Rust backend would gain an HTTP client outside `office-models`, which
  the crate policy bans. And the CLI proxy's programs would be bound to
  every agent that calls any office tool.
- **One gateway plugin with both.** Orbit lets an agent call a plugin only
  when every program the plugin declares is on its activity's allowlist.
  Agents would therefore need `aws` and `gcloud` allowlisted, which would let
  them run those CLIs directly.
- **Proving a CLI command read-only by parsing its arguments.** The verbs
  are not a reliable signal, and several "get" commands read secrets.
- **Naming the tools `orbit.request.*`.** The `orbit.` namespace is
  reserved for Orbit's own tools.

### Decision

- Two Python plugins, standard library only, sharing one package:
  - **`gateway`** for HTTP through user-granted connections. Its secrets live
    in sixteen declared slots, because Orbit plugin secrets are names fixed in
    the manifest. GET and HEAD run directly. Writes are drafts that the user
    approves, sent once and bound to the digest the user saw.
  - **`cloudcli`** for read-only `aws` and `gcloud`, orchestrator only. Its
    own read-only identities are the guarantee; a command allowlist and a
    denylist for secret-reading commands are a second layer.
- **One value-based redactor** at the durability boundary, plus high-confidence
  credential shapes, and an append-only audit with no bodies.
- **Deny rules** for direct `curl`, `wget`, `http`, `aws` and `gcloud` in the
  agent CLI, documented as an accident guard.
- GET and drafts are must-have for the event; `cloudcli` is should-have.

### Consequences

- A secret given to a connection never reaches an agent or a log. Every API
  read is on record, and no API write leaves without the user's approval.
- Cost: a second language in the repo, with its own test job. A limit of
  sixteen connections per install until Orbit supports dynamic secret
  names. The deny rules do not stop an agent that opens sockets itself, and
  the spec says so.

## Gateway tool verbs are hyphenated

**Recorded:** 2026-10-26 · [ORB-18796]
**Code anchors:** `gateway/plugins/gateway/.orbit-plugin/plugin.yaml`; [specs/gateway.md](./specs/gateway.md)

### Context

The gateway spec named its tools `gateway.http.get`, `gateway.connection.list`
and so on. Orbit 0.34 accepts a plugin tool's name only when it is lowercase
letters, digits and `-`, so `orbit plugin validate` refuses a dotted verb and
the plugin could not be installed.

### Decision

The verbs are `http-get`, `http-draft`, `http-send`, `connection-list`,
`connection-put` and `audit-list`; the canonical tool names are
`gateway.http-get` and so on.

### Consequences

- The spec, the skill and the app use the hyphenated names.
- The same rule applies to the `office` plugin, whose built `signals.scan` made
  `orbit plugin validate` fail. Every dotted `office` verb was renamed the same
  way (`office.signals-scan`, `office.knowledge-show`, …) before the rest were
  built, with schema and golden files following the verb.
- Cost: the tool names differ from the dotted `office.*` style until Orbit
  accepts dotted verbs.

## The hero pack is KT videos, not meeting recordings

**Recorded:** 2026-10-26
**Code anchors:** `crates/office-tools/src/tools/sources/`; [specs/kt-videos.md](./specs/kt-videos.md)

### Context

The demo's hero loop was a meeting recording becoming to-dos. Most meetings
that lead to work are short and never recorded, and their actions are
cheaper to tell the agent than to transcribe. Capturing unrecorded calls
locally needs system-audio capture and the consent of everyone in the call,
which Daniel judged not worth building now. Hour-long knowledge-transfer
videos are the opposite case. They are recorded, they are painful to watch
(half tangents), and what they teach lasts. So the cost of processing them
is worth paying.

### Decision

- The hero pack is **KT videos**:
  - a `video` source kind;
  - a **cost preview the user confirms** before anything is spent, bound to
    the plan's digest;
  - a fixed routine that transcribes the audio and reads deduplicated
    keyframes, since a KT is mostly a screen share;
  - one cited extraction into chapters (tangents folded, gotchas lifted
    out), knowledge proposals from the About templates, runbooks and
    questions for the presenter.
- Every claim cites a timestamp. Uncited claims are dropped. Tangent content
  becomes knowledge only as a gotcha. No step makes more calls than the
  confirmed plan counted.
- **Meetings** stay a smaller pack: `.vtt` transcripts and pasted notes,
  through `office.extract`'s meeting profile, with no routine of their own.
  **Capturing unrecorded calls** is out of scope.

### Consequences

- The demo shows the cost that's avoided (an hour not watched) and the gain
  that lasts (pages and runbooks agents use). That connects the hero loop to
  the Library and the proposals, rather than to a list of to-dos.
- Cost: frame extraction and reading (`office.frames`, keyframe
  deduplication without an image library), a planning tool, and a synthetic
  KT video to produce for the demo. Image calls per video are capped and
  shown in advance. Day-1 checks on Muse audio limits, context window, image
  input and the private route gate the plan.

## A new role's material is bootstrapped in phases: digest per source, consolidate per topic

**Recorded:** 2026-10-27
**Code anchors:** `crates/office-tools/src/tools/bootstrap/`; [specs/library-bootstrap.md](./specs/library-bootstrap.md)

### Context

Daniel's own routine when starting a role: gather the onboarding pages,
design docs and projects, then the KT videos, then the team's artifacts,
then the company's processes and policies, and turn them into knowledge an
agent can answer from and act on. The risk is a Library cluttered with a
page per document. The alternatives considered:

- **A proposal per source**, as the KT pack first did. Twenty documents
  about one system would make twenty overlapping pages, and the approval
  queue would flood.
- **An agent loop per document.** Its cost is unbounded, and nothing stops
  it writing page after page.
- **The careful model on every document.** The cost of the careful model is
  only worth paying where sources meet, at the subject.
- **Rules only in prompt text.** They drift, and nothing enforces them
  (STD-02@3 §R25).

### Decision

- **Sources keep everything; pages keep only what is worth maintaining.**
- **Four phases, in order:** docs, KT, team, process.
- **Two stages:** one cheap fixed digest call per source, then one careful
  call per subject group across the batch, producing at most one proposal
  per page.
- **KT videos go through the same consolidation.**
- **Page rules** are written in the skill, and checked mechanically where
  possible:
  - the source-count bar;
  - the runbook shape;
  - alias and token-overlap merging;
  - a word cap;
  - policies need an authoritative ref and an `as_of`;
  - volatile facts are excluded;
  - a new-page budget per batch.
- **Batch preview** bound to a digest, and **setup progress** with computed
  gaps.
- **Runbooks may be `performable`.** The agent does everything up to
  submission, and the user submits a draft or approves a gateway draft.
  Browser automation is out of scope for the event.

### Consequences

- The Library grows by subject and stays reviewable, the cost of a phase is
  known before it runs, and gaps are visible.
- Cost: document conversion (HTML, docx, pptx, and PDF through an optional
  `pdftotext`), a `careful` model tier, three tools and a routine. Before
  this, the KT pack proposed pages directly; now it hands its candidates to
  consolidation.

## The private route is Muse Spark Open on an endpoint the user chooses

**Recorded:** 2026-10-27
**Code anchors:** `crates/office-models/src/routing.rs`, `crates/office-models/src/http.rs`; [specs/models-and-privacy.md](./specs/models-and-privacy.md#the-private-route)

### Context

The private route sent `private` material to Muse Spark Open on Baseten or
on loopback. Checked on 2026-10-27 against Meta's release and the model
card, the model has 816B parameters (42B active) and needs about 700 GB of
GPU memory even in NVFP4, so "local" is not an option on office hardware. A
company's own deployment, the most useful choice for work material, was
refused by the host allowlist. And "private" read as "stays on your
machine", which it does not.

### Decision

- **Three endpoint kinds:** company deployment, hosted provider, and own
  hardware. Each is labelled with where data goes. The allowlist is
  `api.meta.ai` plus the one configured endpoint's exact host, over HTTPS
  unless it is loopback, which replaces the Baseten-or-loopback rule.
- "Private" is worded as **"not sent to Meta's hosted API; sent only to the
  endpoint you chose"**, everywhere it appears.
- **`office.private-route-check` records what the endpoint can take**
  (text, image, audio). Private KT videos and bootstrap batches route
  every tier and modality to it, and are refused only for a missing
  capability.
- Preview shows the route per call group. Private material is never sent to
  the hosted API.
- `open_weights_token` replaces `baseten_token`.
- **Deviation (STD-01@2 §R10, §R35):** renaming the status envelope field
  `secrets.baseten_token` and the installer inputs
  `ORBIT_OFFICE_BASETEN_TOKEN` / `BASETEN_TOKEN` is a breaking change
  under STD-01@2 §R10 (stable machine field names) and §R35 (a renamed
  config key stays accepted for at least one release with a warning).
  The original rename recorded no citation and shipped no alias or
  warning. For one release from this record, the old installer inputs
  are accepted as aliases of `ORBIT_OFFICE_OPEN_WEIGHTS_TOKEN` /
  `OPEN_WEIGHTS_TOKEN` with a stderr warning naming the old variable
  and its replacement (never its value), and `office.status` emits
  `secrets.baseten_token` as a deprecated alias mirroring
  `secrets.open_weights_token`. After that release the aliases are
  removed and the old names are refused rather than silently ignored.
- **Attribution:** "Built with Muse Spark Open", shown as the license asks.
- The audience is **every office worker, software engineers included**. The
  pitch names engineering work alongside the rest.

### Consequences

- A company that runs its own Muse Spark Open gets a clean story: work
  material never reaches a third party.
- Cost: the endpoint allowlist changes from fixed provider hosts to one
  configured host, which is less of a code rule and more of an operator
  choice, so the Settings screen states the destination plainly. The check
  costs three tiny calls. The demo uses a provider, labelled as a third
  party.

## The old unset endpoint remains readable

**Recorded:** 2026-10-27

**Code anchors:** `crates/office-core/src/domain/settings.rs`

### Context and decision

The earlier initializer wrote `open_weights_endpoint: null` in every Library.
The new private route needs a full `open_weights` block, so an old configured
endpoint cannot safely supply the model, destination kind and label. That old
key is removed for non-null values; users must configure the new block.
The old null value is accepted as an unset route so existing default Libraries
remain readable and a subsequent config update removes the obsolete key.

**Deviation from STD-01@2 §R35:** the removed key is refused when non-null,
and the accepted null value is ignored without a stderr warning. The pure
settings parser has no warning channel, and propagating one through every
Library read and init path would change the Library interface. This bounded
exception preserves the common default without guessing a route or silently
routing private material to an endpoint. It does not provide the standard's
one-release warning or compatibility for configured old endpoints.

### Consequences

Libraries with the old default can load and update settings. Libraries with an
old configured endpoint receive an error naming the key and need manual
migration to the new `open_weights` block.

## office.attach stages its copies inside the Library

**Recorded:** 2026-10-27
**Code anchors:** `crates/office-tools/src/tools/library/attach.rs`, `crates/office-store/src/repository/library.rs`; [specs/library.md](./specs/library.md#search-and-attaching-to-a-task)

### Context

`office.attach` copies Library files into a task with
`orbit.task.artifact.put`, which takes a `source_path` and reads only files
inside the workspace. A knowledge page must go out as its model view, not
its file, so each copy is a rendered file that has to exist somewhere first.
The plugin's `plugin_state` directory is under the global `~/.orbit`, outside
the workspace, so the put refuses it. The plugin's only workspace write
grant is `{{workspace}}/library`.

### Decision

- Each copy is written to `library/.office/attach/<call>/<library path>`,
  put, and its staging file removed at once. Emptied staging directories
  are pruned.
- The store refuses a staging root that is a symlink or not a directory,
  and removes only regular files under that root.
- `.office/` is outside the layers `reindex` scans, and each Library commit
  stages only the paths it wrote, so staging never shows in search or in the
  Library history.
- The selection is read at one commit: attach reads `HEAD` before and after
  and refuses with `library_changed` if it moved. The manifest records that
  commit.

### Consequences

- Attach needs no new grant, and the model view is the only form of a page
  that leaves the Library.
- Cost: a crash between a put and its removal leaves a stray file under
  `.office/attach/` that `git status` lists as untracked. It is never indexed
  or committed, and deleting the directory is always safe.

## A leader exit with open pipes settles as a timeout, never by signalling after reap

**Recorded:** 2026-10-27
**Code anchors:** `app/packages/server/src/orbit/client.ts` (`createOrbitClient`), `app/packages/server/src/dispatch/pass.ts` (`shipTasks`)

### Context

Node emits `exit` when the child leader is reaped and `close` only after
its stdio streams close. A descendant that inherits stdout or stderr can
hold a pipe open after the leader exits, so a supervisor that clears its
deadline on `exit` and settles only on `close` hangs indefinitely.
STD-03@2 §R13 requires the child's group to be swept (SIGKILL to surviving
members) before the supervisor returns, while §R14 forbids signalling a
PID or process group after the owning child is reaped. Since `exit` fires
only after reaping, no compliant post-exit sweep exists.

### Decision

Deviation from STD-03@2 §R13 for the post-exit interval only: neither
wrapper signals after `exit`. The original deadline stays armed across
`exit`; when it fires (or, if it already fired, after the existing 250 ms
escalation grace) the call settles without signalling. A leader that
exited 0 with a pipe still open therefore rejects as a timeout rather
than resolving with output collected so far, because stdout is not at
EOF and may be truncated. A `close` that arrives before the bound still
settles normally with the real exit code and output.

### Consequences

- Both `callTool` and `shipTasks` settle within a bounded time when a
  descendant holds a pipe, and no PID or group is signalled after reap.
- Cost: surviving descendants are not swept after the leader is reaped
  (the §R13/§R14 tension above); they are left to their own lifetime and
  the host runtime's containment (STD-03@2 §R21). A leader that exited 0
  just before its deadline can be reported as a timeout if a descendant
  holds its pipes past the bound.

## The calendar is a read-only snapshot with prep before and capture after

**Recorded:** 2026-10-27
**Code anchors:** [specs/calendar.md](./specs/calendar.md)

### Context

Daniel asked for a calendar in the app, kept by the agents: routines and
meetings on one view, prep before a meeting, and a saved transcript and
summary after it. Meetings already had a pack (`.vtt` transcripts and
pasted notes through `office.extract`), but no routine and no way to know
a meeting was coming. Outlook is the calendar most users have. There is no
email sync to ride on, and Orbit has no trigger that fires before an
external event.

### Decision

- **Read, never write.** The calendar is read through `gateway` (an ICS
  feed with a `secret_path` connection, or Microsoft Graph with an
  `oauth2_device` connection). RSVPs, new events and moved events are
  outward-facing actions and are not built.
- **A snapshot in plugin state, not the Library.** The events are a
  schedule, not something that happened. Each sync replaces them
  atomically in `{{plugin_state}}/calendar/`. A meeting enters the Library
  only when its notes or transcript are captured, as a `meeting` source
  linked by the event key.
- **Prep and capture are routines that poll the snapshot.** They do not
  use a new Orbit trigger: `office-meeting-prep` and `office-meeting-inbox`
  run every 15 minutes, de-duplicated by event key and source. This
  revises [the hero pack decision](#the-hero-pack-is-kt-videos-not-meeting-recordings):
  meetings now have routines of their own. Calls that aren't recorded are
  still not captured.
- **Rejected:**
  - committing events to the Library's git (a commit every 15 minutes of
    churn and personal detail, with no lasting value);
  - waiting for email sync (it doesn't exist);
  - an Orbit "before event" trigger (a core change, which is out of scope
    for the event);
  - office fetching the feed itself (only model inference may leave the
    machine outside the gateway's audit).

### Consequences

- The day's meetings, their prep and their outcome share one screen, and
  each captured meeting feeds the person and client pages that the next
  meeting's prep reads.
- Invite bodies are untrusted text from outside. Every model input quotes
  them as data.
- Cost:
  - two new gateway auth schemes, one of them an OAuth refresh-token
    rotation;
  - a bounded ICS recurrence and Windows time-zone reader in office;
  - a new owner-only state file;
  - three routines;
  - a prep model call per meeting, capped per run.
  Recurrence rules outside the subset show their first instance only.

## Teams share through one shared library, synced from a shareable field

**Recorded:** 2026-10-28
**Code anchors:** [specs/shared-library.md](./specs/shared-library.md)

### Context

A new joiner rebuilds the team's knowledge from the same onboarding pages
and KT videos everyone before them already processed. Daniel already works
around this: he keeps one shared knowledge base beside his own, and a sync
pushes and pulls between them on a schedule. The overview listed
"multi-user or teams" as a non-goal for the event. The alternatives
considered:

- **The shared repository as the Library.** Every write would be shared,
  including private sources, 1:1 notes and receipts, and every edit would
  risk a merge.
- **Peer Libraries**, each member pulling every colleague's export. Every
  id and citation would need a per-member namespace, and joining means
  adding a remote for each person.
- **Export and import as sources.** A colleague's pages would arrive as
  copies, so a subject would fork into one page per member.
- **An agent-run sync skill**, as at work. With the field in place, nothing
  in the sync needs judgment, and an activity's system of record is its
  durable artifact, not an agent's message.
- **Going through `gateway`.** The gateway carries HTTP API calls with a
  secret per connection. Git is a different transport, and the user's own
  git credentials already cover it.

### Decision

- **One shared library per workspace**, a git repository with a remote,
  alongside each member's own Library. Pages are shared by subject;
  sources and artifacts stay under the member who shared them and are
  cited as `<member>/<id>`.
- **`shareable` is the approval.** Only `office.share-set` (MCP-hidden) and
  accepted proposals set it. `private` documents and documents citing
  unshared ones are refused. Every byte the sync pushes was already
  approved: by capture, by an accepted proposal or by promotion.
- **The sync is a deterministic tool**, `office.library-sync`. It pushes a
  shared view (no contact or path refs) and pulls into a read-only layer.
  Changes to the user's own copy of a page arrive as proposals, never as
  writes.
- **Git is the third outward path**, after model inference and the gateway.
  It reaches only the configured remote, with an allowlisted environment,
  restricted protocols and no hooks.
- **The teams non-goal is narrowed** to accounts, permissions and shared
  writes outside this sync. This revises the overview's non-goal list and
  the calendar decision's rule that only model inference leaves the
  machine outside the gateway.

### Consequences

- A new joiner connects, syncs and inherits the team's pages, runbooks and
  KT readings, and bootstrap only fills the gaps.
- Colleagues' text is untrusted input, like any source: it is validated on
  pull, quoted as data in model inputs, and a shared script never runs.
- Cost:
  - a second index layer that every read tool and the citation check must
    resolve;
  - a member-qualified citation form;
  - a git transport outside the gateway's audit, recorded only in the
    shared repository's history and the sync result;
  - a conflict proposal kind;
  - one routine.
  Unsharing can't recall copies colleagues already synced, or the history.

## Template decisions not carried over

The template's note-store decisions (persistence module, one module per noun, audit guard, `--json` alias, list envelope, store lock, store format registry, owner-only store state, malformed-note handling, environment fallbacks) were removed with the note domain. They return, rewritten for the office domain, only when the feature they govern lands.

## Proposal acceptance and Orbit auto-tasks

On 2026-10-27 Daniel selected auto-tasks for recurring proposals: Orbit has
no routine-create tool. Shipped daily signals use a separately registered
one-step job and disabled routine; weekly knowledge maintenance is a disabled
auto-task. These three registrations and the two tool blocks are the narrow
manifest ownership exception for this feature.

Office reserves acceptance with Orbit's atomic start transition after
validating the pinned payload and preparing script dependencies. It uses the
existing task, artifact and auto-task callbacks; it never grants authority
through payload fields or exposes acceptance over agent MCP. Closed or
already-started proposals refuse all further mutations. Auto-tasks mint tasks,
so pinned tool steps become task instructions and required tools; they are
not a new scheduler `steps` field. Host-hidden script execution still needs
an authorized deterministic caller.

**Deviation: STD-03@2 §R8 and §R9.** The existing Orbit callbacks and Library
port cannot commit across the two independent stores as one transaction.
Proposal creation and acceptance therefore use explicit sequential writes.
Acceptance's task start and pinned artifact form the durable decision, but
there is no automatic replay of partially applied Orbit effects. An uncertain
callback is never retried; the task remains in progress or review and further
acceptance refuses pending operator inspection. A successfully created
auto-task can remain enabled if recording or closing subsequently fails.
Creation likewise can leave a proposed task with only some attachments;
missing payloads cannot be accepted. Stage contents survive a failed copy.
This bounded limitation avoids duplicate enable/create callbacks but is not
an all-or-nothing publication guarantee. Atomic cross-store recovery needs an
Orbit transaction/idempotency API beyond the current callback contract.
Acceptance requires the exact reviewed artifact digest (STD-05@1 §R21),
returns it on creation, and records it in completion evidence. Script execution
checks that binding for new approvals; legacy undigested approvals stay readable.
Knowledge revisions are separately protected by compare-and-write under the
Library writer lock, and proposal records have a version-8 Library journal.

## Machine installation and workspace initialization are separate

**Recorded:** 2026-10-27
**Code anchors:** `install.sh`; [specs/install.md](./specs/install.md#orbit-office-init-path)

The installer owns machine provisioning: toolchains, plugin installation,
secrets, agent integration and the app service. A small installed `orbit-office`
launcher owns `init [path]`: git, workspace registration, plugin enablement and
the Library initializer. The installer calls that launcher so repairs and
second workspaces follow the same implementation. Embed its source once in the
installer so a curl-to-sh installation remains self-contained. Creation receipts
govern launcher uninstall; global enable/scaffold markers cannot decide the
state of another workspace. Live Orbit state governs those checks.

This command is outside the Rust exec backend (`office-bin`). That backend
serves Orbit's one-call JSON envelope and runs only after a plugin is enabled;
a human command must be able to create the workspace before that point.
`office.init` remains the Library-only tool. No `open` or `status` command is
introduced.

**Renamed 2026-10-28:** the launcher is `orbit-office`. The exec backend binary
kept the name `office`, and a launcher of the same name on PATH showed the
backend's help instead of `init` (Daniel's call: rename the launcher, keep the
backend).

**Deviation: STD-03@2 §R6, §R8 and §R9.** Like the existing machine installer,
setup is a sequential repair procedure across git, Orbit and local receipts,
not a transaction across those stores. Successful steps remain after a later
failure and the next invocation checks and repairs them. Run one installer or
initializer per user at a time; the receipt file has atomic replacement but
no cross-process writer lock. Concurrent setup is not supported.

## An oauth2_device refresh token reaches its slot through Orbit's secret_updates

**Recorded:** 2026-10-27
**Code anchors:** `gateway/src/orbit_gateway/oauth.py` (`_poll`, `access_token`), `envelope.emit`; [specs/gateway.md](./specs/gateway.md#oauth2_device)

The calendar spec first had the device-code sign-in's refresh token reach its
slot through `orbit plugin secret set gateway <slot>` on stdin, like every
other secret. Nobody holding the token can run that command. Orbit refuses a
plugin backend every `secret` verb (`policy_denied`). The app could run it
only if the poll returned the token, which would put it in a tool result.
Orbit's answer to this is the `secret_updates` field of the `exec` answer. It
writes a declared `rotatable` secret with compare-and-swap before the call
returns, and masks the value in the result.

So the poll answers `connected` with a `secret_updates` entry for the
connection's slot, and each call writes back a rotated refresh token the same
way. All sixteen slots are declared `rotatable`, because any of them may
belong to an `oauth2_device` connection. The backend writes updates only for
that kind of connection's own slot. Rejected: a dedicated `oauth_NN` slot
family (sixteen more names, and free-slot logic over two pools), and keeping
the token in plugin state (a secret outside the slots, which Orbit's host
would not mask).

Cost: the other schemes' slots are rotatable too, so a backend bug could
overwrite a bearer token. Compare-and-swap means two overlapping calls
rotating from one version store only one token; the other call's refusal is
`auth_stale`, which is retryable, not `auth_expired`.


## Calendar snapshots use gateway callbacks and a separate private port

**Recorded:** 2026-10-27

The calendar feature owns its tool registry rows, family module, manifest tool
blocks, callback grants, job/routine registrations and schemas. This is the
explicit narrow integrator-ownership exception authorized by the calendar task,
as for the digest. The external parser dependencies are recorded in the root
manifest, architecture and dependency-direction guard; no crate or internal
crate edge is added. Existing complete Ports literals receive a refusing
calendar adapter; testkit's builder receives the working in-memory adapter.

Orbit's `enforce_plugin_callback_allowlist`/`refuse_unless_recorded` compares
exact granted names and does not restrict namespaces. Choose its existing
host-issued descriptor channel with `gateway.connection-list` and
`gateway.http-get`, not a two-step scratch-file bridge. The metadata read
selects Graph versus ICS and provides the base_url for next-link confinement.
Only the gateway makes calendar network requests. Local exact-name allowlists
are accident guards; live host grants and descriptor credentials decide authority
(STD-05@1 §R1, §R5). A real connection requires both Office's new callback grants
and the gateway connection to be enabled; offline fixtures do not establish a
live account connection.

`chrono` disables its clock/default features. `chrono-tz` supplies IANA
transitions and a vendored CLDR release-46 territory-001 table supplies Windows
names. Floating/unresolved times are skipped, never interpreted as UTC. RRULE
work is bounded and unknown syntax retains only the first instance.

The calendar store uses the existing bounded writer-lock mechanism in its own
private directory (no Library-lock nesting). Each success atomically replaces
owner-only events.json with a new generation. Failures atomically write a safe
code plus the exact prior snapshot hash in last_error.json; list ignores error
metadata for a different generation. This leaves snapshot bytes untouched on
failure and avoids a two-file clear/publication transaction. Readers see an
atomic event generation; concurrent failed attempts may update only its banner.
The whole backend retains its 60-second deadline; an interrupted process keeps
the old snapshot but cannot guarantee a last_error receipt. Each gateway child
is separately bounded to 30 seconds and is never retried.

Calendar settings are excluded from the existing bootstrap plan fingerprint:
they do not affect its sources, inference or approved budget (STD-03@2 §R28).
This preserves existing preview digests instead of regenerating unrelated
conformance goldens when the calendar block is introduced. A public dispatch
regression checks confirmation after changing calendar.connection.

## Meeting prep reuses immutable routine report publication

**Recorded:** 2026-10-27

The meeting-prep task authorizes the narrow integrator-owned registry, calendar
module, manifest and job/routine registration edits, as for the daily digest.
No crate or internal dependency edge is added. The artifact model gains an
optional `event_key`, omitted when absent so existing reports retain their wire
shape. Existing manifest constructors explicitly leave it absent. Routine
approval remains limited to reports in `digest-daily` and `meeting-prep`; prep
requires an event key. The Library's existing writer-lock version allocation
also rejects duplicate prep keys before publication. Other artifact series
retain their existing version behavior.

Meeting prep crosses only core ports. It reloads page model views per selected
event and checks the Library generation before inference and publication.
The shared pure contact-text redactor is public for this model-view boundary;
known contact-ref values and attendee addresses are also removed throughout the
serialized views. The agenda is blockquoted untrusted JSON data, separate from
the fixed instruction. Output items survive only with supplied, resolving page
or source-anchor citations; the renderer owns citation markup. No evidence
means a deterministic brief, with no inference. Privacy conservatively covers
all gathered input, including evidence the model does not cite.

There is no calendar-account identity field in the snapshot. A user-page
contact/title/alias can identify self-only invitations; without it, the existing
invitee list is the available signal. This limitation is documented explicitly,
with no address inferred from the host environment. Model calls are not
reserved before inference: simultaneous callers may both spend, while locked
publication guarantees one durable report per key, as with daily digests.



## Calendar meeting capture owns its routine registration and summary sidecar

This feature's task grants the narrow exception for the plugin manifest's
integrator ownership: register the meeting inbox job/routine in the same change.
The same manifest change declares ffprobe, required for local meeting audio
duration inspection before transcription. It adds no crate or dependency edge. The inbox uses the existing `office.extract`
meeting profile with no id, keeping deterministic orchestration in application
code behind the same Library, Calendar, Models and Orbit ports. Each new source
copies only event identity, public attendee names and the event's sensitivity;
event captures deliberately bypass content deduplication.

`summary.md` is a completion marker published after to-do filing, and is folded
into the source index record by the same companion-reading path as chapters.
The source body remains the original evidence. `document: summary` gives
source-show a separate line-anchor space without changing transcript anchors.
An extraction result without a summary is an uncertain to-do handoff and refuses
automatic repetition; partial transcription has its own completion receipt.
Neither condition permits silently accepting partial evidence or duplicating
to-dos. The disabled 15-minute routine is included with the meetings pack, like
the KT routine; enabling it remains the onboarding/user choice.

## Sharing approval owns its narrow manifest additions

**Recorded:** 2026-10-28
**Code anchors:** `office-core/src/domain/sharing`, `office-tools/src/tools/sharing`,
`office-store/src/repository/library.rs`; [shared-library.md](./specs/shared-library.md).

This feature owns its new tool family declaration, `tools/sharing/mod.rs`,
registry row, `share-set` manifest block, schemas and conformance fixture,
following the calendar's narrow manifest-ownership precedent. These are
necessary to ship a callable tool with a closed contract, not a general
exception for unrelated registry or manifest changes. No crate or dependency
edge is added.

Approval checks run both at tool validation and inside the store's writer
lock, against current cited Markdown rather than stale indexed flags.
`share-set` rewrites only its scalar; absence already means false, so clearing
an absent field is a no-op. The sensitivity outcome carries whether that
atomic update revoked sharing, rather than guessing from a read before locking.
Migration 3 records a durable rescan obligation for existing rows; a fresh,
empty index incurs no backfill write. Unconfigured shared settings are omitted
from canonical settings serialization, preserving existing budget fingerprints.
Shared view projection is pure; sync, credentials, transport and the read-only
shared layer remain separate work.

## The shared library sync owns its manifest additions and its git port

**Recorded:** 2026-10-28
**Code anchors:** `office-core/src/domain/sharing/sync.rs`,
`office-core/src/ports/shared.rs`, `office-store/src/shared.rs`,
`office-tools/src/tools/sharing/sync.rs`; [shared-library.md](./specs/shared-library.md#officelibrary-sync).

**Manifest ownership.** This feature edits the integrator-owned files it needs
to ship one callable tool with a closed contract, following the sharing
approval and calendar precedents: `tools/sharing/mod.rs`, the registry row,
the `library-sync` block in `plugin.yaml`, the `office-shared-sync` job and
routine entries in `spec.definitions`, the `library-sync` request/response
schemas, the `choice` field and `kept` change in the `proposal-accept`
schemas, and a conformance fixture. It is not a general exception for
unrelated registry or manifest changes. No crate or dependency edge is added;
`office-common` gains one supervisor entry point with a caller-chosen deadline.

**Git sits behind a port.** `SharedRepository` (lock, state, reset, fetch,
read, checkout, push) keeps the tool pure and testable with
`office_testkit::fakes::FakeShared`; `office_store::SharedStore` is the only
code that runs git. Fetching only updates a remote-tracking ref and reads go
through git objects, so `dry_run` never touches the working tree, the index,
the sync state, proposals or the remote. Pushes build one commit with
plumbing (`read-tree`, `hash-object`, `update-index`, `commit-tree`) on the
fetched head and push it by object id, so the working tree never has to be
merged and a rejection is a plain non-fast-forward.

**Per-path state decides push versus proposal.** Each shared path records the
blob last seen, the blob last reconciled (`base`: pushed, matched, or
overridden by **Keep mine**) and the digest of the own document then (`own`).
A page is pushed only while the shared copy is still `base`; a changed shared
copy with an unchanged own copy is an incoming `knowledge_update`, and with a
changed own copy a `shared_conflict`. Matching content (ignoring `shared_by`
and `contributors`) settles both sides without a push. Member namespaces are
always this member's to overwrite. Spec reading: "unchanged since the last
push" is measured against `own`, and a page this member never pushed but holds
an own copy of counts as changed, so it conflicts rather than overwriting.

**Branch changes reset too.** The spec names remote and member; the state is
also bound to the branch, because another branch is another shared history.

**Local remotes run no remote hooks.** Git clears `-c` overrides for local
transports, so a path or `file://` remote's own `receive-pack` hooks would run
as the user. Pushes to such remotes pass `--receive-pack` with
`core.hooksPath=/dev/null`; SSH and HTTPS servers run their hooks on the
server, as intended.

**Deviations from the spec text.** Skipped push documents are reported in the
result's `skipped` list (with `phase: push`) instead of a separate Needs you
task; the app (ORB-19421) builds that card. **Ask an agent to merge** is not
built; a conflict offers **Use theirs** and **Keep mine**. The demo shares a
transcript without frame readings, because those come only from the macOS
video pipeline; `frames.md` round trips in the end-to-end test.

## Meeting extraction files every to-do as owner:you; the orchestrator promotes

**Recorded:** 2026-10-29 · [ORB-19747]
**Code anchors:** `crates/office-tools/src/tools/sources/extract.rs::run`;
`plugin/.orbit-plugin/definitions/activities/office-agent-todo.yaml`

### Context

`office.extract` (`profile: meeting`) filed one task per model to-do with
the model's `owner` copied into the filed tag, and with no acceptance
criteria, no `no-diff-expected` and no attached context. The agent to-do
activity accepts only tasks tagged `owner:agent` **and**
`no-diff-expected`, so a model-marked agent to-do was refused if
dispatched, and nothing gated the model's owner choice against the
"runbook or clear tool path" rule. The alternative — extraction filing
agent to-dos complete, with criteria the model would have to author —
was rejected: no extraction code path sends acceptance criteria to
`orbit.task.add`, and an extraction model reply is unreviewed evidence,
not a reviewed task brief.

### Decision

Extraction files every meeting to-do as `owner:you` (as it already did
for KT questions). The model's `owner` remains in the structured result
(`extract-result.json`) as a hint only; it never becomes the filed tag.
Promotion is the orchestrator's step, after the summary: promote a filed
to-do to `owner:agent` only when an attached runbook or a clear,
permitted tool path can produce it and it is not outward-facing, adding
acceptance criteria and `no-diff-expected` and running `office.attach`
before it becomes ready. The model schema, the owner filter and the
agent activity are unchanged.

### Consequences

- No to-do filed by `office.extract` can carry `owner:agent` without
  also carrying `no-diff-expected` and acceptance criteria: it cannot
  carry `owner:agent` at all. The meetings skill, the orchestrator packs
  reference and the kt-videos/calendar specs state this rule.
- A model-suggested agent to-do waits as the user's to-do until the
  orchestrator (or the user) promotes it; nothing is dispatched straight
  from a meeting.
- Cost: promotion is a manual orchestrator pass over each meeting's
  to-dos, and a user who never runs it keeps agent-suitable work in
  their own list.

## Unused automation stays planned until usage telemetry exists

**Recorded:** 2026-10-29 · [ORB-19748]
**Code anchors:** `crates/office-core/src/domain/signals.rs::unused_automation`, `crates/office-tools/src/tools/proposals/signals_scan.rs::run`; [specs/proposals.md](./specs/proposals.md#signals)

### Context

`unused_automation` reads `facts.automation`, but the scan builds
`SignalFacts::default()` and never fills `automation`, so the signal can
never fire. No port exposes the needed opened/read telemetry: the Orbit
port lists auto-tasks and task history with no opened event, and the
Library port has no read tracking. The alternatives were to load per
auto-task usage (minted tasks plus when outputs were last opened or
approved) or to mark the signal planned.

### Decision

Mark `unused_automation` as planned and non-firing in
[specs/proposals.md](./specs/proposals.md#signals),
[references/glossary.md](./references/glossary.md) and the orchestrator
`proposals-and-insights.md` reference. Keep the `unused_automation` enum
value in `signals-scan.response.json` as the stable contract; the scan
code is unchanged.

### Consequences

- The three artifacts agree the signal does not fire, and the
  orchestrator must not tell the user an automation is unused unless they
  say so.
- Cost: the `retire` proposal kind stays reachable only by conversation,
  and wiring real usage telemetry later must also revisit declared-cadence
  coverage, which currently uses listed task titles.

## Private content is withheld from MCP-visible read tools

**Recorded:** 2026-10-29 · [ORB-19752]
**Code anchors:** `crates/office-tools/src/tools/sources/show.rs`, `crates/office-tools/src/tools/library/search.rs`, `crates/office-tools/src/tools/knowledge/show.rs`, `crates/office-tools/src/tools/artifacts/show.rs`; [specs/models-and-privacy.md](./specs/models-and-privacy.md), [specs/selection-sharing.md](./specs/selection-sharing.md)

### Context

`private` means inference only on the user's chosen private endpoint.
The office tools enforced that for their own model calls, but the
MCP-visible read tools returned private bodies and snippets to their
caller unchecked: `office.source-show` returned the whole body or anchor
span, `office.search` returned an FTS snippet built from the body, and
`office.knowledge-show` / `office.artifact-show` returned page and
artifact bodies. The orchestrator is the user's agent CLI, usually on a
hosted model, so every such read sent private text to that hosted model
— exactly what selection sharing already prevents by dropping the quote
from private selections. The two surfaces disagreed.

The alternatives considered:

- **A per-caller rule (MCP versus app).** The call context carries no
  caller-kind key in this checkout, and `McpScope` is a static manifest
  flag, so no in-repo discriminator exists. A caller key supplied in
  tool input would be spoofable by the very caller it must hold back.
- **A summary produced on the private route instead of the body.** No
  such summary exists for most documents, producing one needs a private
  endpoint call on a read path, and a generated summary is still
  derived private content whose wording no one approved for release.
- **Refusing private reads with an error.** The caller then cannot
  learn even that the document exists, which breaks citation,
  re-routing and "listed by reference" flows that the digest already
  uses for private items.

### Decision

Uniform withholding on the MCP-visible read surface. For a `private`
document, `office.source-show`, `office.search`, `office.knowledge-show`,
`office.artifact-show` and `office.scribe-query` return metadata only — id, layer, kind,
title, path, sensitivity, and (for sources) provenance, anchors as
identifiers and citers — and mark the result `content_withheld: true`.
Withheld, per tool: the source body/`text`/span, its `summary.md`
sidecar and chapter/frame/gotcha text; the search snippet (the hit
still lists, with `sensitivity` on every hit); the page body; and the
artifact's body companion. This matches `office.selection-get`'s
stored-record shape for private objects (reference and anchor, never
the quote) and `office.attach`, which already withholds private
documents. Media payloads keep their existing private-route gating.
No private-route summary is produced.

### Consequences

- No caller of these tools — MCP agent or app — receives private body
  text through them. The app, which calls the same tools as the user,
  therefore shows the withheld state for private items instead of the
  body; restoring full private display in the app needs a future
  `mcp_scope: none` app-only read tool, deliberately not built here.
- The agent can still find, cite and route private material: search
  lists it, source-show names it, and model work on it goes through
  the tools that route to the private endpoint.
- Cost: a private source cannot be read in the app's source view until
  that app-only path exists, and callers must handle
  `content_withheld: true` (an empty body/snippet) rather than a
  refusal.

## Scoped agent preparation, separate from application authority

**Recorded:** 2026-10-30
**Code anchors:** `plugin/.orbit-plugin/plugin.yaml`, `scripts/install/lib/steps.sh`

The orchestrator needs to capture sources, file pending proposals, attach task
context and consume app selections ([ORB-20096]). Orbit >=0.39.0 provides the generic
`agent_callable: true` plus digest-bound `agent_tools` consent mechanism; Office
uses it for exactly `capture`, `proposal-create`, `attach`, `selection-get`.
The [full inventory](specs/plugin-surface.md#agent-authority-inventory) records
backend writes, callbacks, cost and the decision for every remaining tool.
No core exception, new callback grant or operator-wide agent session is added.
MCP placement remains unchanged; hidden tools remain governed through CLI too.

Operator acceptance remains the only application path for proposal payloads.
Attachment is limited to task artifacts and selection consumption to the app tray.
Extraction/transcription retain their cost/confirmation route; sharing retains
its external-send boundary. App/operator authority and audit identity are intact.

Installer consent lists the exact verbs and all existing scoped grants. A
non-interactive run requiring consent now refuses unless `--yes` is explicit,
rather than inferring consent from absent input. Repair preserves a recorded fs
scope and refuses an unexpected agent verb set; Orbit owns digest witnesses and
revokes consent on widening. The Office version remains unchanged; only the
minimum host/toolchain pin moves to 0.39.0. This follows STD-05@1 §R3 and §R21.

The real public fixture must prove authorization and persistence; installer stubs
prove only installer behavior. A host callback filesystem denial cannot count as
successful proposal verification or be repaired by broadening authority.

## Local calendar events are workspace writes

**Recorded:** 2026-10-30 · [ORB-20246] · amended 2026-11-01 · [ORB-20825]
**Code anchors:** `office-core/src/domain/calendar/local.rs`; `office-store/src/calendar.rs`;
`office/src/app.rs` (`RealCalendar::store`);
`office-tools/src/tools/calendar/{sync,list}.rs`; `screens/calendar/editor.ts`;
`specs/calendar.md`.

### Context

Requiring an Outlook or ICS connection hid routines and scheduled to-dos behind
setup, and prevented using the meeting workflow without an external calendar.
The existing calendar port already gives list, prep and capture one event model.
The prohibition on outward-facing actions protects external systems, rather than
preventing the user from editing their own workspace data.

### Decision

Keep a versioned local event store and independent source choices at
`<library>/.office/state/local-calendar/local.json` (amended below; it was
`<workspace>/.office/calendar/local.json`). This user-owned data is separate
from the imported plugin-state snapshot: a sync never overwrites authored events.
Local writes are in the same local Library action class as notes and briefs;
they send no invitations, modify no remote calendar and require no gateway.
They are not Markdown Library records and are not automatically Library git commits.
Prep and summaries still use their existing model/privacy routes.

Merge selected sources behind the Calendar port so all existing meeting consumers
use the same event keys and workflow. Local updates retain keys and series; deletion
keeps captured Library notes. List output marks origin in words in the app.
Settings can show local, connected, both, or neither without deleting anything.
Always render the schedule even while the calendar setup card is shown.

Extend the existing mutating calendar-sync surface with a closed `local` operation
instead of changing the shared tool registry or adding an outward action. Its
original empty and `ics_file` inputs remain compatible; local-only empty sync is a
no-op. Use the existing bounded writer lock and atomic file helper, with private
permissions and fail-closed schema-version handling.

### Consequences

- Local creation, editing and deletion work offline and persist across app restarts.
  External snapshots retain the read-only contract.
- Prep, capture and summaries use the existing pipeline for both sources.
- Cost: a separate store must be backed up alongside the Library; local
  events have no recurrence, invitations, or automatic Library git history. The
  calendar-sync tool now carries local mutations as well as imported sync.

### Amendment: the store lives in the Library's ignored state (ORB-20825)

The installed plugin can write only `{{workspace}}/library` and
`{{plugin_state}}`, so under the plugin sandbox every local change to
`<workspace>/.office/calendar/` was refused; the binary tests run Office
unsandboxed and could not see it. The store moves to
`<library>/.office/state/local-calendar/local.json`, resolved from the
configured Library root after the Library grant check, following the scribe
parse files (ORB-20820). The manifest and consent are unchanged, and the
Library's `.gitignore` already ignores `.office/state/`. The directory is
`local-calendar`, not `calendar`, so it never shares a directory or writer lock
with the imported snapshot when a call carries no `plugin_state` and the
snapshot falls back to the same `.office/state/`.

- **Old events are read, not moved.** While the new file is absent, reads fall
  back to `<workspace>/.office/calendar/local.json` with the same symlink and
  schema-version checks; the next local change writes the merged state to the
  new path. The old file is never rewritten or removed, because the sandbox
  cannot write there.
- **No pre-init writes.** Dispatch already refuses calendar tools on an
  uninitialized Library, so a local write never creates Library directories
  ahead of `office.init`.
- **Consequence.** Local events are per-machine writer state: as before, they
  are not in Library git history, and they do not travel with a Library clone.
  Deleting `.office/state/` (for example to rebuild the index) deletes them too,
  so back up the Library directory as a whole, not only its git history.
- **Rejected:** a `{{workspace}}/.office/calendar` write grant (a new grant,
  consent change and integrator-owned manifest edit, and the workspace repo would
  have to ignore it) and `{{plugin_state}}` (a plugin cache reset would delete
  authored events).

## Library sources are partitioned by kind

**Recorded:** 2026-10-30 · [ORB-20243]
**Code anchors:** `crates/office-store/src/repository/library.rs` (`capture_source`, `migrate_source_layout`, `initialize_at`); `crates/office-store/src/driver/file/library.rs::migrate_legacy_sources`; `crates/office-core/src/domain/sharing/sync.rs::classify`; `crates/office-core/src/domain/sharing/mod.rs::shared_document`; [specs/library.md](./specs/library.md#sources), [specs/shared-library.md](./specs/shared-library.md#the-shared-repository)

### Context

Every source sat in one date tree (`library/sources/YYYY/MM/<id>/`), so
conversation sources — soon one per orchestrator session — mixed with
documents and every other kind. A source's `kind` is set at capture and
never changes, so it can partition the tree without paths ever moving
again. Citations already address a source by id (`src-…#Ln`), not by
path.

### Decision

New sources are written under `sources/<kind>/YYYY/MM/<source-id>/`,
and shared member sources under
`members/<member>/sources/<kind>/YYYY/MM/<source-id>/`. Enumeration,
listing, search, citation resolution and shared-layer validation resolve
by id and accept both the new layout and the legacy
`sources/YYYY/MM/<source-id>/` layout (locally and under
`members/<member>/`). `office.init` migrates a Library in place: each
legacy source directory moves whole — `source.md`, originals and
sidecars — into the directory for the kind in its frontmatter, then the
Library is reindexed and the move committed. The migration is
idempotent (a re-run moves nothing and commits nothing), and a source
whose kind directory already holds a different directory for its id is
left in place rather than overwritten. `LibraryStore::migrate_source_layout`
is the same move as a standalone step.

### Consequences

- Kinds are distinguishable in the tree and in the shared library, and
  paths stay stable because kind is immutable.
- Cost: two layouts are readable until every Library has run `office.init`
  (or the standalone migration) once, and a destination collision keeps
  its legacy copy until a person resolves it.


## The scribe is watched and corrected in a small window

**Recorded:** 2026-11-01 · approved by the user in conversation
**Code anchors:** none yet; [specs/recall.md](./specs/recall.md#the-scribe-window)

### Context

A scribe the user cannot watch is a black box: a misread line surfaces
only when a later recall answer is wrong. The user asked for a small chat
window where they see what the scribe writes and can ask it questions,
and wanted a Claude desktop app mod in particular.

### Decision

- One Scribe screen in the web app, sized for a narrow PWA window, and a
  Claude desktop mod pane beside the chat for the current session, both
  views over the same Library and tools.
- Corrections are new `correction` entries naming the corrected anchor;
  readers apply them. The log stays append-only.
- An open window tightens that session's debounce; **Capture now** covers
  the rest.

### Consequences

- The user can catch and fix capture errors while the conversation is
  fresh, and corrections feed the rules page.
- `correction` is a new entry kind that the brief, digest, recall and feed
  must honour; an older reader would show corrected entries uncorrected.
- The ask box in the web app makes a model call on the app server, under
  the existing privacy routing.
- Cost: more passes while a window is open, and one model call per
  question.

## One scribe per user, queried through recall

**Recorded:** 2026-11-01 · approved by the user in conversation
**Code anchors:** `office-core/src/domain/source/conversation/recall.rs`
(session context, stored refs, selection), `office-tools/src/tools/recall.rs`
(read-only dispatch and live task status); [specs/recall.md](./specs/recall.md).
Orbit backend built in ORB-20688; provider UI/client remains planned.

### Context

The live scribe ([below](#a-live-scribe-extracts-the-conversation-while-it-runs))
was specified as constellation hooks. The user works across many
conversations at once and loses track of where something was discussed,
what was decided and whether it landed, and asks orchestrators again. They
asked for the same scribe to be present in every conversation, to tell
them apart, and to answer such questions itself, pointing back to the
conversation, without bothering an orchestrator.

### Decision

- Ship the hooks and commands as a per-user provider plugin from this
  repository (`plugin/claude-code/`), not as per-project hooks. The Orbit
  plugin stays the only Library writer; the provider plugin is a client.
- Give each conversation source the context to recognise it later:
  project, branch, start and last-active times, a resume command and a
  short title (scribe-proposed, user-overridable).
- Extract task, PR and commit references from captured lines
  deterministically; resolve their status live at question time, never
  store it.
- Answer through a read-only, model-free `office.recall` tool and a
  low-cost `recall` subagent that cites every claim and says "not
  captured" rather than guess.

### Consequences

- Questions about past conversations no longer cost an orchestrator's
  context; the current conversation receives only the answer.
- Recall is only as complete as capture: sessions before installation
  need `/scribe backfill`, and Codex sessions are missing until their
  adapter lands. `coverage` makes the gaps visible.
- The scribe-proposed title is a second model-authored field, marked
  `title_by: scribe` and replaced by any user title.
- Cost: one low-cost model call per question, plus live Orbit and GitHub
  reads for the references in the result.

## A live scribe extracts the conversation while it runs

**Recorded:** 2026-11-01 · approved by the user (PR #250)
**Code anchors:** incremental passes and `Note:`/`Scribe:` commands built
in [ORB-20685] (`office-core/src/domain/source/conversation.rs`,
`office-tools/src/tools/sources/capture.rs`); the rules page, answers to
questions and scribe handoff built in [ORB-20686] (the same files,
`office-core/templates/scribe-rules.md` and the `scribe` page field in
`office-core/src/domain/knowledge/page.rs`); the switch, trigger and
feedback have none yet; [specs/sessions.md](./specs/sessions.md#live-scribe)

### Context

SessionEnd extraction ([below](#conversation-evidence-comes-from-session-transcripts-on-a-cheap-crew))
moved capture to a cheap crew, but only after the session closes, so the
orchestrator still captures by hand during the session. On 2026-10-31,
after comparing claude-mem's always-on observer, the user asked for a
dedicated scribe instead: switched on per session, reading only the
conversation threads, following rules the user writes, and commandable by
the user or the orchestrator. claude-mem observes every tool call and lets
a model decide what to keep; the user wants the conversation only.

### Decision

- Run the existing extractor incrementally while a per-session switch is
  on, from the provider's end-of-turn hook, debounced, plus a final
  SessionEnd pass. No new tool: `office.capture` gains `from_line` and a
  prefix hash so a growing transcript appends instead of versioning.
- What to note is a Library page the user owns (`dom-scribe-rules`). Rules
  narrow and steer; they never widen the guard.
- Relax the cue guard in two narrow, deterministic ways: `Note:` commands,
  and a short affirmative or option choice answering the immediately
  preceding assistant question, recorded with both lines verbatim.
- Allow an opt-in, marked, anchor-citing scribe handoff so the orchestrator
  can stop maintaining it.

### Consequences

- The orchestrator's per-turn capture work goes away while the scribe is
  on; the log is current during the session, not only after it.
- Answer-pair decisions accept a small risk of reading a reflexive "ok" as
  consent. The checks (adjacent question, ends in `?`, no uncertainty
  markers, closed affirmative list) bound it, and the question is quoted
  beside the answer so a reader can judge.
- The scribe handoff is the first model-authored entry in the log. It is
  marked, cited and replaceable, and off by default.
- Cost: one low-complexity pass every few turns per scribed session, each
  reading only new lines. Claude Code only until a Codex adapter lands.

## Conversation evidence comes from session transcripts on a cheap crew

**Recorded:** 2026-10-30 · [ORB-20242]
**Code anchors:** `office-core/src/domain/source/conversation.rs`,
`office-tools/src/tools/sources/capture.rs`; [specs/sessions.md](./specs/sessions.md#transcript-extraction)

The user requested moving conversation-source writing out of the expensive
orchestrator's per-turn work, preserving verbatim user directions and recording
the conversation provider/model. This supersedes the earlier rejection of a
cheap-crew job for conversation evidence: the host stages raw transcripts, and
the primary trigger is SessionEnd rather than a daily summary cron.

Extend `office.capture` instead of registering another tool. The cheap crew
selects numbered scrubbed transcript lines; deterministic code supplies quotes
and demotes unclear/non-user decisions to open questions. It cannot emit a
handoff or resolution. A local conservative path supports private transcripts
without disclosing them to a crew. The session's last reported model and adapter
provider are evidence provenance, not authenticated identity or the extractor
model. Existing source provenance is nullable.

Run on the session host, inside existing filesystem grants after private
staging. The constellation SessionEnd hook queues one low-complexity task/job;
a disabled half-hour recovery auto-task catches missed staged sessions. No
model is hardcoded: dispatch resolves the configured low-complexity pool.
Session id plus raw hash makes retries no-ops; a changed hash appends new
entries and journals a previous-version snapshot under the existing writer
lock. Old anchors and orchestrator handoffs survive transcript changes.

The hook/staging sweep belongs to constellation. Per ARCHITECTURE.md's
integrator-owned-file rule, registering the shipped auto-task in `plugin.yaml`
is a separate integration task. The definition is shipped and validated here,
but is not advertised as installed or enabled. Until integration, manual logs
remain the supported fallback. This is the scoped deviation from the requested
fully wired automation; it does not grant a feature task ownership of the
shared manifest. No adopted standard is relaxed.

Cost: the first adapter supports Claude Code JSONL only; other providers refuse
until their adapters land. The explicit-cue guard can retain a real decision as
open, which is preferable to invented authority. Credential scrubbing covers
known shapes, not arbitrary prose secrets. Normal-preview extraction uses the
cheap crew; private extraction is conservative local work. The brief can lag
until an asynchronous job finishes and a later digest may pick up late evidence.


## Library proposals are partitioned by status

**Recorded:** 2026-10-30 · [ORB-20247]
**Code anchors:** `crates/office-core/src/domain/proposal.rs` (`ProposalStatus`, `parse_record_path`, `record_path`, `with_outcome`); `crates/office-tools/src/tools/proposals/reconcile.rs`, `accept.rs`, `create.rs`; `crates/office-store/src/repository/library.rs` (`move_proposal`, `proposal_pages`); `app/packages/server/src/features/approvals/service.ts::dismiss`; [specs/proposals.md](./specs/proposals.md#record-and-lifecycle), [specs/library.md](./specs/library.md#layout)

### Context

Every proposal's evidence page sat flat in `library/proposals/<id>.md`
whether it was waiting, accepted or dismissed, so the folder becomes a long
list. Daniel asked for proposals to be partitioned by status like sources
(ORB-20243). Unlike a source's kind, a proposal's status changes, so its page
has to move. Acceptance also wrote a separate `<id>-accepted.md` receipt.

### Decision

The page sits in the folder for its status, named in the app's words:
`proposals/open/<id>.md` while the Orbit task is `proposed`, and
`proposals/accepted/YYYY/MM/<id>.md` (`done`) or
`proposals/dismissed/YYYY/MM/<id>.md` (`rejected`) once closed, dated by the
closing month so closed folders stay bounded. Any other task status keeps the
page where it is. The record stays the Orbit task and nothing links to a page
by path, so a move is safe: lookups resolve by proposal id in any status
folder and in the legacy flat layout.

- `office.proposal-create` writes under `open/`.
- `office.proposal-accept` moves the page to `accepted/` in the commit that
  records the outcome: the receipt becomes the page's `## Outcome` section
  instead of a separate file.
- Dismissal stays the app's Orbit rejection (it carries who dismissed and
  why); the app then calls a new app-only tool, `office.proposal-reconcile`,
  which moves the page to `dismissed/` in one commit.
- `office.proposal-reconcile` (`{}`, mutating, `mcp_scope: none`) also moves
  pages whose task closed outside the app and migrates legacy flat pages,
  folding a legacy receipt into the accepted page. It reads one complete
  proposal task list and is idempotent: a page already in its folder is left
  alone, so a re-run changes nothing.
- A move is one journaled Library write (journal version 9,
  `proposal_move`): the new page and the removal of its old paths are
  committed together and replayed together after a crash.

A dedicated tool was chosen over folding dismissal or reconcile into
`office.proposal-accept`, whose name, input and `accepted: true` response
contract would otherwise change meaning. Adding it touched the
integrator-owned registry row, `tools/proposals/mod.rs`, the manifest block
and ARCHITECTURE.md's tool list; this task's scope named the proposal tool
family and manifest, so it made those one-line changes rather than splitting
off an integration task.

### Consequences

- Waiting proposals are one short folder; closed ones are browsable by month,
  and a closed page carries its outcome.
- Cost: dismissal and its page move are two calls, so a failed reconcile
  leaves a dismissed page under `open/` until the next reconcile; the task
  is still the record. Operator closures catch up only at the next app
  dismissal or a manual `orbit tool run office.proposal-reconcile`.
- Cost: reconcile refuses when the proposal task list is truncated (more
  than 1000 proposal tasks) rather than guess at missing statuses.
- Two layouts are readable until reconcile has run once on a Library.

## The app reports missing approval authority instead of granting itself Operator

**Recorded:** 2026-10-31 · [ORB-20245]
**Code anchors:** `app/packages/server/src/features/approvals/service.ts` (`refusal`); `app/packages/web/src/screens/today/approvals.ts` (`problem`, `NOT_PERMITTED`); `app/packages/server/src/orbit/client.ts` (module header); [specs/proposals.md](./specs/proposals.md#record-and-lifecycle), [specs/plugin-surface.md](./specs/plugin-surface.md#agent-authority-inventory)

### Context

Accepting a proposal on Today failed with "Please try again" and retrying
never helped. `office.proposal-accept` is a mutating, `mcp_scope: none` tool
that is not `agent_callable`, so Orbit admits only Operator or Runner. The
installed app runs `orbit tool run` with no `ORBIT_OPERATOR` and resolves as a
local CLI holding Agent, so Orbit refuses it with `capability_denied`. Dismiss,
send-back and approve (`orbit.task.update`, `office.artifact-promote`) and
`gateway.http-send` share the gap; routine control and auto-task mint already
report it as `not_permitted`. The proposal spec said "only the app can call
it", while the plugin-surface inventory says Operator/Runner only.

### Decision

- The app does not hold a blanket Operator grant: no `ORBIT_OPERATOR` in the
  service templates, as the client header and the earlier operator decision
  already require. The approval tools stay out of `agent_callable`, whose
  consent also admits agent sessions, so agents still cannot accept.
- Scoped authority for the user-approval tools must come from Orbit as a
  generic, host-issued, audited application identity bound to this workspace
  and an explicit tool list. Orbit 0.40.0 offers only Agent, Operator and
  Runner, so the Orbit prerequisite is filed in the Orbit workspace
  (ORB-20362); the Office wiring follows once it lands (ORB-20363).
- Until then, every approval route reports Orbit's authority refusal
  (`capability_denied`, `policy_denied`, from Orbit's stderr or a tool
  envelope) as the reason `not_permitted`; accept-all names it per page.
  Today shows one plain sentence: the app isn't allowed to do this yet,
  nothing changed, and trying again won't help.
- An operator accepts from the office workspace with `ORBIT_OPERATOR=1`
  (STD-05@1 §R3: authority is held by the operator, not inferred by the app).

### Consequences

- Proposals, sends and dismissals still cannot be completed from the app; the
  user now sees why instead of retrying.
- When the Orbit application identity lands, the app wires its credential to
  the Orbit child process, a server-route test exercises accept through the
  real authority path, and this decision and the proposal spec are revised.

## Recall resolves PR refs live through a granted `gh` source

**Recorded:** 2026-11-01
**Code anchors:** `office-core/src/ports/pr.rs` (`PullRequests`, `PrView`,
`PrError`), `office-store/src/pr.rs` (`GhPullRequests`),
`office-tools/src/tools/recall.rs` (PR arm of `live_ref`),
`plugin/.orbit-plugin/plugin.yaml` (`spec.requires.programs`);
[specs/recall.md](./specs/recall.md#live-pr-state-built).

This task (ORB-20710) is the narrow integrator-ownership exception for the
one-line `gh` entry in `spec.requires.programs`, authorized by the task
itself following the calendar precedent above; it is not a general
exception for other manifest or architecture edits. No crate or internal
dependency edge is added: the port follows the existing pattern (one file
per port in `office-core`, a fake in `office-testkit`, the real adapter in
an adapter crate, wired in the binary's `ports()`), and existing complete
`Ports` literals receive the refusing `UnavailablePullRequests` adapter.

- The adapter runs `gh pr view <owner/repo#n> --json
  state,mergedAt,mergeCommit` once, never retried, under
  `office_common::process::run_captured_with_deadline` with a 15-second
  deadline. The child environment is cleared except `PATH`, `HOME`,
  `XDG_CONFIG_HOME`, `GH_TOKEN` and `GH_HOST`, so `gh` can authenticate
  without inheriting Office or Orbit authority variables.
- Resolved refs report `status` (`open`/`closed`/`merged`), `merged_at`
  and `merge_commit`. Failures are `unresolved` with a cause-naming fixed
  reason (`pr_source_unavailable`, `pr_lookup_failed`,
  `invalid_pr_response`, `invalid_pr_ref`) and never fail the call or echo
  program output.
- **Operator step:** adding `gh` to `spec.requires.programs` changes the
  granted programs, so the operator must upgrade the office plugin and
  re-enable it for `gh` to be granted. Until then PR refs read
  `unresolved` / `pr_source_unavailable`.

## Recall answers and scribe control: authority and private withholding

**Recorded:** 2026-11-01 · [ORB-20756]
**Code anchors:** `office-tools/src/tools/recall/answer.rs`,
`office-tools/src/tools/sessions/scribe_control.rs`,
`office-core/src/domain/scribe_control.rs`,
`office-core/src/ports/scribe_control.rs` (`ScribeControls`),
`office-store/src/scribe_control.rs` (`ScribeControlStore`);
[specs/recall.md](./specs/recall.md#answering-in-each-host),
[specs/sessions.md](./specs/sessions.md).

- **No `agent_callable` for either tool.** `office.recall-answer` costs a
  model call and `office.scribe-control` writes plugin state, and the
  installer's consented agent verb set is exactly the four verbs in
  "Scoped agent preparation" above (repair refuses any other set). Both
  therefore stay Operator/Runner tools: the web window and the user's own
  `orbit` CLI reach them, and a plugin hook running inside an Orbit-managed
  agent session is refused and fails open on its local flag and heartbeat
  files. Widening the verb set is a separate, consented change.
  **Superseded for `office.scribe-control` (and `scribe-append`)** by
  [Scribe v2 S5](#scribe-v2-s5-scribe-control-stays-agent_callable-a-managed-session-cannot-turn-scribing-on)
  below: [ORB-20856] granted the verbs because the plugin calls Orbit as
  `local-cli` holding `[agent]`, and S5 keeps the grant while refusing a
  turn-on under managed-task context in the backend instead.
  `office.recall-answer` is unchanged.
- **Private-derived claims are withheld.** recall-answer is MCP-visible, so
  it follows "Private content is withheld from MCP-visible read tools":
  without an open-weights route private sources are listed by reference
  only and never sent to a model; with one they are sent on the private
  route, and any claim citing them returns `text: null`,
  `content_withheld: true` with its citations. Public claims are returned
  in full.
- **Control state is a new port.** Scribe control is Office-owned plugin
  state, not Library evidence, so it lives in its own SQLite file in the
  plugin state directory behind `Ports.scribe_control`, following the PR
  source precedent: existing complete `Ports` literals receive the refusing
  `UnavailableScribeControls`, the testkit builder a shared in-memory fake.
  No crate or dependency edge is added. Records untouched for 30 days are
  dropped on the next write.
- **Integrator follow-up:** `ARCHITECTURE.md` still lists both tools as
  planned and does not name `scribe_control.sqlite`; that file is
  integrator-owned and is updated in its own task.

## Scribe v1: commanded capture into one tagged file per session

**Recorded:** 2026-11-01 · [ORB-20690]
**Code anchors:** none yet; [specs/scribe.md](./specs/scribe.md)

### Context

The live scribe, recall, the desktop pane and the web window were built in
one push. In use, the pane's answers came back "not captured": automatic
passes were invisible, recall's search was too narrow, and the subagent was
refused by auto mode. Daniel's direction (2026-11-01): "I think we got ahead
of ourselves. lets go one step at a time."

### Decision

v1 is a dedicated, cheap `scribe` agent that Daniel or an orchestrator
commands. `/scribe` parses the transcript and scribes only the messages not
yet scribed. `/scribe note` takes direct notes and `/scribe ask` answers with
citations. Output is one `SCR-<dd-hhmm>-<title>.md` per session under
`sources/conversation/<yyyy>/<mm>/`: front matter carries who, where,
provider, cursor and topics, and the body has one tagged, verbatim entry per
line. The model only proposes entries; a model-free append step checks
quotes, tags and topics and writes. Automatic Stop/SessionEnd passes are off
by default. The web Scribe screen (ORB-20696) was parked here; Daniel
unparked it on 2026-11-01 and it is built over v1 (see
[Web Scribe screen over scribe v1](#web-scribe-screen-over-scribe-v1)).

### Consequences

- Answers come from grep over plain files, so they are cheap and exactly
  cited, and an orchestrator can check them without a model.
- Capture is only as current as the last `/scribe`; there is no background
  pass to rely on.
- Two conversation source layouts coexist until a migration is chosen.

Cost: one more source layout, and the built recall surfaces sit unused until
v1 has proven what to keep.

## Scribe v1 parse and append: where files live and who may take a tag

**Recorded:** 2026-11-01 · [ORB-20780]
**Code anchors:** `office-core/src/domain/source/conversation/scribe.rs`,
`office-tools/src/tools/sessions/scribe_pending.rs`,
`office-tools/src/tools/sessions/scribe_append.rs`,
`office-core/src/ports/library.rs` (`scribe_parse_path`,
`write_scribe_parse`, `scribe_files`, `write_scribe_file`, `host_name`),
`office-core/src/ports/clock.rs`
(`time_zone`), `office-store/src/scribe_parse.rs` (`ScribeParseStore`);
[specs/scribe.md](./specs/scribe.md#pipeline).

- **A message is one transcript record.** `office.scribe-pending` reuses
  the conversation adapter unchanged and joins the kept lines of one record
  with newlines; the nth such message is `m-<nnnn>` (from `m-0001`). Records
  are only appended, so a longer transcript only adds ids. A private
  session (configured private or holding a scrubbed credential) is
  withheld exactly as the capture preview is: nothing is written or returned.
- **Session facts ride beside the message file.** `office.scribe-append`
  takes no transcript, so pending also writes `<session-id>.meta.json`
  (provider, model, host, project, cwd, branch, transcript, start) next to
  the `.jsonl`, both owner-only in a `0700` directory. Append refuses with
  `scribe_not_parsed` without them.
- **Parse files live in the Library's ignored state (ORB-20820).** They
  are `<library>/.office/state/scribe/<session-id>.jsonl` and
  `.meta.json`, resolved from the configured Library root. They were under
  the workspace's `.orbit/tmp/scribe/`, which the installed plugin cannot
  write: Orbit refuses any plugin write grant under workspace `.orbit`, so
  every unparsed session failed under the sandbox. The new directory is
  inside the existing `{{workspace}}/library` grant, so the manifest and
  consent are unchanged, and the Library's `.gitignore` already ignores
  `.office/state/`. `parsed` stays workspace-relative (for example
  `library/.office/state/scribe/<id>.jsonl`). Rejected: `{{plugin_state}}`
  (`parsed` would stop being workspace-relative, and agents never read
  plugin state) and `{{workspace}}/.office` (a new grant, and the workspace
  repo would have to ignore it). The Claude Code plugin's own hook files
  stay under `.orbit/tmp/scribe/`: the provider hooks write them
  unsandboxed.
- **SCR files are Library files, not indexed sources.** They commit through
  a new journaled write kind (version 10) that writes the file and removes
  its previous name in one commit, under the writer lock, refusing a stale
  read (`scribe_changed`). Reindex keeps scanning only `source.md`, so the
  existing conversation sources and their readers are untouched.
- **Front matter is fixed-shape.** Every key is always present on one line,
  so an entry's file line (`#L<n>`, what grep and `resolves` cite) never
  moves when topics merge or the file is renamed. An untitled session has
  `title: null` and a slug from Daniel's first message; a title change
  renames the file and records the old id in `aliases` (the same title again
  does nothing).
- **Authors are fixed by the guard, not the page.** `decision`, `direction`,
  `todo`, `question` and `reminder` come only from user messages or
  `by: user` notes ("Assistant lines are scribed only for landed and fact");
  `agent-decision`, `pending`, `resolved` and `incident` need `by: <crew>`
  on an assistant message or agent note; any other tag, including one the
  page adds, takes either. The page's `Tags`, `Topics` and `Capture`
  sections steer the agent and set the allowed names; a missing section keeps
  its default.
- **Local time comes from the clock port.** `Clock::time_zone` (default
  UTC) names the host's IANA zone; the binary reads `TZ`, then the
  `/etc/localtime` link. Names and entry times use it; message files keep
  the transcript's RFC 3339 times.

## Scribe staging strips tool payloads before the 8 MiB cap

**Recorded:** 2026-11-01 · [ORB-20849]
**Code anchors:** `plugin/claude-code/scripts/scribe_v1.py::V1.stage`,
`_strip_record`, `_strip_block`, `_credential_fragments`;
`crates/office-core/src/domain/source/conversation.rs::parse` (keeps only
`text` blocks), `crates/office-store/src/repository/library.rs::load_session`
(the 8 MiB refusal); [specs/scribe.md](./specs/scribe.md#staging-tool-payloads-are-not-staged)

### Context

Long sessions failed `office.scribe-pending` with
`session_transcript_too_large` because the plugin staged the provider
transcript whole and most of its bytes are tool inputs and results the
parser never reads. The 8 MiB cap itself stays: it bounds what Office will
read and scrub in one call.

### Decision

The plugin stages only the records the parser reads, per record and with no
whole-file threshold: record envelope and message role/model, string
content and `text` blocks verbatim; non-text blocks as minimal stubs with
payloads dropped; other large record fields dropped. Message ids are
therefore identical to an unstripped parse, and a growing transcript's
unchanged staged prefix stays byte-identical. Credential-shaped fragments
in dropped payloads are kept verbatim in a parser-ignored `_preserved`
field, mirroring `conversation::scrub`, so such a session is still marked
private and withheld from the scribe agent. When the conversational text
alone exceeds the cap, the refusal stands for the whole session — a
windowed read of only the messages after `scribed_through` would need a
Rust reader that can assign prefix-dependent ids without the full file,
and is left as future work; scribing in passes as a session grows is the
current remedy.

### Consequences

- Tool-heavy sessions (tens of megabytes as provider files) scribe through
  `/scribe` and `/scribe backfill` with stable `m-<n>` ids.
- The staged file's digests describe staged bytes, not provider bytes.
- Cost: a Python mirror of `scrub`'s credential shapes must be kept in step
  with `office-core`, and a session whose text alone passes 8 MiB still
  cannot be scribed in one call.

## Web Scribe screen over scribe v1

**Recorded:** 2026-11-01 · [ORB-20696]
**Code anchors:** `app/packages/server/src/features/scribe/index.ts`
(`loadFeed`, `addNote`, `ask`), `app/packages/server/src/features/scribe/model.ts`
(`noteEntry`, `answerOf`), `app/packages/web/src/screens/scribe/`,
`crates/office-tools/src/tools/sessions/scribe_query.rs` (result keys
`title`, `project`, `scribed_through`); [specs/scribe.md](./specs/scribe.md#relation-to-what-is-built),
[specs/web-app.md](./specs/web-app.md#screens)

### Context

Daniel unparked the web Scribe screen for the scribe v1 demo and re-scoped
it from the retired live scribe onto SCR files. The server feature may call
only `orbit tool run`, so it cannot read SCR front matter or the parse state.
Two inputs were missing: the session title and project to group the feed,
and the session cursor that `office.scribe-append` needs as `through`.

### Decision

- `office.scribe-query` results gain three keys from the line's SCR front
  matter: `title`, `project` and `scribed_through`. The change is additive;
  no key is renamed or retyped.
- A note from the screen is appended with `through` set to the session's own
  `scribed_through`, so it never marks unread messages scribed. A session
  with no cursor (never scribed) gets no note from the screen; `/scribe note`
  holds one for the first catch-up instead.
- The note is parsed as `/scribe note` parses it (`by: user`, leading
  `#tag` and keys). Without a leading tag the screen sends the tag chosen in
  its menu, preselected by a few model-free cues; it does not call the
  scribe agent to pick one.
- The feed is the general `office.scribe-query` over the last 30 days. The
  general query keeps the newest lines up to its limit in (time, path,
  line) order and truncates the oldest, with `count` reporting every match;
  the canned modes keep their oldest-first truncation. A window over the
  1000-line limit narrows to 7 days, then to today, to report a tighter
  range, and an over-cap final day still keeps its newest lines. The feed
  reports that window as `since` with `total`/`shown` naming the
  truncation. Resolved pendings and superseded preferences are hidden, as
  the tool hides them.
- The ask box shows `office.scribe-ask` claims after checking each quote
  once more against the shortlisted line text the same response carries.
  The server never reads Library files.

### Consequences

- The feed, the canned views and the ask box add no model call beyond the
  one `office.scribe-ask` makes.
- Cost: a session that was parsed on another host (no parse state here)
  refuses a screen note with `scribe_not_parsed`; the note waits for a local
  `/scribe`.

## Private-route token bound to its endpoint origin

**Recorded:** 2026-11-01 · [ORB-20884]
**Code anchors:** `crates/office-core/src/domain/endpoint.rs` (`BoundToken`,
`TokenBinding`), `crates/office-models/src/routing.rs` (`private_endpoint`),
`crates/office-tools/src/tools/status.rs` (`open_weights_token_binding`),
`app/packages/server/src/features/settings/private-route.ts`
(`savePrivateRoute`), `app/packages/server/src/features/settings/secret.ts`;
[specs/models-and-privacy.md](./specs/models-and-privacy.md#the-private-routes-token)

### Context

Saving the private route with the token left empty kept the saved token,
whatever the new endpoint was. Moving from a company's endpoint to another
provider therefore sent the company's token to that provider, starting
with the capability check. A direct `office.config-set` reached the same
state, since the token and the endpoint were stored independently.

### Decision

- The secret `open_weights_token` holds one JSON record, `{"origin",
  "token"}`, written in one `orbit plugin secret set` call on stdin. The
  binding lives with the credential: Library config can be written by
  `office.config-set`, so a binding there could be forged by one call.
- Every model call pairs the token only with an endpoint whose origin
  (scheme, host and port) equals the bound one, from the same call's
  settings and context. Anything else is sent unauthenticated. Ordering of
  the Settings writes is not relied on for safety.
- A value that is not the record (a bare token from the installer or a
  hand-typed `secret set`) is unbound and never sent. Settings asks for it
  again; the installer is unchanged, since it knows no endpoint.
- `office.status` gains `open_weights_token_binding` (additive); Settings
  words the token field from it and offers *Remove the saved token*
  (`clear_token`, which runs `orbit plugin secret rm`).

### Consequences

- An installer-stored token stops being sent until it is entered in
  Settings (or stored as the record by hand).
- Returning to the token's original origin with the field empty reuses it
  there, since that is the origin it was approved for.

## Scribe parse: task notifications, side-session briefs and worktree project names

**Recorded:** 2026-11-01 · [ORB-20919]
**Code anchors:** `crates/office-core/src/domain/source/conversation.rs::strip_harness`,
`parse` (brief role, title record), `crates/office-core/src/domain/source/conversation/scribe.rs::planned`
(slug source), `crates/office-tools/src/tools/sessions/scribe_pending.rs` (`project` input),
`plugin/claude-code/scripts/scribe_v1.py::repo_project`, `_is_scribe_only_session`;
[specs/scribe.md](./specs/scribe.md), [specs/sessions.md](./specs/sessions.md#adapter-and-provenance)

### Context

The 2026-11-01 regenerated backfill still polluted SCR files three ways:
`<task-notification>` user records kept as Daniel's text (so `/scribe
backfill` driver sessions were not scribe-only), desktop side sessions
named from their orchestrator-written opening brief, and sessions in a
git worktree recording the worktree directory as `project`/topic.

### Decision

- `<task-notification>` blocks are stripped with `<system-reminder>`, in
  the Rust adapter and in the plugin's scribe-only check.
- A side session's opening brief keeps its message and `m-<n>` id but is
  an assistant message: it never takes Daniel's tags and never names the
  file. The slug comes from the transcript's title/summary record, else
  the brief's task-title line, else Daniel's first real message. This
  supersedes the slug-from-Daniel's-first-message rule above for side
  sessions only; all other sessions keep that rule.
- `project` is the git repository name (the main worktree's top-level
  directory). For a session at a repository or linked-worktree root,
  the plugin resolves it with `git rev-parse
  --git-common-dir` and passes it to `office.scribe-pending`; when git
  cannot resolve it that way (a non-git directory, a plain
  subdirectory, or a worktree removed by GC)
  the `cwd` basename fallback stands, reproducing the worktree slug in
  that case. The backfill `--project` filter uses the same name.

### Consequences

- Driver sessions of only `/scribe` plus task notifications are
  scribe-only and write no SCR file.
- Side-session SCR files are named for their own work, and no brief text
  is attributed to Daniel.
- Worktree sessions record the repository as `project`, and default
  topics carry no worktree slug.

## Scribe: real side-session preamble, custom titles and GC'd worktrees

**Recorded:** 2026-11-01 · [ORB-20935]
**Code anchors:** `crates/office-core/src/domain/source/conversation.rs::is_side_session_brief`,
`brief_first_sentence`, `parse` (latest `custom-title`),
`crates/office-core/src/domain/source/conversation/scribe.rs::planned_slug`, `project_of`,
`crates/office-tools/src/tools/sessions/scribe_pending.rs` (`custom_title` input),
`plugin/claude-code/scripts/scribe_v1.py::repo_project`, `V1.stage`, `V1.pending`;
[specs/scribe.md](./specs/scribe.md)

### Context

The regenerated backfill after ORB-20919 still named SCR files
`SCR-…-background-from-the-session-that-started-this-on.md` and recorded
worktree directories as `project`. The earlier decision had quoted the
desktop brief preamble without its parentheses and colon, so the match
never fired for the real text; it also left GC'd worktrees on the `cwd`
basename fallback.

### Decision

- Both brief forms match, case-insensitively, ignoring the parentheses
  and the trailing colon. This supersedes the preamble quote in the
  ORB-20919 decision above.
- The latest `custom-title` record's `customTitle` is the slug and
  title source for every session; the stager passes it to
  `office.scribe-pending` as `custom_title`. A side session without one
  falls back to its title/summary record, then the brief's task-title
  line, then the brief's first prose sentence after the preamble.
- A `<repo>/.claude/worktrees/<name>` cwd names `<repo>` by that path
  rule, applied before the git resolution and the basename fallback, so
  a worktree removed by GC still records its repository.

### Consequences

- Real desktop side-session briefs are assistant text, take no Daniel
  tags, and never name a file from their preamble.
- Sessions with a set custom title are named from it, whatever kind of
  session they are.
- GC'd Claude Code worktrees record the repository as `project`.

## Scribe: placeholder session titles are no title

**Recorded:** 2026-11-01 · [ORB-20950]
**Code anchors:** `crates/office-core/src/domain/source/conversation/scribe.rs::placeholder_title`,
`named_title`, `planned_slug`; regression test
`crates/office-tools/tests/scribe.rs::placeholder_custom_title_is_absent_and_a_real_title_still_names_the_file`;
[specs/scribe.md](./specs/scribe.md)

### Context

The desktop app titles an unnamed session `New session`. Once the latest
`custom-title` record named the SCR file (the ORB-20935 decision above),
two sessions carrying that placeholder were filed as `…-new-session.md`,
though one's first message was a far better name.

### Decision

- A custom title or session title of `New session` is no title, whatever
  its case and with or without one trailing number. The slug skips it
  exactly as it skips a blank title.
- The match is whole-title: `New session planning` is a real title and
  still names the file.
- This refines the custom-title rule above for the placeholder case only.

### Consequences

- A session whose latest custom title is the placeholder takes its slug
  from Daniel's first real message. A side session takes its brief's
  task-title line or first sentence, as it does without any title.
- An earlier real custom title is not consulted once the latest record
  is a placeholder, since only the latest `customTitle` is kept.

## QA fixture: a named, audited test-fixture exception for backend behaviour on disposable data

**Recorded:** 2026-11-01 · [ORB-20946]
**Code anchors:** `Makefile::qa-exec`; regression test
`crates/office/tests/cli.rs::qa_exec_fixture_runs_init_in_its_temp_root`;
runbook [../../qa-exec.md](../../qa-exec.md)

### Context

Full QA recorded 47 required scenarios as BLOCKED because QA agents
cannot call mutating Operator/Runner-only office tools through Orbit.
Their backend behaviour on disposable data is still worth checking each
sweep. STD-05@1 §R3 requires permission to be decided by one
authorization function on every entry surface, not by which surface
lists a tool, and STD-05@1 §R4 requires any escape hatch that raises a
caller's authority to be explicit, named and audited. Calling
`office orbit-tool` directly from a shell would otherwise look like
such a hatch with none of those properties: dispatch has no
authorization step of its own because Orbit is that function for the
real surface.

### Decision

- The `make qa-exec` fixture is an explicit, named **test-fixture
  exception** under STD-05@1 §R3–R4. It is not an agent authority
  route, and it leaves the Orbit authorization of the real surface
  unchanged: no MCP scope, allowlist or plugin manifest changes.
- It runs only a freshly built `office` binary, only against a temp
  root it creates itself (or a root carrying its own fixture marker
  under the system temp directory, so a QA area can chain `init` then
  a second tool in the same disposable root). It never runs against a
  real workspace, Library or task store, and it refuses any supplied
  root outside its own temp directory and any input path that is
  absolute outside that root or escapes it with `..`.
- The call context carries only the root keys the backend reads —
  `workspace_root`, `plugin_state` and `library_dir`, all inside the
  temp root. It carries no identity or operator claim, never sets
  `ORBIT_OPERATOR`, runs with a cleared environment and an empty
  `HOME` inside the root, and makes Orbit callbacks unavailable
  (`ORBIT_BIN=/bin/false`, no callback descriptor or `ORBIT_*`
  callback state), so no run can reach a real task store.
- Every run is audited under STD-05@1 §R4: an audit line naming the
  tool, the temp root and the UTC time is appended inside the temp
  root (`qa-exec-audit.log`) and printed on the target's stdout, so a
  fixture run is never indistinguishable from an ordinary call.
- This is an accident guard on a shared OS account, not a security
  boundary against that account (STD-05@1 §R5): a caller who can run
  `make` can run the binary directly. No other control is relaxed
  because this fixture exists.

### Alternative considered

Hand each refused scenario to an operator to run through the existing
operator path in a throwaway workspace each sweep. That needs no
exception, but it puts a person in every full-QA sweep for behaviour
that disposable-data automation can check.

### Consequences

- QA agents can check the backend behaviour of tools Orbit refuses to
  agents, on throwaway data, under this documented exception. A pass
  says nothing about Orbit authorization, which the fixture does not
  exercise.
- Tools whose handlers need Orbit callbacks, network or model routes
  fail in the fixture by design; that failure is fixture evidence
  about the disposable setup, not about the real surface.

Cost: one Makefile target, one runbook and one end-to-end test to
keep honest, and a standing exception a reviewer must keep bounded to
temp roots — if the fixture ever needs a real root, that is a new
decision, not an extension of this one.

## Scribe v2: opt-in sessions, marker-only tags, ledgers and the SCR as handoff

**Recorded:** 2026-11-01 · [ORB-21083]
**Code anchors:** none yet; [specs/scribe.md](./specs/scribe.md),
[specs/sessions.md](./specs/sessions.md#scribe-v2-the-latest-scr-is-the-handoff)

### Context

In use, scribe v1 was noisy. Every session was scribed, most `why` entries
had no rationale, and the feed listed dozens of untitled sessions.
Daniel's direction of 2026-11-01, given in the orbit-office on-call
session ([ORB-20840]), is captured verbatim in Library note
`src-20261101-scribe-v2-redesign-direction`. His decision to retire
`sources/note` with no window is in `src-20261101-retire-sources-note-outright`.
His request that decision entries serve as precedent for a decision model
([ORB-21089], [ORB-21090]) came the same day. Four tasks built on v1 had
already shipped: [ORB-21063], [ORB-21065], [ORB-21075] and [ORB-21079].

### Decision

Each point traces to the direction (D1–D13 in
[specs/scribe.md](./specs/scribe.md#spec-scribe-v2)):

1. **Opt-in** (D1). Sessions default off. `/scribe on|off|update` is the
   surface. A Stop hook starts a pass every N minutes or N user turns,
   with no timer. Orchestrators may turn it on in an interactive session
   (`on_by: agent`) or ask.
2. **Markers, with a model-free trust rule** (D2). `#preferences`,
   `#decision` and `#scribe <tag>` count only in the user's own typed
   turns. Detection is in `office.scribe-pending`, and append refuses any
   marker it did not detect. Agent markers are recorded `by: agent` and
   never put a preference into effect.
3. **Two default tags** (D3), `preferences` and `decisions`. `why`,
   `pending`, `todo`, `landed` and the agent tags are removed. User action
   items become `owner:you` Orbit to-dos; agent work stays Orbit tasks
   cited from Next. `lesson` is not a default, and no sink writes polaris.
4. **Stores** (D4): an append-only decision ledger per scope and a
   preference page per scope
   ([store paths](#scribe-v2-store-paths-a-decisions-source-kind-and-templated-preference-pages)).
5. **Contradictions** (D5) are checked when a preference is proposed, and
   weekly by a disabled-by-default auto-task.
6. **One SCR per session** (D6) with rewritten Summary, Goals, Outcome and
   Blockers and Next, append-only Tags entries with `T<n>` ids, and
   `previous:` in its scope. The title comes from the Summary.
7. **The latest SCR is the handoff** (D7), per project and global, and the
   brief merges both. Handoff log entries retire after the window.
8. **Feed** (D8): one card per project plus one global card; history
   only through `previous:`.
9. **Tags in Library settings** (D9)
   ([tag definitions](#scribe-v2-tag-definitions-live-in-library-settings-app-saves-wait-on-orb-20363)).
10. **Scheduling markers** (D10) file a proposed task with `not_before`.
    The scribe never dispatches.
11. **Migration** (D11) maps the v1 commands and queries for 30 days and
    backfills preferences and decisions as proposals. *Amended 2026-11-02:
    no backfill; v2 starts clean
    ([S9](#scribe-v2-s9-no-v1-backfill-v2-starts-clean-the-tidy-up-is-its-own-operator-tool)).*
12. **`sources/note` is retired outright** (D12)
    ([below](#retire-sourcesnote-outright-into-the-decision-ledgers)).
13. **Ledger entries are precedent** (D13): question, options or
    `unrecoverable`, chosen, verbatim quote and ref, scope, context,
    `decided_by`, `decided_at`, `reverses` (with derived `reversed_by`)
    and source id, as one JSON object per line.

### Alternatives rejected

- **Keep scribing every session and filter the feed.** That keeps paying
  for passes nobody reads, and the noise was in what got recorded, not
  only in what was shown.
- **Let the scribe agent decide which lines matter, as in v1.** Inference
  produced the untagged `why` noise. Markers make the user's intent
  explicit and checkable without a model. Inferred capture stays as a
  per-tag opt-in that only proposes.
- **Honour markers anywhere in the transcript.** A pasted document or a
  tool result could then plant `#preferences`. Only typed user turns
  count, and paste-sized turns never apply directly.
- **A long-lived timer or daemon for cadence.** It runs when no one is
  talking and needs supervising. The Stop hook already fires on every
  turn.
- **Keep handoff entries beside SCRs.** Two handoffs per session would
  disagree. The SCR is rewritten anyway, so it is the current state.
- **Keep per-thread handoffs (ORB-21060) inside a project.** Daniel asked
  for one card per project. Concurrent sessions stay visible through
  `other_sessions`, and per-assignment state belongs on the assignment
  task.
- **Store `reversed_by` on the reversed entry.** That would edit an
  append-only ledger. It is derived on read.

### Consequences

- An unscribed session leaves no SCR and no handoff. Its record is its
  Orbit tasks.
- Headless sessions, including unattended on-call shifts, cannot be
  scribed. Their assignment task is their record.
- An unmarked paste can still pass as typed text. Default propose mode and
  the paste-size limit bound the harm to a proposal the user must accept.
- The decision model gets a schema now ([ORB-21090]), at the price of the
  scribe reading surrounding turns for question and options.

Cost: three shipped features (ORB-21063, ORB-21065, ORB-21075) are
reworked within weeks of landing. There is a 30-day window carrying both
the v1 and v2 surfaces, and two more Library stores to index.

## Scribe v2 store paths: a decisions source kind and templated preference pages

**Recorded:** 2026-11-01 · [ORB-21083]
**Code anchors:** none yet; [specs/scribe.md](./specs/scribe.md#tag-stores),
[specs/library.md](./specs/library.md#layout)

### Context

The direction named `sources/decisions/<project>.md` and
`sources/decisions/global.md` for decisions, and
`knowledge/preferences/<project>.md` and `knowledge/preferences/global.md`
for preferences, and asked that the paths be confirmed against the
Library model and template rules. The model puts sources at
`sources/<kind>/YYYY/MM/<source-id>/source.md`. It puts knowledge pages
only at `knowledge/domain/<id>.md` and `knowledge/runbooks/<id>.md`, with
the id prefix deciding the folder. `office.knowledge-write` refuses
anything else. "Decision log" already names the per-session conversation
source.

### Decision

- **Decisions keep the direction's paths.** `decisions` is a source kind,
  since a decision is evidence. Ledgers are single files directly under
  `sources/decisions/`, a third layout beside the per-source directories
  and the single-file SCRs. Citations use entry ids (`dec-<scope>-<n>`),
  so the path never has to move. The store is always called the decision
  *ledger*, leaving "decision log" to the conversation source.
- **Preferences move to `knowledge/domain/dom-preferences-<scope>.md`.**
  They follow a bundled `preferences` template (id pattern, one
  `## <matter>` section each, an attach policy). The intent of "one page
  per project plus global" is kept, and the folder name is not.

### Alternatives rejected

- **A `knowledge/preferences/` folder with a third page type.** It
  needs a new id prefix, `office.knowledge-write` path rules, index,
  search, attach, shared-library and back-link support. It buys nothing a
  template does not.
- **Ledgers in the per-source layout (`sources/decisions/YYYY/MM/<id>/`).**
  A ledger is long-lived, so either one directory holds years of entries
  under a misleading month, or a scope's decisions split across many
  sources and grep has to find them all.
- **Decisions as SCR lines only, as in v1.** That gives no per-scope
  store a precedent reader can load, and decisions made outside a scribed
  session would have nowhere to go.

### Consequences

- Library enumeration, indexing and citation resolution learn a third
  source layout and the `dec-` prefix.
- Preference pages share the 1,200-word cap. The weekly routine proposes
  consolidation before a page reaches it.

Cost: one more source layout to keep in every reader, and preference
paths that differ from the direction's wording.

## Scribe v2 tag definitions live in Library settings; app saves wait on ORB-20363

**Recorded:** 2026-11-01 · [ORB-21083]
**Code anchors:** `crates/office-core/src/domain/scribe_settings.rs`,
`crates/office-core/src/domain/settings.rs`; [specs/scribe.md](./specs/scribe.md#configuration)

### Context

Daniel wants to create, edit and delete tags in the Scribe tab. The
direction asks for an app-writable settings file, because the app has no
operator authority, and otherwise asks the design to state the dependency
on [ORB-20363]. The app calls Orbit as `local-cli` holding Agent. Every
Library settings write goes through `office.config-set`, which is
`mcp_scope: none` and refused to that authority. [ORB-20363], which wires
the app to Orbit's scoped application authority ([ORB-20362]), is blocked.

### Decision

Tag definitions and cadence are a `scribe:` block in the Library's
`.office/config.yaml`, written only by `office.config-set` and validated
by `office-core`. App saves therefore depend on [ORB-20363], whose
operation list must add `office.config-set`, limited to `scribe.*` keys.
Until it lands, the Tags editor is read-only, reports a save as
`not_permitted`, and shows the operator command.

Built in [ORB-21115]. The block stays out of the file while both parts
are default, so an unset `scribe.tags` means "the shipped defaults" and
follows later changes to them, while an explicit list (even `[]`) is the
user's own. A customized block is stored in the canonical JSON form that
`Settings::to_yaml` already writes, because the settings YAML subset has
no list of mappings. A definition that names a file target (a path-shaped
`sink`, or a `path`, `file` or `target` key) is refused with the typed
`tag_sink_refused`; a plain unknown sink name stays
`invalid_library_setting`. `office.status` and `office.scribe-pending`
return the effective block.

### Alternatives rejected

- **An app-owned settings file outside the Library.** Office tools could
  not read it (their grants are the Library and plugin state). The app
  would become a second writer of scribe policy with no audit.
- **The app writing the Library or plugin state directly.** That bypasses
  the writer lock and the rule that the app never touches `.orbit/` or
  Library files.
- **Making tag writes agent-callable, or an agent-callable office tool.**
  A tag's mode decides whether a preference applies without a proposal,
  so an agent able to change it could promote its own preferences.
- **Tags on `dom-scribe-rules` through proposals.** Acceptance is also
  blocked on [ORB-20363], so it does not escape the dependency, and every
  tag edit becomes a two-step approval.

### Consequences

- From the CLI, an operator can change tags at once. From the app, tags
  can be changed only after [ORB-20363].
- No agent can change scribe policy.

Cost: the Tags editor ships read-only and stays that way until a blocked
authority task lands.

## Retire `sources/note` outright, into the decision ledgers

**Recorded:** 2026-11-01 · [ORB-21083]
**Code anchors:** `crates/office-store/src/repository/library.rs` (the
`SourceKind::Note` capture fallback); `crates/office-core/src/domain/source.rs`
(`SourceKind`); `crates/office-core/src/domain/settings.rs`;
`crates/office-testkit/src/fakes/library.rs`

### Context

`office.capture` with plain text and no `kind` falls back to the `note`
kind. Five note sources exist, all Daniel's directions of 2026-11-01.
Daniel: "there are no users for this. so we can deprecate outright"
(`src-20261101-retire-sources-note-outright`). The workspace guide's
contract rule says to add and never rename, and the plan for v1 retirements
is a 30-day window.

### Decision

No compatibility window for `note`, per Daniel. Plain-text capture without
`kind` is refused with `source_kind_required`, naming the paste kinds and
the `decision` input. It is never appended to a ledger by default. `note`
leaves the domain, the settings kinds and the app's retention kinds. The
five notes move into the decision ledgers in one commit, and their old
`src-…` ids resolve through the ledger's `moved_sources`, so citations
hold.

### Alternatives rejected

- **Default plain text into the decisions ledger,** which the direction
  allowed. A pasted Slack thread is not a decision, and a ledger that
  precedent is read from must not fill with pastes.
- **A 30-day window like the other v1 surfaces.** No caller uses `note`,
  so a window would only carry dead code.

### Consequences

- A caller that relied on the fallback now gets a typed refusal naming
  the fix. This is a deliberate contract removal, which Daniel authorized.
- The constellation `AGENTS.md` rule that sends decisions to
  `office.capture` changes in a separate ws_constellation task.

Cost: a breaking change to `office.capture`'s accepted input, and alias
resolution for five ids kept indefinitely.

## Decision ledger build: migration in `office.init`, global by default, private notes kept

**Recorded:** 2026-11-02 · [ORB-21114]
**Code anchors:** `crates/office-core/src/domain/source/decisions.rs`;
`crates/office-store/src/repository/library/decisions.rs`;
`crates/office-store/src/driver/file/library.rs` (`migrate_note_sources`);
`crates/office-tools/src/tools/sources/capture/decision.rs`

### Context

The [`note` retirement](#retire-sourcesnote-outright-into-the-decision-ledgers)
asks for an operator-run, idempotent migration and a capture `decision`
input whose `ref` names a source anchor or a task comment. The spec does
not say where the migration runs, which ledger a note goes to, what a
private note becomes, or how a task comment is named.

### Decision

- The migration is a step of `office.init`, after the legacy-layout move,
  with the same contract: under the writer lock, one commit, and a re-run
  that moves nothing reports `already_initialized` with no commit.
- A note tagged `project:<name>` (or `scope:project:<name>`) goes to that
  project's ledger; any other note goes to `global`.
- A private note stays where it is: a ledger is always `normal`, and
  copying private text into one would publish it.
- A task comment ref is `orbit://task/<task-id>#comment-<n>`, read through
  Orbit's `task_show`; its entry's `source` is `orbit-task:<task-id>`.
- An entry record's body is its verbatim quote, so a migrated note's old
  `#L<n>` anchors count the same lines they did before.

### Alternatives rejected

- **A separate `office.migrate-notes` tool.** A new tool is a new
  contract for a one-time step; `office.init` already owns Library layout
  migrations and their idempotence.
- **Migrating private notes as reference-only entries.** The entry would
  lose the text the old citations anchor into.

### Consequences

- Operators run `office.init` once on the Library to migrate the existing
  notes; new `note` captures are refused from this build on.

## Scribe v2 S4: format flag, marker lists and local private parse

**Recorded:** 2026-11-02 · [ORB-21117]
**Code anchors:** `crates/office-core/src/domain/source/conversation/scribe/v2/`
(`markers.rs`, `scr.rs`, `pass.rs`);
`crates/office-store/src/repository/library/scribe_pass.rs`;
`crates/office-tools/src/tools/sessions/scribe_pending.rs`,
`scribe_append/v2.rs`, `scribe_query/v2.rs`, `scribe_ask.rs`

### Context

[Scribe v2](./specs/scribe.md#pipeline) reworks `office.scribe-pending`
and `office.scribe-append`, but the plugin that drives them switches over
only in S5, and v1 must keep running until then. The spec says
`scribe-append` refuses a finding "whose marker is not in the parse
result" without saying whether assistant markers are in it. It says a
private session's markers are "detected locally", while the store's
transcript read clears a private session's lines so that no preview can
disclose them.

### Decision

- **A `format` flag, not a replacement.** `scribe-pending` and
  `scribe-append` take `format: "v2"`; a session whose SCR is already v2
  stays v2, and `sections`/`findings` select v2 in append. Without them
  both tools behave as before. v1 and v2 keys in one call are refused.
- **Two marker lists.** `markers` holds the line-start markers of the
  user's typed turns; `agent_markers` holds the assistant's, recorded
  `by: agent`. Append accepts a finding whose (`msg`, `line`) is in
  either list, recomputed model-free from the parse over
  (`scribed_through`, `through`], and an agent marker is never applied.
- **Paste regions in the adapter.** The conversation adapter marks the
  lines inside a `<pasted_content>` region of a user record (`pasted` on
  the line, 1-based line numbers on the parsed message); marker detection
  skips them as it skips fences and blockquotes.
- **A local-only transcript read.** A new `Library::scribe_transcript`
  port keeps a private session's lines. `scribe-pending` v2 uses it only
  to write the private parse and detect markers; the response withholds
  the messages and returns markers by reference.
- **One `ScribePass` journal write (version 11)** carries the SCR, the
  ledger files and any preference page, compare-and-swapped under the
  writer lock, with the `previous` chain checked against the scope's
  latest SCR.
- **Front matter strings are JSON-quoted** (`title_by: "scribe"`,
  `previous: null`), as every other Library front matter is rendered.

### Alternatives rejected

- **Replacing v1 in place.** The plugin's v1 hooks would break until S5.
- **Refusing assistant markers outright.** The spec records them
  `by: agent`; refusing would lose the record.
- **Keeping lines in `session_transcript` for private sessions.** That
  read backs the transcript preview; clearing there is the disclosure
  guard.

### Consequences

- S5 passes `format: "v2"` and moves the plugin to markers, sections and
  findings.
- Inferred candidates (`capture: inferred`) are not built: append accepts
  marker-derived findings only. `on_by` stays with S5.
- The `preferences` query mode reports open proposals from the SCR
  entries that name one (`proposal:`), not from the proposal store.

## Decision ledger as a precedent provider: Office serves only what Orbit cannot

**Recorded:** 2026-11-01 · [ORB-21090]
**Code anchors:** none yet; [specs/decision-precedents.md](./specs/decision-precedents.md),
[specs/scribe.md](./specs/scribe.md#decision-ledger)

### Context

Daniel asked for a decision model that predicts a pending human decision
from recorded precedent ([ORB-20840]). Orbit designed the consumer and a
generic `orbit.precedent-provider/1` contract ([ORB-21089]; Orbit
`agent-main` at `266d234e`, `docs/design/decision-assistance/specs/precedent-decisions.md`).
Orbit has a built-in provider over its own answered decision requests and
merges providers by `citation.digest`. It refuses any precedent whose
`decided_by_kind` is not `human` or `human_confirmed`, and any precedent
decided at or after the request. The scribe v2 decision ledger (D13) was
shaped for this use but had no ingestion, provider or feedback rules.

### Decision

1. **Schema is extended, never renamed.** Four optional fields: option
   `id`/`consequence`, `orbit`, `same_as` and `confirms`. Only ingestion
   writes `orbit`, `confirms`, option ids and `decided_by: model`.
2. **Unrecoverable options stay precedent as single-option records**
   (`chosen` as the only option, with a fixed context note), so they meet
   Orbit's option check without inventing alternatives.
3. **Answered Orbit requests are ingested into the ledger**, idempotent by
   request id, with a deterministic `same_as` link to a scribe entry of the
   same decision (same task, within 30 minutes, matching chosen answer).
4. **The provider never returns an ingested decision or its `same_as`
   group.** Orbit's built-in provider serves those, and Office cannot
   produce Orbit's digest, so serving them would count one decision twice.
5. **Model answers are precedent only once a human confirms them;
   overrides are `reverses` entries and are precedent.** Review-gate
   `confirm`/`veto` entries are feedback marks, never returned.
6. **Conflicts reach the consumer as data, not a verdict:** both entries
   and a fixed context line, from the weekly routine's open to-dos.
7. **The agreement view is aggregate and read-only**, from Orbit's
   per-category readout, so it cannot contaminate a category in shadow.

### Alternatives rejected

- **Serve ingested Orbit decisions from Office too.** Double counting under
  Orbit's digest merge, and two citations for one label.
- **Merge or edit entries on dedupe.** The ledger is append-only; a link on
  the later entry keeps both pieces of evidence.
- **Return a suggested option or score from the provider.** The contract
  and the task forbid it; the answer is Orbit's alone.
- **Drop entries without options.** They are Daniel's real choices; the
  single-option record keeps them without pretending they were labelled.
- **Report conflicts with a new response field now.** That changes Orbit's
  contract from this repository. It is recorded as an open question for
  Orbit instead.

### Consequences

- Ingestion waits on an Orbit read of answered requests that does not
  exist yet ([ORB-21217] is blocked on it).
- Without a contract field for conflicts, Orbit's deterministic
  contradiction downgrade cannot fire on an Office-only conflict; the model
  sees it through the context line.
- In practice the Office provider serves scribe and captured decisions
  made in conversation, which Orbit cannot see.

## Scribe v2 S5: `scribe-control` stays `agent_callable`; a managed session cannot turn scribing on

**Recorded:** 2026-11-02 · [ORB-21118]
**Code anchors:** `crates/office-core/src/domain/scribe_control.rs`
(`Control`, `OnBy`, `Switch`),
`crates/office-tools/src/tools/sessions/scribe_control.rs` (`managed`),
`crates/office-store/src/scribe_control.rs` (`on_by`/`scope` columns),
`plugin/claude-code/scripts/scribe_v2.py`,
`plugin/claude-code/scripts/scribe.py` (`v2_hook`)

### Context

The [ORB-20756 entry](#recall-answers-and-scribe-control-authority-and-private-withholding)
recorded "No `agent_callable` for either tool" for `office.scribe-control`.
[ORB-20856] then made `scribe-control` (and `scribe-append`)
`agent_callable: true` on purpose: the Claude Code plugin calls Orbit as
`local-cli` holding `[agent]`, so without the grant every call from the
user's own interactive session was refused. The manifest and this ledger
disagreed, and the grant also let an Orbit-managed (headless) agent turn
scribing on, which scribe v2 forbids: a headless session is never scribed
([specs/scribe.md](./specs/scribe.md#turning-it-on)).

### Decision

- **Keep the grant; refuse the turn-on in the backend.** When the call
  envelope carries managed-task context (`context.task_id`),
  `office.scribe-control` refuses `set_switch` with `on: true` — the one
  op that turns scribing on or records `on_by` — with the stable code
  `scribe_managed_session`. Reads (`get`) and turning off stay allowed.
  The plugin additionally refuses a turn-on in an `sdk-cli` transcript
  (`scribe_headless_session`) before it calls Office.
- **The record gains `on_by` and `scope`.** A turn-on records `on_by`
  (`user` for `/scribe on`, the window and the pane; `agent` for the
  orchestrator's plugin-tool `on`, whose reply tells the agent to say so
  to the user) and the scope token it named (`global` or
  `project:<name>`). Pre-v2 callers that name nobody keep the recorded
  values; a first plain turn-on reads as the user's. `off` clears `on_by`
  and keeps `scope`. An existing database gains the two columns in place.
- **Preference proposals route after the append; tasks route before it.**
  Pipeline step 3 files through the user's CLI before the append so ids
  land on the SCR entry. That holds for scheduling, `orbit_task` and
  `owner:you` tasks. A preference proposal's page cites the SCR entry
  (`<SCR>#T<n>`) that only the append creates, and
  `office.proposal-create` resolves citations, so preference proposals
  are filed from the append's `needs_proposal` immediately after; the
  entry then reads `propose: <reason>` and the proposal id is in the pass
  report and the plugin's routing ledger (SCR id, message id, line).

### Alternatives rejected

- **Removing the grant again.** The user's own session would be refused
  on every control call; that is the failure [ORB-20856] fixed.
- **Trusting the plugin's headless check alone.** The record is the
  shared switch every host writes, so the refusal belongs in the backend
  next to the record rules, not in one provider's guess at its session.

### Consequences

- The `scribe-control` request/response schemas gain `on_by` and `scope`
  (additive; old callers omit them). The plugin-surface row now states
  the grant as the manifest declares it.

## Scribe v2 S9: no v1 backfill, v2 starts clean; the tidy-up is its own operator tool

**Recorded:** 2026-11-02 · [ORB-21122]
**Code anchors:** `crates/office-tools/src/tools/sessions/scribe_migrate.rs`,
`crates/office-core/src/domain/source/conversation/scribe/v2/markers.rs`
(`note_command`), `plugin/claude-code/scripts/scribe_v2.py`
(`catchup_v2`, `note_v2`)

### Context

D11 asked for a one-time backfill of v1 preferences and decisions as
proposals, and S9 first built it: one preference proposal per scope and
matter and one all-or-none `ledger_append` proposal per scope. Review found
the preference proposals self-blocking (each a full-page rewrite pinned to
one revision, so accepting one made the rest stale), and the real Library
held about 184 v1 preference matters, all landing for review at once.
Daniel, 2026-11-02: "no need for backfill btw. its going to be noisy. I
rather start clean".

### Decision

- **No backfill.** v1 `#preference`, `#decision` and `#agent-decision`
  lines and conversation-log `direction` and `decision` lines are not
  proposed into the preference pages or the decision ledgers. v2 starts
  with empty pages and ledgers; Daniel restates what still holds as
  markers. The v1 lines stay readable as evidence, and during the window
  the `pref` and `why` modes still answer from them after the v2 stores.
- **No `ledger_append` proposal kind.** It existed only for the backfill,
  so it is not added, nor the Library port's batch append.
- **A separate tool, `office.scribe-migrate`** (mutating, MCP scope none:
  an operator runs it, as `office.init`), not a step of `office.init`,
  for what remains: one `knowledge_update` removing the v1 Tags section
  and the `scribe:` mapping from `dom-scribe-rules`, and a report of the
  open v1 `#pending` lines (never filed). Filing a proposal is an Orbit
  write, and `office.init` must keep working with no Orbit. The proposal
  carries the `scribe-migration` tag and a hashed `rules` key tag; a
  re-run skips it in any status (truncated list refused).
- **`/scribe note` writes nothing.** The pass reads the command line itself
  as the marker, so a note follows the trust rule (only a user's typed
  turn counts) instead of a plugin-side write that would bypass it.
- **The window** runs 2026-11-02 to 2026-12-02; S10 ([ORB-21123]) runs on
  or after its end. `scribe.handoff` retires at once: capture drops the
  offer as `disabled`, and the page flag stays accepted so no page breaks.

### Alternatives rejected

- **Fix the backfill** (re-file stale preference proposals after each
  accept, or a section-level preference payload). It works, but still puts
  hundreds of old statements in front of Daniel for review, which he
  declined.
- **A step of `office.init`.** It would make init depend on Orbit and run
  on every init.

### Consequences

- Preferences and decisions recorded under v1 are not precedent for v2
  readers (the brief, `/scribe ask`, the precedent provider) unless
  restated.
- `scribe-query` v1 responses gain `deprecated`, `replacement` and
  `v2_results` (all additive). The root AGENTS.md change is [ORB-21239]
  in ws_constellation.

## Decision precedents P2: ingestion reads `include_closed`, keys on request and answer time, and writes one batch

**Recorded:** 2026-11-02 · [ORB-21217]
**Code anchors:** `crates/office-core/src/domain/source/decision_ingest.rs`
(`plan`), `crates/office-tools/src/tools/sessions/decision_ingest.rs`,
`crates/office-store/src/repository/library/decisions.rs`
(`write_decision_ledgers`, journal version 12)

### Context

The spec ([decision-precedents.md](./specs/decision-precedents.md#ingesting-orbit-decisions))
assumed ingestion needed a new Orbit read (`state: answered`,
`answered_since`). At build time Orbit's `orbit.decision.list` already
returns answered, superseded and expired requests with `include_closed`,
each answered one carrying its `resolution` (`answer_id`, `answered_by`,
`provenance`, `note`, `answered_at`). Orbit's model actor, its citation and
the `model_answer_review` gate are still proposed there (human-decision-channel
§11.2–11.3), so their shapes are not yet fixed.

### Decision

- **Read.** `orbit.decision.list` with `include_closed: true`, once for the
  Office workspace and once per `brief.task_workspaces` selector
  (`workspace`); `since` is applied by Office. `orbit.decision.show` fills
  a resolution only when a listed answered request omits it. The Office
  workspace failing refuses the run; a followed one failing is reported
  in `unavailable`.
- **Idempotence key: request id and answer time.** A request is
  `already_ingested` when an entry records the same request id and the
  same `decided_at`. Orbit's veto re-opens the original request, so a later
  human answer to the same id is a new answer: it is appended with
  `reverses` naming the newest earlier entry for that request, which is the
  spec's re-answer row. A request id alone would have skipped it.
- **Review gate binding.** A `model_answer_review` request names the model
  answer it reviews in `binding.request_id` (else a top-level `review_of`),
  as §11.3 says it is "bound to the original request id". With no such
  entry yet ingested it is skipped `review_target_missing` and picked up on
  a later run. Answers are processed oldest first, so a model answer and
  its review in one run link.
- **Scope.** A request's workspace is its category's leading
  `<workspace_id>/` (Orbit §11.1), else the followed selector's last
  `ws_` component. `ws_<name>` goes to `project:<name>` when `<name>` is a
  ledger slug; everything else, including the Office workspace's requests
  with no category, goes to `global`.
- **Quote.** The ledger requires `chosen` to be a verbatim span of
  `quote`. The resolution note is the quote when it contains the chosen
  label; otherwise the quote is the label, a newline, then the note. With
  no note it is the label.
- **Question.** The request's `question` when present, else its `summary`,
  else a fixed wording per gate.
- **One commit.** The planner computes every new line against the ledgers
  as read; the Library port `write_decision_ledgers` writes all changed
  ledgers compare-and-set (`decision_ledger_changed`, nothing written) and
  commits once through a version-12 `decision_ledgers` journal. An entry
  that fails ledger validation is reported `invalid` and skipped; the rest
  of the run still appends.
- **Routine.** `office-decision-ingest` is hourly at minute 20, seeded
  disabled, targeting a one-step job with `max_active_runs: 1`. Orbit
  routines carry no `dedupe` key (only auto-tasks do), so the spec's
  `dedupe: skip_if_open` is met by the job's single active run.

### Alternatives rejected

- **Wait for a dedicated answered-request read.** The existing read
  already returns what ingestion needs; a filter would only shorten the
  listing.
- **One `append_decision` call per entry.** That is one commit per entry,
  and a later entry (a confirm) must name an id assigned under the lock.

### Consequences

- When Orbit fixes the model citation and review binding shapes, only
  `decision_ingest.rs` changes if they differ from the names read here.
- Each run lists every closed request; `skipped` grows with Orbit's
  history until `since` is set.

## Task References

- [ORB-21117] — built Scribe v2 S4: the SCR v2 format, markers and the trust rule, and query and ask over the new stores.
- [ORB-21114] — built the decision ledger store, capture `decision` input and the `note` migration.
- [ORB-20946] — ships the disposable-data QA fixture (`make qa-exec`) for mutating office tools Orbit refuses to agents.
- [ORB-16585] — files the orchestrator assignment: rules, go/no-go, coordination, demo and submission for the office-work entry.
- [ORB-13117] — created the Rust CLI template this workspace was generated from.
- [ORB-13134] — hardened the template's gates and goldens.
- [ORB-18729] — built the source view, upload and paste capture, including citation anchor landing.
- [ORB-18796] — built the gateway plugin and found Orbit's verb rule.
- [ORB-18719] — built `office.search` and `office.attach`, and decided where attach stages its copies.
- [ORB-19747] — filed every meeting to-do as `owner:you` and left promotion to agent work to the orchestrator.
- [ORB-19748] — marked `unused_automation` as planned across the spec, glossary and orchestrator reference.
- [ORB-19421] — built the app's shared-library card in Needs you.
- [ORB-19751] — registered Muse Code with `--agent muse` in the installer.
- [ORB-19752] — withheld private content from MCP-visible read tools.
- [ORB-20096] — enabled scoped agent permissions for Office capture and proposals.
- [ORB-20228] — stopped the installer copying office skills the Orbit plugin already links.
- [ORB-20242] — moved conversation evidence to session-transcript extraction on a cheap crew.
- [ORB-20243] — partitioned Library sources by kind.
- [ORB-20246] — kept local calendar events as workspace writes.
- [ORB-20247] — partitioned Library proposals by status.
- [ORB-20245] — made the app report a missing approval authority plainly instead of granting itself Operator.
- [ORB-20362] — landed Orbit's scoped, audited application authority for user-approved tool calls.
- [ORB-20363] — wires the office app's approval actions to that scoped application authority.
- [ORB-20685] — built the live scribe's incremental capture and `Note:` commands.
- [ORB-20756] — built recall answers, recall feed mode and Office-owned scribe control.
- [ORB-20690] — orchestrated the scribe and recall push; recorded scribe v1.
- [ORB-20780] — built `office.scribe-pending` and `office.scribe-append`.
- [ORB-20825] — moved local calendar events into the Library's ignored state, inside the plugin's write grant.
- [ORB-20849] — made long, tool-heavy sessions scribable by stripping tool payloads at staging.
- [ORB-20696] — built the web Scribe screen over scribe v1.
- [ORB-20884] — bound the private-route token to the endpoint origin it was saved for.
- [ORB-20840] — the orbit-office on-call session where Daniel gave the scribe v2 direction.
- [ORB-21063] — built global handoffs and `/handoff <project>`; reworked by scribe v2.
- [ORB-21065] — built structured handoffs; their fields map onto the v2 SCR sections.
- [ORB-21075] — built the Scribe › Handoffs view; it becomes the v2 one-card feed.
- [ORB-21079] — gave scribe sessions a first-pass title; v2 derives it from the SCR summary.
- [ORB-21083] — designed scribe v2: opt-in sessions, marker-only tags, ledgers, SCR as handoff, `note` retired.
- [ORB-21089] — plans Orbit's decision model, which reads decision precedent.
- [ORB-21090] — designed the office precedent provider over the v2 decision ledger.
- [ORB-21115] — built the `scribe` Library settings block, its `office.config-set` keys and `tag_sink_refused`.
- [ORB-21217] — built ingestion of answered Orbit decision requests into the ledger, and the decisions above.
- [ORB-20856] — made `office.scribe-control` and `office.scribe-append` agent-callable so the plugin's local-cli `[agent]` calls succeed.
- [ORB-21118] — built Scribe v2 S5: the Claude Code opt-in, Stop-hook cadence and marker routing, and the managed-session refusal above.
- [ORB-21122] — built Scribe v2 S9: the v1 command mapping and `office.scribe-migrate` above, with no v1 backfill.
- [ORB-21123] — removes the v1 scribe surfaces after the window.
- [ORB-21239] — replaces the root AGENTS.md decision-log rule in ws_constellation.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.


## Decision precedents P3: bind v1 scope and rank a private in-memory snapshot

The built Orbit v1 validator (`application/task/precedent_provider/model.rs`
and `validation.rs`, Orbit contract commit `b11244a4719cd0d33487146fd745e9eb98f5e35d`)
requires `scope.workspace_id` and `scope.category` to be nonempty strings;
workspace must equal the query's workspace. It also caps response rows at
`k`, strings at 8192 bytes, options at eight and the encoded response at
262144 bytes. This overrides the earlier Office design's nullable global
scope fields. Office binds every row to the querying workspace and uses
the request category when the ledger has none, without modifying the ledger.
The category bonus still uses only an actual `orbit.category` match.
Library settings have no owner-name field, so `decided_by` uses `user`.

The existing Library search index ranks whole documents and does not expose
per-entry scores. The provider ranks only its eligible entry snapshot using
the same FTS5 tokenizer/BM25 in an in-memory index behind the Library port.
Each positive BM25 magnitude is divided by the best magnitude, giving
0..1 lexical relevance. Quoted prefix terms are ORed for lexical overlap;
private, future and other excluded text never enters that index.
The binary uses the existing file-only read-open path so even absent plugin
state stays absent. Malformed/unreadable ledgers are isolated and named.

Reversal evidence consumes the same `k` budget: a pair displaces the lowest
remaining unpaired candidate. If a chain cannot fit, newer reversal evidence
wins and truncation is explicit. Relationships at or after `as_of` have no
effect; an excluded reversal never leaks its text, while its old entry stays
flagged and an omission records the gap. Unresolved-conflict reporting is P4.

The P3 brief explicitly assigns tool registration, manifest, schemas and
ARCHITECTURE changes to this task; the necessary shared-list edits are part
of that integration. No dependency or crate edge changes. Companion port,
composition, file-reader and testkit edits make the real binary implement
the contract and keep the read-only boundary intact.

For `STD-01@2 §R11` and `§R34`, this host contract follows Orbit's exact
wire shape: absent optional scope topic is omitted and no `total` field is
added. The normal ranked top-k shortlist is the requested retrieval result,
so it does not set `truncated`; byte cuts, omission overflow and reversal
pairs that cannot fit do. This preserves the consumer's distinction between
a shortlist and an incomplete evidence packet.
