---
title: office — Decisions
owner: daniel
last_updated: 2026-10-27
last_validated: 2026-10-26
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

## Task References

- [ORB-16585] — files the orchestrator assignment: rules, go/no-go, coordination, demo and submission for the office-work entry.
- [ORB-13117] — created the Rust CLI template this workspace was generated from.
- [ORB-13134] — hardened the template's gates and goldens.
- [ORB-18729] — builds the source view, upload and paste capture, including citation anchor landing.
- [ORB-18796] — built the gateway plugin and found Orbit's verb rule.
- [ORB-18719] — built `office.search` and `office.attach`, and decided where attach stages its copies.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.


### Daily digest definitions ownership

The daily digest adds only its job and routine entries beside the existing
`spec.definitions` registrations, in addition to enabling its reserved tool
block. This is the operator-authorized narrow exception to the
ARCHITECTURE.md integrator ownership rule for this feature. The one-step job
inlines the deterministic `plugin.tool_call` activity, avoiding another shared
activity registration. The routine uses the accepted routine schema without
`required_tools`; `config.tool: office.digest` is its explicit tool reference.


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
**Code anchors:** `install.sh`; [specs/install.md](./specs/install.md#office-init-path)

The installer owns machine provisioning: toolchains, plugin installation,
secrets, agent integration and the app service. A small installed `office`
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
