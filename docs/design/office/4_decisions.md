---
title: office — Decisions
owner: daniel
last_updated: 2026-10-26
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

The approval and write tools (`init`, `knowledge.write`,
`proposal.accept`, `artifact.promote`, `selection.put`, `config.set`,
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
`office.config.set` (validated, then committed). Secrets stay in Orbit's
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
  The anchor landing (opening the source scrolled to the cited span) is
  planned under ORB-18729: until the planned source screen ships, the
  anchors are a recorded URL contract with no landing behaviour. The
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
This records the URL-format contract; the landing behaviour (the source
view opening at that anchor and scrolling to the span) is planned under
ORB-18729 and ships with the source screen, which is currently planned.
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
- **`office.session.brief`**, computed at session start from Orbit and the
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
  and `office.knowledge.write` checks it against that template.
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

## Routines run approved scripts through `office.script.run`, and their outputs wait for review

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
- **`office.script.run`** (mutating, `mcp_scope: none`) runs the pinned,
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

## Template decisions not carried over

The template's note-store decisions (persistence module, one module per noun, audit guard, `--json` alias, list envelope, store lock, store format registry, owner-only store state, malformed-note handling, environment fallbacks) were removed with the note domain. They return, rewritten for the office domain, only when the feature they govern lands.

## Task References

- [ORB-16585] — files the orchestrator assignment: rules, go/no-go, coordination, demo and submission for the office-work entry.
- [ORB-13117] — created the Rust CLI template this workspace was generated from.
- [ORB-13134] — hardened the template's gates and goldens.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
