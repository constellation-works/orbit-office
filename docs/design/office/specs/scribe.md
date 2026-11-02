---
type: design
summary: "Spec: scribe v2 — opt-in scribing per session, inline markers honoured only in the user's own typed turns, preferences and decisions as the only default tags (preference pages changed by proposal, an append-only decision ledger fit to serve as precedent), and one SCR per session whose latest copy per project and global is the handoff"
last_validated: 2026-11-02
---

# Spec: Scribe v2

Status: designed ([ORB-21083]). Slices S1–S9 are built: the
[decision ledger](#decision-ledger) store, `office.capture`'s `decision`
input and the [`note` retirement](#retiring-sourcesnote) ([ORB-21114]);
the `scribe:` settings ([ORB-21115]); preference pages and the
contradiction check ([ORB-21116]); the v2 SCR, markers and the trust
rule in `office.scribe-pending` and `office.scribe-append`, with
`office.scribe-query` and `office.scribe-ask` over the new stores
([ORB-21117]; see [Built tools](#built-tools-s4)); the Claude Code
plugin's opt-in, Stop-hook cadence and marker routing ([ORB-21118];
see [Built plugin](#built-plugin-s5)); the brief and `/handoff`
over the latest SCR ([ORB-21119]; see
[sessions.md](./sessions.md#scribe-v2-the-latest-scr-is-the-handoff));
the web Scribe tab: the one-card feed over `view: list`, the Preferences
and Decisions panels and the Tags editor ([ORB-21120]; see
[Feed](#feed) and [Configuration](#configuration)); the weekly
contradiction auto-task ([ORB-21121]); and S9, the migration
([ORB-21122]; see [Migration](#migration)): the compatibility window
opened on 2026-11-02 and closes on 2026-12-02. Only the removal slice
(S10) is not built.
Scribe v1 is built and still runs
until the slices in [Build order](#build-order) land. The v1 text
this spec replaces is at `git show d398471:docs/design/office/specs/scribe.md`,
and its decisions stay in [4_decisions.md](../4_decisions.md#scribe-v1-commanded-capture-into-one-tagged-file-per-session).
The v2 decisions are
[Scribe v2: opt-in sessions, marker-only tags, ledgers and the SCR as handoff](../4_decisions.md#scribe-v2-opt-in-sessions-marker-only-tags-ledgers-and-the-scr-as-handoff)
and the three entries after it.

Every rule below traces to one of two Library notes holding Daniel's words
verbatim: `src-20261101-scribe-v2-redesign-direction` (direction points
1–11 and 13, cited here as **D1**–**D11** and **D13**) and
`src-20261101-retire-sources-note-outright` (**D12**: "there are no users
for this. so we can deprecate outright"). Both are captured from the
orbit-office on-call session ([ORB-20840]). After the
[note migration](#retiring-sourcesnote) they resolve as decision ledger
entries under the same ids.

## Why

The v1 feed and its `why` view are noisy. Every session is scribed, most
`why` entries have no rationale, and the feed lists dozens of untitled
sessions. Daniel's direction is to scribe less and keep what is kept
worth reading: scribe a session only when asked, record only what the
user marks, keep preferences and decisions in stores built for them, and
let one rolling session record per session double as the handoff.

## Goals and non-goals

v2 does:

- Scribe a session only after `/scribe on` (D1).
- Record tag entries only from inline markers, honoured only in the
  user's own typed turns (D2).
- Ship two default tags, `preferences` and `decisions` (D3), with one
  store each (D4) and contradiction checks (D5).
- Keep one SCR per scribed session whose upper sections are rewritten each
  pass and whose tag entries are append-only (D6). The latest SCR per
  project, and the latest global SCR, is the handoff (D7). The feed shows
  one card per project and one global card (D8).
- Let the user manage tags from the Scribe tab (D9), and turn a scheduling
  marker into a proposed Orbit task (D10).
- Migrate v1 within its 30-day compatibility window (D11), retire
  `sources/note` outright (D12), and write decision entries that can serve
  as precedent ([ORB-21089], [ORB-21090]; D13).

v2 does not:

- Dispatch work. Every Orbit task the scribe files is proposed or is an
  `owner:you` to-do, which no dispatcher picks up (D10).
- Track agent work, landed changes or to-dos itself. Those are Orbit
  tasks; the SCR's Next section cites them (D3).
- Run a long-lived timer. Passes start from the Stop hook (D1).
- Write polaris or almanac, from any tag ([context boundary](#lesson-and-the-store-boundary)).

## Turning it on

A session is scribed only when it is turned on (D1). The default for every
new session is off, and the v1 provider-plugin setting that could default
it on is retired.

| Command (Claude Code) | Does |
|---|---|
| `/scribe on [@<project>\|@global]` | Turn scribing on for this session. The optional scope overrides the [session scope](#session-scope). The first pass runs at once and creates the session's SCR. |
| `/scribe off` | Stop new passes. Nothing already written is removed. |
| `/scribe update` | Run a pass now, ignoring the cadence. It works only while the session is on. |
| `/scribe status` | Show on or off, the scope, the SCR id, `scribed_through` and when the next pass is due. |
| `/scribe title <text>` | Set a user title (`title_by: user`); it wins over the summary-derived title. |
| `/scribe ask <question>` | Answer from the ledgers, preference pages and SCRs with citations ([Answering](#answering)). |
| `/scribe tags` | List the tag definitions in force ([Configuration](#configuration)). |

The switch is the existing Office-owned record behind
`office.scribe-control` ([sessions.md](./sessions.md#switch)). It gains
`on_by` (`user` or `agent`) and the session's scope. `/scribe now` maps
to `/scribe update`, and the v1 `on|off|status|title` keep their meaning
([Migration](#migration)).

**Cadence.** While a session is on, the provider's end-of-turn hook
(`Stop` in Claude Code) checks the time and the user turns since the last
pass. When either reaches its threshold, the hook starts one pass: by
default every 20 minutes or every 10 user turns, whichever comes first
(`scribe.cadence.minutes` and `scribe.cadence.turns`,
[Configuration](#configuration)). There is no timer. A session with no new
turns gets no pass, and a closed terminal gets none until it resumes. The
hook never waits for inference. It starts one headless scribe run for the
session, guarded by a per-session lock so at most one runs, and returns.
That run's own transcript is headless (`entrypoint: sdk-cli`) and is never
scribed. SessionEnd runs one final pass for a session that is on.

**Orchestrators.** An orchestrator agent may turn scribing on for the
interactive session it runs in, through the plugin's `scribe` tool with
`on`. The record then carries `on_by: agent`, and the tool's reply tells
the agent to say so to the user in one line. The user's `/scribe off`
wins. An agent may also only ask ("turn on the scribe for this
session?"). In a headless Orbit-managed session `office.scribe-control` is
refused ([plugin-surface.md](./plugin-surface.md#agent-authority-inventory)),
so such a session cannot be scribed. Its record is its assignment task, as
the context-boundary rule says.

### Session scope

Each SCR is scoped to one project or to global (D6, D7).

- **Project** by default: the session's project is the git repository name
  that v1's parse already resolves ([Pipeline](#pipeline)).
- **Global** when the working directory holds two or more repositories
  among its children and grandchildren (the constellation root). This is
  the same test the brief's `auto` mode uses
  ([sessions.md](./sessions.md#choosing-the-project)). A session is also
  global when the user writes `/scribe on @global`.
- `/scribe on @<project>` names a project explicitly. The scope is fixed
  once the SCR exists. A later `/scribe on @…` with a different scope
  closes this SCR and starts a new one in the new scope, with `previous:`
  set in that scope.

## Markers

The user marks what the scribe records by writing a marker in the chat
(D2). The next pass honours it. Markers replace v1's mid-call
`/scribe note`.

| Marker | Records |
|---|---|
| `#preferences <text>` (also `#preference`) | A preference finding: a proposal, or a direct page change in apply mode ([Preferences](#preferences)). |
| `#decision <text>` (also `#decisions`) | A decision ledger entry ([Decision ledger](#decision-ledger)). |
| `#scribe <tag> <text>` | An entry for any defined tag, default or custom. |
| `#scribe <text>` (no tag name first) | An instruction to the scribe. Today the only one it acts on is a [scheduling request](#scheduling-markers-and-to-dos); anything else is reported back as not understood. |

- A marker may name a scope right after itself: `@global` or
  `@<project>`, as in `#preferences @global always draft, never send`.
  Without one, preferences default to global and decisions to the
  session's scope (D3).
- A decision's rationale is an optional `because:` in the same turn, as in
  `#decision ship v2 first because: the feed is unusable`. The reason must
  have been stated; the scribe never adds one (D3).
- A marker starts a line. A marker in the middle of a sentence, in a
  fenced code block or in a blockquote is not one.

### Trust rule: whose markers count

A marker counts only in the user's own typed turns (D2). This is the
prompt-injection guard, and it is checked without a model.

1. **Detection is model-free.** `office.scribe-pending` finds markers in
   the parsed messages and returns them as `markers`: message id, line,
   marker, tag, scope and text. `office.scribe-append` refuses any
   marker-derived entry that is not in that list
   (`marker_not_found`). The scribe agent cannot invent a marker or move
   one onto another message.
2. **Only typed user text is eligible.** The adapter already keeps only
   user and assistant text ([sessions.md](./sessions.md#adapter-and-provenance)):
   tool calls, tool results, thinking, sidechains, compaction summaries,
   `<system-reminder>`, `<task-notification>` and `<local-command-*>` text
   never reach the parse, and a desktop side session's opening brief is an
   assistant message. Beyond that, a marker is ignored when it sits inside
   pasted content the adapter can see. That covers a provider's paste
   record or placeholder expansion, a `<pasted_content>` wrapper, a fenced
   block or a blockquote. A marker is also ignored in text the user's
   message quotes from a file or a tool, which never reaches the parse
   anyway.
3. **Unmarked pastes are a known gap.** A paste the provider leaves
   unmarked looks like typed text. Two limits contain it. Apply mode is
   off by default, and a marker in a user turn longer than 40 lines or
   4,000 characters is treated as propose mode whatever the tag's mode, so
   no paste-sized turn can change a preference page by itself.
4. **Agent markers are recorded, never authoritative.** A marker in
   assistant text, or in an orchestrator's own turn, is recorded with
   `by: agent`. An agent `#decision` becomes a ledger entry with
   `decided_by: agent`. An agent `#preferences` can become at most a
   proposal marked `by: agent`, never a direct change in any mode. No
   agent marker can put a preference into effect (D2). An agent marker
   cannot schedule a task either: it is reported, not filed.
5. **Headless and private sessions.** Headless runs are never scribed. In
   a private session markers are detected locally and written
   [by reference only](#privacy).

## Tags

### Defaults

Exactly two tags ship by default (D3).

| Tag | Who | Scope | Sink | Mode |
|---|---|---|---|---|
| `preferences` | the user (agent markers only propose) | global, or a project when the marker names one | the scope's [preference page](#preferences) | `propose` (default) or `apply` |
| `decisions` | the user and agents | the session's scope, or global when marked | the scope's [decision ledger](#decision-ledger) | append (evidence needs no approval) |

### Removed tags

| v1 tag | v2 |
|---|---|
| `why` (and `#no-why`) | Removed: most entries had no rationale (D3). A decision's rationale is its optional `because`. |
| `pending`, `resolved` | Removed. A user-owned action item becomes an Orbit `owner:you` to-do. Agent-owned items are Orbit tasks the SCR's Next section cites. |
| `todo`, `reminder` | Removed as tags. A to-do is an `owner:you` Orbit to-do; a timed request is a [scheduling marker](#scheduling-markers-and-to-dos). |
| `landed` | Removed. Landed work is an Orbit task's `done` transition, which the brief already reports. |
| `decision`, `direction`, `agent-decision` | Folded into `decisions` (`decided_by` tells human from agent). Directions that set a standing rule are `preferences`. |
| `preference` | Renamed `preferences`, with a page store instead of SCR lines. |
| `fact`, `question`, `incident` | Removed. A user can define any of them again as a custom tag. |

### Custom tags

The user may define more tags, and may edit or delete the defaults, in
the Scribe tab or the settings file ([Configuration](#configuration)). A
tag definition has:

- `name`: lowercase kebab-case, unique;
- `scope`: `global`, `project` or `session` (the session's scope);
- `instruction`: what the scribe should record for this tag. It is passed
  to the scribe agent as data and cannot widen authority, as the v1 rules
  page could not;
- `sink`: one of `decisions_ledger`, `preference_page`, `scr_only` or
  `orbit_task`;
- `mode`: `propose`, `apply` or `append`, paired with the sink.
  `preference_page` takes `propose` or `apply`; `orbit_task` takes
  `propose` and always files a proposed task; `decisions_ledger` and
  `scr_only` take `append`. Any other pair is refused;
- `workspace`: for `orbit_task` only, the Orbit workspace selector the
  proposed task is filed in (default: the office workspace);
- `capture`: `markers` (default) or `markers+inferred`. With inferred
  capture the scribe may also record unmarked candidates. Those are always
  `by: scribe`, always propose mode, and listed apart in the pass report.

A deleted default stops being recognized: its markers are reported as an
unknown tag and kept in the SCR's pass report only. **Restore defaults**
brings them back.

### `lesson` and the store boundary

`lesson` is not a default (D3). The constellation's context-boundary rule
(polaris `projects/context-boundary/README.md`, "Which store? Ask in
order", outside this repository) puts "what any unattended worker must know or follow" in polaris (rule 2,
before the Library's rule 3), and says "the Library never writes polaris
or almanac, and they never write it". The design keeps that line:

- No sink writes a polaris or almanac file. The sink list has no file
  target outside the Library, and settings validation refuses a tag
  definition naming one (`tag_sink_refused`).
- A user who wants `lesson` defines it with `sink: orbit_task` and
  `workspace:` set to the polaris Orbit workspace. Each finding files a
  **proposed** Orbit task there, carrying the verbatim quote and its SCR
  entry id. The plugin files it through the user's own `orbit` CLI, not
  through Office. That is an Orbit record (rule 1: a unit of work), not a
  Library write. A polaris change then lands under polaris's own rules.
- The SCR records the entry with the task id. The Library keeps no copy
  of the lesson, so nothing is duplicated across the boundary.
- If the workspace cannot be reached, the entry stays in the SCR as
  `sink: failed` and the pass report names it. It is never written
  anywhere else instead.

## Tag stores

Each default tag has one store (D4). Both paths below fit the Library's
sources/knowledge split ([library.md](./library.md#layout)): a decision
is evidence of what was said, so it is a source; a preference is a
standing understanding the user approves, so it is a knowledge page
changed by proposal. The fit is recorded in
[Scribe v2 store paths](../4_decisions.md#scribe-v2-store-paths-a-decisions-source-kind-and-templated-preference-pages).

### Decision ledger

**Paths:** `sources/decisions/<project>.md` and
`sources/decisions/global.md` (D4). `decisions` is a new source kind that
replaces `note` ([Retiring `sources/note`](#retiring-sourcesnote)).

- **Layout.** A ledger is one file per scope, directly under its kind
  directory, not `sources/<kind>/YYYY/MM/<source-id>/source.md`. A ledger
  spans years, and the Library cites ids, not paths, so a fixed path loses
  nothing. SCR files are the precedent for a single-file source. The
  Library's enumeration, index and citation resolution learn this layout
  alongside the two existing ones.
- **Ids.** The ledger's id is `decisions-<scope>` (`decisions-global`,
  `decisions-orbit-office`). Each entry has a permanent id
  `dec-<scope>-<n>`, numbered from 1 in that ledger. Citations use the
  entry id (`dec-orbit-office-7`); the `#L<n>` of its line is also valid,
  since lines never move. `office.knowledge-write`'s citation check
  accepts the `dec-` prefix.
- **Append-only.** One commit per append, never edited or removed, like
  conversation sources. A correction or a change of mind is a new entry
  with `reverses:`.
- **Front matter:** `id`, `kind: decisions`, `title` (`Decisions
  (<scope>)`), `scope`, `created_at`, `sensitivity` (always `normal`; see
  [Privacy](#privacy)) and `moved_sources` (the note ids migrated in,
  mapped to their entry ids). A ledger whose front matter, ids or entry
  sequence disagree is refused (`decision_ledger_invalid`), never skipped.
- **Reads.** `office.source-show` of a ledger id returns its body and
  every entry under `decisions`; of an entry id, the entry's quote as the
  body and that one entry. Each entry carries its derived `reversed_by`,
  its `ledger` id and its `line`. A bare entry id is a valid citation (the
  entry's quote is its body, so `dec-<scope>-<n>#L<n>` anchors into the
  quote); `decisions-<scope>#L<n>` anchors into the ledger file's body.
- **Not the conversation decision log.** The per-session conversation
  source that [sessions.md](./sessions.md#decision-log) calls the
  "decision log" stays under `sources/conversation/` and keeps its name.
  This store is always called the decision *ledger*, at a different path.

**Entry schema** (D13). One entry per line, so grep finds it and
`#L<n>` holds, written as the entry id, the time, `#decision` and one
compact JSON object. This is the same one-line form as the structured
handoff ([sessions.md](./sessions.md#structured-handoff-contract)).
[ORB-21090] reads exactly these fields: [decision-precedents.md](./decision-precedents.md#entry-schema)
says how each serves as precedent and adds four optional, ingestion-only
or dedupe fields (option `id`/`consequence`, `orbit`, `same_as`,
`confirms`) without renaming any field below.

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | `dec-<scope>-<n>`, the entry's permanent id. |
| `question` | yes | What was being decided. When a message asked it, the verbatim question with `question_ref` (its message id). Otherwise a one-line restatement by the scribe, with `question_by: scribe`. |
| `options` | yes | The options considered, each `{text, ref}` verbatim from a message, or the string `"unrecoverable"` when the turns do not state them. Never invented. Ingested options also carry `id` and `consequence` from the Orbit request; `office.capture` and `office.scribe-append` refuse option `id`/`consequence`. |
| `chosen` | yes | The chosen option: a verbatim span of the decision message. |
| `quote` | yes | The decision message's verbatim text (the marker line), and |
| `ref` | yes | its transcript line ref: SCR id, the SCR tag entry id and the message id (`SCR-…#T12 m-0187`). |
| `because` | no | The stated rationale, verbatim with its `ref`, only when one was stated (D3). |
| `scope` | yes | `global` or `project:<name>`. |
| `context` | yes | `{task_type, workspace, task, risk_tier}`, each null when unknown. They come from the session's Orbit assignment (`ORBIT_ACTIVE_TASK_ID`, else the task the turns cite), never guessed. |
| `decided_by` | yes | `human`, `agent` or `model`. `human` only for a user's marker in a typed turn. `agent` for an agent's marker. `model` is for a decision model's answer, written only by Orbit decision ingestion's feedback path ([ORB-21089], [ORB-21090]; [feedback rules](./decision-precedents.md#feedback)); the scribe never writes it, and it is never precedent until a human confirms it. |
| `decided_at` | yes | The decision message's time, RFC 3339 with offset. |
| `reverses` | no | The entry id this one overturns. |
| `orbit` | no | Ingestion only: `{request_id, gate, category, topic, answered_by, channel}` from the answered Orbit request, plus `model` (assessment id, policy digest, model identity, probability, cited precedents) when the model answered. `office.capture` and `office.scribe-append` refuse it. |
| `same_as` | no | The entry id recording the same decision from the other source. Accepted by `office.capture` and `office.scribe-append`, checked like `reverses` (`decision_not_found`); also set automatically when an append matches exactly one ingested entry (same task, `decided_at` within 30 minutes, chosen matches the chosen answer). An ambiguous match links nothing and never blocks the append. |
| `confirms` | no | Ingestion only: the `decided_by: model` entry this human entry confirms. `office.capture` and `office.scribe-append` refuse it. |
| `reversed_by` | derived | Never written, since entries are append-only. Readers compute it from later entries' `reverses`, and `office.scribe-query` returns it. |
| `source` | yes | The evidence's source id: the SCR id; for a migrated note, its original `src-…` id; for an answered Orbit decision request that [ORB-21090] ingests, `orbit-decision:<id>`. |
| `private` | no | `true` for a reference-only entry ([Privacy](#privacy)). |

```markdown
- dec-orbit-office-7 2026-11-01T22:39:00-07:00 #decision {"id":"dec-orbit-office-7","question":"Should sources/note get a compatibility window?","question_by":"scribe","options":"unrecoverable","chosen":"deprecate outright","quote":"#decision there are no users for this. so we can deprecate outright","ref":"SCR-20261101-2231-0a3104#T4 m-0187","scope":"project:orbit-office","context":{"task_type":"feature","workspace":"ws_orbit-office","task":"ORB-<id>","risk_tier":null},"decided_by":"human","decided_at":"2026-11-01T22:39:00-07:00","reverses":null,"source":"SCR-20261101-2231-0a3104"}
```

When the user marks `#decision`, the scribe agent reads the surrounding
turns (the marker's message and up to ten before it) for the question and
the options (D13). `office.scribe-append` checks every verbatim field
(`quote`, `chosen`, each option, `question` with a ref, `because`) against
the cited message, as v1 checks quotes. It refuses a mismatch
(`quote_mismatch`), and a `decided_by` that disagrees with the marker's
author (`decided_by_mismatch`).

**Outside a scribed session**, an agent or the user appends a decision
with `office.capture` and a `decision` input carrying the same fields
without `id` (assigned on append) or `reversed_by` (derived). `ref` then
names a source anchor (`<source-id>#<anchor>`) or a task comment
(`orbit://task/<task-id>#comment-<n>`, read through Orbit); `source` is
set from it (`orbit-task:<task-id>` for a comment), and a different
`source` is refused. Every verbatim field is checked against its ref
(`quote_mismatch`), an unreadable ref is refused
(`decision_ref_unresolved`), a ref to a private source is refused unless
the entry is `private` (`decision_source_private`), `decided_by: model`
is refused (`decided_by_mismatch`), and `reverses` naming no existing
entry is refused (`decision_not_found`). `kind: decisions` with text is
refused (`decision_input_required`). The append takes the writer lock
and commits once. This replaces
v1's "append to the decision log" rule ([Retiring
`sources/note`](#retiring-sourcesnote)).

### Preferences

**Pages:** one preference page per scope, `dom-preferences-global` and
`dom-preferences-<project>`, stored as `knowledge/domain/dom-preferences-global.md`
and `knowledge/domain/dom-preferences-<project>.md`. The direction named
`knowledge/preferences/<project>.md` and `knowledge/preferences/global.md`
(D4). The Library model has only `knowledge/domain/` and
`knowledge/runbooks/`, with the id deciding the path
([library.md](./library.md#knowledge-pages)). So v2 keeps the direction's
intent (one page per project plus a global page) at the path the model
already supports. It gives them a bundled one-per-scope template,
`preferences` ([knowledge-templates.md](./knowledge-templates.md#bundled-templates)):

- id pattern `dom-preferences-<scope>`;
- one `## <matter>` section per class of question (the v1 `matter` slug).
  Each section states the current preference in one or two sentences,
  cites its evidence (SCR tag entry ids and any ledger ids), and records
  `stated_at` and `review_by` (default 90 days, as in v1);
- attach policy: included whenever a task in that project, or any task
  for the global page, is attached.

**Changed only by accepted proposals or apply mode** (D3, D4):

- **Propose (default).** Each preference finding becomes one
  `office.proposal-create` proposal: `knowledge_add` for a scope's
  first page, else `knowledge_update` pinned to the page's `sha256`. The
  user accepts or dismisses it. Dismissal memory applies as for any
  proposal.
- **Apply.** With the tag's `mode: apply`, a user's marker in a typed turn
  under the paste-size limit writes the page directly in the pass's
  Library commit, through the same page validation as
  `office.knowledge-write`. A finding that hits a contradiction, an agent
  marker or an inferred candidate always falls back to propose.
- **Override.** A new preference on an existing matter replaces that
  section's statement. The page's history (`office.knowledge-show
  history`) and the cited SCR entries keep the old one.
- **Size.** Pages keep the 1,200-word body cap. When a page passes 1,000
  words, the [weekly routine](#contradiction-checks) proposes a
  consolidation.

## Contradiction checks

Contradictions are checked twice (D5).

- **At proposal time.** Before the scribe files a preference proposal, it
  checks the finding against the scope's existing page. A model-free check
  flags a section with the same `matter` and a different statement. The
  scribe agent also reports semantic conflicts with other matters on the
  page. `office.proposal-create` renders both as a `## Conflicts` section
  on the proposal's evidence page, quoting the current statement and its
  citation, so accepting is a deliberate override. A conflicting finding
  never takes apply mode.
- **Weekly.** The `office-scribe-contradictions` auto-task definition
  (weekly, `dedupe: skip_if_open`, `complexity: low`, no crew pinned) ships
  **disabled**, registered under `spec.definitions.auto_tasks` in
  `plugin.yaml` like every office routine. It reads every decision ledger
  and preference page. For a preference that conflicts with another
  preference, or with a later decision, it files one proposal to settle
  the page. For two decisions that conflict with no `reverses` link, it
  files one `owner:you` to-do asking which stands. The user's answer is
  recorded as a new decision with `reverses`. One proposal or to-do per
  conflict, each citing both entries. It never edits a ledger.
  It also compares decisions within an Orbit category (an ingested entry's
  `orbit.category`, or a scribe entry's `same_as` partner's category), even
  when the question text differs, and ends each decision-conflict to-do
  with a machine-readable `Conflicting entries:` line. The precedent
  provider reports a conflict whose to-do is still open
  ([ORB-21219]; [decision-precedents.md](./decision-precedents.md#contradictions)).
- **When a meeting is extracted.** See [Meeting decisions](#meeting-decisions).

### Meeting decisions

Built in [ORB-21340]. `office.extract` with `profile: meeting` feeds the
ledger too, not only conversation scribing. After it saves
`extract-result.json`, and before it files to-dos:

1. **Append.** Each cited decision becomes one `decisions-global` entry
   through `office.capture`'s `decision` path, so every verbatim check
   applies. `ref` is `<source-id>#t=hh:mm:ss` (the decision's first `#t=`
   cite, else its first anchor). `quote` is that turn as the source holds
   it. `chosen` is the turn's own span of the decision text, matched
   without case, else the speaker's words. `question` is the decision text
   with `question_by: scribe`. `options` is `"unrecoverable"`,
   `decided_by` is `human`, and `decided_at` is the extraction time. A
   decision that fails a check is reported in `ledger.skipped` with its
   refusal code. It never blocks the rest. A private meeting feeds
   nothing (`ledger.private: true`): its text never reaches a ledger.
2. **Check.** One model call on the Standard tier through the Models
   port, so on the Meta route. It runs only when there are new entries and
   prior current entries (not reversed, not private, not an unconfirmed
   model answer; the newest 200 across every ledger), or open conflicts
   and to-dos. It returns the new entries that contradict a prior one
   (both settle the same matter differently), with the matter in a few
   words. It also returns the to-dos that act on a contested entry, meaning
   either side of a new conflict or of an open conflict to-do. Ids the call
   was not given are dropped. Ledger text is data, never instructions.
3. **Needs you.** Each new contradiction files one `owner:you` to-do
   tagged `office-scribe-contradictions` and `source:<id>`. Its title asks
   which decision stands. Its description cites both entries (id, time,
   chosen, ref) and ends with the `Conflicting entries:` line. It is
   exactly the routine's to-do, so the precedent provider reports it as an
   unresolved conflict. A failed read of open conflicts is reported as
   `ledger.conflicts_unread`.
4. **Hold.** An agent code to-do (see `meetings.agent_workspace`,
   [kt-videos.md](./kt-videos.md)) that acts on a contested entry is not
   filed for agents. It is filed `owner:you` with tag `contested`, its
   type and acceptance criteria, and a description ending
   `Acts on: <entry id>` and `Held until <conflict to-do> is resolved.`.

The tool's output carries `ledger`: `appended` entry ids, `skipped`,
`conflicts` (`entries`, `task`, `matter`) and `checked`.

**Resolving.** `office.decision-resolve` with the conflict to-do's `task`
and the entry that `stands` settles it in one call. It appends a
`decided_by: human` entry that copies the standing entry's question and
evidence and `reverses` the other, then closes the to-do (`done`). It then
reads the open `contested` to-dos the conflict holds. One acting on the
standing entry is filed for agents in `meetings.agent_workspace`, as an
ordinary code-change task with its criteria, and closed. One acting on the
overturned entry is `rejected`. It is retry-safe: when the other entry is
already reversed, nothing is appended. It refuses a task that is not an
open conflict to-do (`decision_conflict_not_found`), a `stands` that the
to-do does not name (`invalid_argument`), and a standing entry that was
itself reversed (`decision_reversed`). Answering by hand with
`office.capture` and `reverses` stays valid. It records the decision but
releases no held to-do.

## Scheduling markers and to-dos

- **Scheduling** (D10). A user's `#scribe <request> at <time>`, such as
  `#scribe add this as a scheduled task at 11PM tonight`, creates one
  **proposed** Orbit task in the office workspace with `not_before` set.
  The scribe agent resolves the time in the host's time zone against the
  marker message's timestamp. The plugin checks it is an RFC 3339 instant
  in the future and files the task through the user's `orbit` CLI. The
  description quotes the marker verbatim, states the resolved time and
  zone, and cites the SCR entry. The task stays proposed until the user
  approves it. The scribe never dispatches, starts or approves work. A
  time it cannot resolve is reported back and nothing is filed.
- **User action items.** When a pass finds an action the user owns ("I'll
  call Jane tomorrow"), it files an `owner:you` Orbit to-do. The office
  dispatch pass submits only `owner:agent` backlog tasks
  ([web-app.md](./web-app.md#authority)), so a to-do is never run. The
  Next section cites its id. Filing is idempotent per SCR and message, so
  a re-run files nothing new.
- **Agent-owned items** are not filed by the scribe. The orchestrator
  files its own tasks; Next cites them. An item with no task id is
  written as an Outcome and Blockers line, not as a Next item.

## The SCR file

One SCR per scribed session (D6).

**Location:** the Library's `sources/conversation/<yyyy>/<mm>/`, as
`SCR-<yyyymmdd>-<hhmm>-<session6>.md`. That is the session's local start
date and time, plus the first six characters of the session id. The name
is fixed at creation and never renamed, so v1's slug, clash suffix and
one-time rename retire. The id is the file stem. It is unique across
months, unlike v1's `SCR-<dd-hhmm>-<slug>`, so `previous:` can name it.

```markdown
---
id: SCR-20261101-2231-0a3104
title: Scribe v2 redesign direction
title_by: scribe
scope: project
project: orbit-office
session: 0a31040c-e7e2-411a-8100-1eba1b8c1dde
started: 2026-11-01T22:31:00-07:00
updated: 2026-11-01T23:05:00-07:00
previous: SCR-20261031-0907-77ab12
scribed_through: m-0187
on_by: user
provider: claude-code
model: claude-opus-5-5
host: macbook
cwd: ~/workspace/constellation/codebases/orbit-office
branch: main
transcript: ~/.claude/projects/-Users-daniel-…/0a31040c-….jsonl
parsed: library/.office/state/scribe/0a31040c-….jsonl
sensitivity: normal
---
## Summary
(scribe) Redesigned the scribe: opt-in sessions, marker-only tags, two ledgers, SCR as handoff.

## Goals
- Write the scribe v2 design for ORB-<id>.

## Outcome and Blockers
- The design is drafted; slices are not yet filed.
- Blocked: the Tags editor's saves need app authority (ORB-<id>). observed_at: 2026-11-01T23:00:00-07:00 recheck: before building the editor.

## Next
- ORB-<id> — file the implementation slices. owner: agent. authorized: "file them as proposed tasks" (m-0180).

## Tags
### Preferences
- T3 m-0142 2026-11-01T22:48-07:00 by: user scope: global "#preferences always draft emails, never send" proposal: <proposal-id>
### Decisions
- T4 m-0187 2026-11-01T22:39-07:00 by: user scope: project:orbit-office "#decision there are no users for this. so we can deprecate outright" ledger: dec-orbit-office-7
```

- **Front matter** carries D6's fields (`title`, `scope`/`project` for
  `project|global`, `session`, `started`, `updated`, `previous`) plus the
  v1 provenance keys still in use. Every key stays on one line.
  `previous` is the scope's latest SCR by `updated` when this SCR was
  created, read under the Library writer lock; it is null for a scope's
  first v2 SCR.
- **Summary, Goals, Outcome and Blockers** are rewritten on every pass
  (D6). They are model-written, so the Summary line starts with
  `(scribe)` as the scribe handoff did, and the brief shows that marker.
- **Next** is rewritten on every pass. Each item cites a complete Orbit
  task id (D6). An `authorized:` clause is a verbatim user quote with its
  message id, or it is absent: the scribe never infers consent.
- **Tags** has one subsection per tag in use: Preferences, Decisions,
  then custom tags in name order. Entries are append-only (D6). Each one
  is a verbatim quote with its transcript line ref (the message id) and
  its proposal, ledger or task id. Entries carry a permanent entry id
  `T<n>` and are cited as `<SCR id>#T<n>`, because rewritten upper
  sections move line numbers. `#L<n>` is not a stable anchor in a v2 SCR.
- **Commits.** Each pass is one Library commit, so the history of the
  rewritten sections stays readable through `office.source-show history`.
- **Budgets and checks** for the rewritten sections reuse the structured
  handoff validator ([sessions.md](./sessions.md#structured-handoff-contract)):
  complete task ids, no bare numbers, single-line items, observation
  times with offsets, and budgets of 500 (Summary), 500 (Goals), 2000
  (Outcome and Blockers) and 1000 (Next) characters.

### Titles

The title comes from the Summary (D6). On every pass, while the title is
not user-set, the scribe proposes a title of at most eight plain words
drawn from the Summary, sent as `title` with `title_by: scribe`. A user
title (`/scribe title`, `title_by: user`) wins and is never replaced. A
transcript `custom-title` is passed to the scribe as a hint, no longer a
precedence level. Since an SCR exists only after its first pass, which
always sets a title, a v2 SCR never lacks a title and needs no slug
fallback.

## Pipeline

```
transcript ──parse (no model)──▶ <library>/.office/state/scribe/<session-id>.jsonl
                                        │  messages after scribed_through, markers, tags, previous SCR
                                        ▼
                         scribe agent (cheap model) ──▶ sections + tag findings (JSON)
                                        │
           plugin: proposal-create per preference finding, orbit.task.add per proposed task or to-do
                                        │
           append (no model, validates) ──▶ SCR-<yyyymmdd>-<hhmm>-<session6>.md + decision ledger lines
```

1. **Parse: model-free.** The Claude Code plugin stages the provider
   transcript into the workspace first (see
   [Staging](#staging-tool-payloads-are-not-staged) below), then
   `office.scribe-pending` reads the staged file through the
   existing conversation adapter ([sessions.md](./sessions.md#adapter-and-provenance)).
   That adapter keeps only the text exchanged between Daniel and the agent:
   no tool calls, tool results, thinking or sidechains. Credentials are
   scrubbed as today. Harness wrapper text in a user message is not Daniel's
   words: `<system-reminder>` blocks, `<task-notification>` blocks and
   `<local-command-*>` output are
   dropped, and a slash command is kept as one `/<name> <args>` line
   ([sessions.md](./sessions.md#adapter-and-provenance)). A desktop side
   session's opening brief is the orchestrator's text, not Daniel's: it
   is kept as an assistant message, so it keeps its `m-<n>` id but
   never takes Daniel's tags. The brief opens with `Background (from
   the session that started this one):` in the desktop app (the
   unparenthesised form without the colon occurs too); both forms are
   matched case-insensitively, ignoring the parentheses and the
   trailing colon. The session's `project` is the git repository name:
   a Claude Code worktree cwd (`<repo>/.claude/worktrees/<name>`)
   names `<repo>` by that path rule, even when the worktree directory
   was removed by GC; otherwise, for a session at a repository or
   linked-worktree root, the plugin resolves the main worktree's
   top-level directory (through `git rev-parse --git-common-dir`) and
   passes it to `office.scribe-pending`, falling back to the `cwd`
   basename when git cannot resolve it that way (a non-git directory
   or a plain subdirectory). The plugin also passes the transcript's
   latest `customTitle` (`custom-title` record) to
   `office.scribe-pending` as `custom_title`, when present.
   Default topics come from that project name, so no worktree slug reaches
   them. Output is
   `<library>/.office/state/scribe/<session-id>.jsonl`, one message per line:
   `{"id":"m-0042","role":"user|assistant","at":"<rfc3339>","text":"…"}`.
   Ids are stable: the nth kept message is always `m-<n>`, so re-parsing a
   longer transcript only adds lines. The file is rewritten on every run and
   stays private (`0600`, private directory). It sits in the Library's
   git-ignored `.office/state/`, inside the plugin's `{{workspace}}/library`
   write grant (Orbit refuses any plugin write grant under the workspace's
   `.orbit`), and follows a configured `library_dir`. The `parsed` path
   (tool output and SCR front matter) is workspace-relative, such as
   `library/.office/state/scribe/<session-id>.jsonl`. The Claude Code
   plugin's own hook files stay under `.orbit/tmp/scribe/`.

   v2 adds to the parse result, without a model: `markers` (see the
   [trust rule](#trust-rule-whose-markers-count)), the tag definitions in
   force, the session scope, and the scope's latest SCR (its id and
   rewritten sections) as `previous`.
2. **Scribe: cheap model.** The `scribe` agent (the `scribe` model level)
   gets the rules, the tag definitions, the messages after
   `scribed_through` plus up to 10 earlier ones as read-only context, the
   markers, and the SCR's current sections. It returns the rewritten
   Summary, Goals, Outcome and Blockers and Next; a title while not
   user-set; one finding per marker (a ledger entry's fields, a preference
   statement with its `matter` and conflicts, or a task request); and,
   only for tags with inferred capture, candidates marked `by: scribe`.
3. **Route: plugin, through the user's CLI.** Before appending, the
   plugin files what needs an id: `orbit.task.add` per scheduling
   request (proposed, with `not_before`), `owner:you` to-do and
   `orbit_task` tag finding, so the task id lands on the SCR entry.
   Preference proposals are the exception: the proposed page cites the
   SCR entry (`<SCR>#T<n>`) only the append creates, and
   `office.proposal-create` resolves citations, so the plugin files each
   `needs_proposal` preference finding immediately after the append
   ([S5 decision](../4_decisions.md#scribe-v2-s5-scribe-control-stays-agent_callable-a-managed-session-cannot-turn-scribing-on)).
   Idempotency keys are the SCR id plus the
   marker's message id and line, kept in the plugin's per-session routing
   ledger (persisted before the append), so a retried pass files nothing twice.
4. **Append: model-free, Office tool.** `office.scribe-append` writes one
   Library commit:
   - It refuses a marker-derived finding whose marker is not in the parse
     result, a verbatim field that is not an exact substring of its cited
     message, a tag not in force, a section over budget or with an
     incomplete task id, and a `decided_by` or `by` that disagrees with
     the marker's author. Refusals are reported, never written.
   - It creates the SCR on the first pass, with `previous` read under the
     writer lock. It rewrites the four upper sections, appends new Tags
     entries with the next `T<n>` ids, and moves `scribed_through`. As in
     v1, a batch that refuses something and captures nothing writes no
     file and keeps the cursor.
   - It appends each decision to its scope's ledger with the next
     `dec-<scope>-<n>` id (creating the ledger file on first use), and in
     apply mode writes the preference page. All of this lands in the same
     commit as the SCR.

   The model never edits the files, so a bad run cannot corrupt earlier
   entries, and every quote points at real text.

### Built tools (S4)

[ORB-21117] built steps 1 and 4 and the read side. v1 runs unchanged
beside them: a session takes v2 when the caller passes `format: "v2"` or
its SCR is already v2, so the plugin switches over in S5
([4_decisions.md](../4_decisions.md#scribe-v2-s4-format-flag-marker-lists-and-local-private-parse)).

- **`office.scribe-pending` v2** (`format`, `scope`). The adapter marks
  the lines inside a `<pasted_content>` region of a user turn
  (`pasted`, 1-based line numbers per message); fenced blocks and
  blockquotes were already dropped. `markers` lists the line-start
  markers of the user's typed turns after `scribed_through`; markers in a
  fence, a blockquote, a paste region or mid-sentence are not markers.
  `agent_markers` lists the assistant's, kept apart and never
  authoritative. Each carries `msg`, `line`, `marker`, `tag` (null for a
  `#scribe <text>` instruction), `scope` (`@global`/`@<project>` or the
  tag default), `text`, `because`, `quote`, `by`, `in_force` and
  `large_turn` (over 40 lines or 4,000 characters). The result also
  returns `scr` (the `SCR-<yyyymmdd>-<hhmm>-<session6>` name, its
  sections, `tag_entries` and `next_tag`), the session `scope` (fixed
  once the SCR exists: `scribe_scope_fixed`), the scope's `previous` SCR
  with its sections, and the effective `tags` and `cadence`. A private
  session (the conversation sensitivity setting, or credentials in the
  transcript) still writes its local parse, so markers are detected
  locally, but returns no messages and its markers by reference only
  (`content_withheld`).
- **`office.scribe-append` v2** (`sections`, `findings`, `format`). It
  recomputes the markers from the parse over (`scribed_through`,
  `through`] and refuses `marker_not_found`, `quote_mismatch` (the
  quote is not the marker line verbatim, or a ledger field is not in
  its message), `decided_by_mismatch`, `tag_mismatch`, `tag_not_in_force`,
  `author_mismatch`, `decision_invalid`, `decision_not_found` (a
  `reverses` naming no entry), `preference_invalid`, `preference_withheld`
  (private session), `agent_marker_not_filed` and `task_id_incomplete`.
  Sections over budget, an empty item or a Next item without a task id
  refuse the call. A pass writes one Library commit: the SCR (front
  matter `id`, `title`, `title_by`, `scope`, `project`, `session`,
  `started`, `updated`, `previous`, `scribed_through`, … `sensitivity`),
  each decision appended to its scope's ledger with the next
  `dec-<scope>-<n>` id (the ledger is created on first use), and in
  apply mode the preference page `knowledge/domain/dom-preferences-<scope>.md`
  after `office.knowledge-write` validation. Tags entries are
  `- T<n> <msg> <time> by: <user|agent> scope: <scope> "<quote>"` plus
  `ledger:`, `matter:`, `page:`, `proposal:`/`propose:`, `task:`,
  `outcome:` or `withheld: private`; earlier entries are never rewritten.
  The Summary carries `(scribe) `; the title is the user title when set,
  else the first eight words of the scribe title or Summary. The SCR
  name, `previous` (the scope's prior latest SCR) and every bundled file
  are compare-and-swapped under the writer lock (`scribe_changed`), and
  a retried pass reports `duplicates` instead of writing twice. An agent
  marker is recorded `by: agent` and never changes a preference page in
  any mode; a paste-sized turn, a conflict or propose mode only reports
  `needs_proposal`. v1 `entries` and v2 keys are refused together, and
  v1 append refuses a private parse (`scribe_session_private`).
- **`office.scribe-query`** gains `decisions` (ledger entries with the
  derived `reversed_by` and `current`, filtered by `scope`, `project`,
  `decided_by`, `since`/`until`, `text`, `task` and `workspace`),
  `preferences` (page sections with `citation` `<page>#<matter>`,
  `stale`, and the SCR entries that `proposed` a change) and `scr` (one
  card per project plus the global card; `id` returns one SCR and its
  `previous`). v1 filters (`tag`, `topic`, `refs`, `session_id`) are
  refused in the v2 modes, and the v2 keys in the v1 modes.
- **`office.scribe-ask`** shortlists ledger entries (cited `dec-<scope>-<n>`),
  preference sections (`<page>#<matter>`) and v2 Tags entries
  (`<SCR>#T<n>`) beside the v1 lines.

Only the user's typed turns can carry a human marker
([trust rule](#trust-rule-whose-markers-count)). The v1 guard still holds:
an assistant line never becomes a human decision or a preference in effect.

### Built plugin (S5)

[ORB-21118] built the provider side in `plugin/claude-code`
(`scripts/scribe_v2.py`, the `v2_hook` in `scripts/scribe.py`):

- **Opt-in.** Every session defaults off. `/scribe on [@<project>|@global]`
  writes the shared record (`office.scribe-control`, now with `on_by` and
  `scope`) and runs the first pass; `/scribe off` stops passes and removes
  nothing; `update` (and the v1 `now`) runs a pass only while on; `status`
  shows the switch, scope, SCR id, `scribed_through` and the next pass;
  `tags` lists the definitions `office.status` reports. The v1 commands
  keep working during the window ([Migration](#migration), slice S9).
- **Orchestrators.** The `scribe` tool's `on` action records
  `on_by: agent` and its reply tells the agent to announce the turn-on in
  one line. A headless (`sdk-cli`) session is refused in the plugin
  (`scribe_headless_session`), and any turn-on under managed-task
  context is refused by the backend (`scribe_managed_session`).
- **Cadence.** With the legacy `auto_capture` off, Stop and SessionEnd
  take the v2 path: a session the plugin has never seen returns at once;
  otherwise the hook reads the shared record and, only while the session
  is on, the cadence (`office.status`, cached in session state). A due
  Stop (minutes or user turns since the last pass started, or a pending
  Capture-now) and every SessionEnd start one headless run
  (`claude -p` driving `/scribe update`, or `SCRIBE_PASS_COMMAND`) under
  a per-session pass lock the child inherits, and return without waiting.
- **Routing.** The `append2` step files scheduling, `orbit_task` and
  `owner:you` tasks before the append and preference proposals from its
  `needs_proposal` right after, under the idempotency keys above. Agent
  markers reach the append with no task id and are reported, never filed.
  A private session's update still withholds everything from the scribe
  agent, as in v1.

### Staging: tool payloads are not staged

The plugin's stager (`scribe_v1.py::V1.stage`) does not copy the provider
transcript whole. Per record it stages only what the adapter reads: the
record envelope (`type`, `sessionId`, `timestamp`, `cwd`, `gitBranch`,
`isMeta`/`isSidechain`/`isCompactSummary` and ids), the message role and
model, string content, and `text` content blocks verbatim. Every non-text
block (`tool_use`, `tool_result`, `thinking`, …) is staged as a minimal stub
carrying its type and small ids with its payload dropped, and other large
record fields (for example `toolUseResult`) are dropped. Malformed lines are
kept verbatim, as is a trailing complete record; a trailing partial record
is left out, as before.

- **Message ids are unchanged.** Ids count kept text messages only, so a
  staged parse assigns exactly the `m-<n>` ids an unstripped parse would.
- **Prefix stability.** Stripping is a pure per-record function with no
  whole-file threshold or positional logic, so re-staging a growing
  transcript leaves the staged bytes of unchanged records unchanged (the
  staged bytes feed the transcript and per-line prefix digests).
- **Privacy is unchanged.** Credential-shaped fragments in a dropped
  payload — provider token prefixes (`sk-`, `ghp_`, `AKIA`, `eyJ`, …), PEM
  blocks, secret assignments, bearer tokens and credential-bearing URLs,
  mirroring `office-core`'s `scrub` — are kept verbatim in a parser-ignored
  `_preserved` field on the stub (or on the record for dropped record-level
  fields). Office's scrub of the staged bytes therefore still marks such a
  session private and `scribe-pending` withholds it, as for the unstripped
  file. No other tool-payload text is staged.
- **Over-cap conversations.** Office's 8 MiB cap (`conversation::INPUT_LIMIT`)
  applies to the staged file. A tool-heavy session whose provider file is
  far over the cap normally stages under it and scribes normally. If the
  conversational text alone (with preserved credential fragments) still
  exceeds 8 MiB, `office.scribe-pending` still refuses the whole call with
  `session_transcript_too_large`: there is no windowed parse of the messages
  after `scribed_through`, because message ids need the full prefix. The
  practical remedy today is to scribe a long session in passes as it grows;
  a windowed reader in Office is future work, not part of this staging
  change. The legacy automatic-capture snapshots (`provider-scribe-…`) are
  unaffected: they still copy provider bytes whole under their own 8 MiB
  bound, and no `from_line` capture reads a `scribe-<id>.jsonl` file.

## Configuration

**Tag definitions and cadence live in the Library settings file**,
`.office/config.yaml`, under a `scribe:` block (D9):

```json
{
  "scribe": {
    "cadence": { "minutes": 20, "turns": 10 },
    "precedents": { "half_life_days": 90 },
    "tags": [
      { "name": "preferences", "scope": "global", "sink": "preference_page", "mode": "propose", "capture": "markers",
        "instruction": "A standing answer to a class of question, in the user's words." },
      { "name": "decisions", "scope": "session", "sink": "decisions_ledger", "mode": "append", "capture": "markers",
        "instruction": "A choice that was made, with the question and options from the turns before it." }
    ]
  }
}
```

That is the effective block for a Library with no `scribe` key, not text in
the file: `office.config-set` omits the block while cadence, tags and
precedent policy are default,
so a default change in a later release applies to a Library that never
customized them. A customized block is written with the rest of the file
as the canonical JSON mapping (valid YAML), which is the only form
`office-core` reads for lists of mappings; the settings YAML subset has no
nested list of mappings ([plugin-surface.md](./plugin-surface.md#library-settings-shape)).

- **Why the settings file.** Library settings already live in the
  Library, validated by `office-core` on every read and written only by
  `office.config-set` ([plugin-surface.md](./plugin-surface.md), the
  settings decision in [4_decisions.md](../4_decisions.md#plugin-config-holds-only-library_dir-library-settings-live-in-the-library)).
  `office.config-set` is `mcp_scope: none` and not agent-callable, so no
  agent can change a tag, a sink or a mode. That matters, because a tag's
  mode decides whether a preference takes effect without a proposal.
- **Keys** (built, [ORB-21115]). `office.config-set` accepts
  `scribe.cadence` (the block, `{minutes, turns}`, minutes 5–240, turns
  2–100; an omitted part keeps its default; null restores both),
  `scribe.tags` (the whole list; null restores the two defaults; `[]`
  means no tags) and `scribe.tags.<name>` (one definition, added or
  replaced; null deletes it, and refuses a name that is not defined).
  Deleting a default from the untouched defaults first materializes the
  other default. A definition needs `scope`, `instruction` (nonempty, at
  most 2000 characters) and `sink`; `name` defaults to the key and must
  match it; `mode` defaults by sink (`propose` for `preference_page` and
  `orbit_task`, `append` for `decisions_ledger` and `scr_only`);
  `capture` defaults to `markers`. A list holds at most 32 tags.
- **Precedent recency** (built, [ORB-21218]).
  `scribe.precedents.half_life_days` is an integer from 1 to 3650,
  default 90; null restores the default. It controls the read-only,
  host-facing `office.decision-precedents` provider's exponential decay
  ([decision-precedents.md](./decision-precedents.md#the-precedent-tool)).
  Invalid values or unknown keys inside a hand-edited `precedents`
  block are refused on load as well as on write.
- **Refusals.** Each is typed and leaves the file and HEAD unchanged,
  and none echoes the value. `invalid_library_setting` names the dotted
  key for: a sink that is not one of the four, a sink and mode pair other
  than `preference_page` with `propose` or `apply`, `orbit_task` with
  `propose`, or `decisions_ledger` or `scr_only` with `append` (key
  `scribe.tags.<name>.mode`), a duplicate or malformed name (lowercase
  kebab-case, at most 32 characters, starting with a letter; in a
  whole-list value a malformed name is keyed by its index, as
  `scribe.tags[0].name`), a `workspace` on a sink other than
  `orbit_task`, an out-of-range cadence, and any malformed field.
  `tag_sink_refused` is returned for any file target outside the
  Library: a `sink` shaped like a path (it contains `/`, `\`, `.`, `~` or
  `:`), an `orbit_task` `workspace` shaped like a path (it contains `/`
  or `..`, or starts with `.` or `~`), or a definition key that names one
  (`path`, `file`, `target`, `dir`, `directory`, `destination`,
  `output`). A scribe key outside
  those above is `unknown_library_setting`. The same rules apply
  when the file is read, so a hand-edited block cannot bypass them.
- **Reading the effective block.** `office.status` returns it as
  `scribe` (`cadence`, `tags`, `tags_customized` and `precedents`), never null: an
  unset key or an uninitialized Library reads as the defaults.
  `office.scribe-pending` returns the same `tags` and `cadence`, so a
  pass sees the definitions in force without a second call.
- **How the app writes them, and the dependency** (built, [ORB-21120]).
  The Scribe tab's **Tags** editor creates, edits and deletes tags (name,
  scope, instruction, sink, mode) and saves through `office.config-set`,
  as the Routines screen saves retention ([web-app.md](./web-app.md#authority)):
  `GET /api/scribe/tags` reads the effective block and
  `POST /api/scribe/tags` writes one save (`upsert` as
  `scribe.tags.<name>`, `delete` as that key with null, Restore defaults
  as `scribe.tags` with null), then reads the block back.
  The app has no operator authority: it calls Orbit as `local-cli`
  holding Agent, so Orbit refuses `office.config-set` today. That is the
  same refusal the approval actions get. Writing tags from the app
  therefore **depends on [ORB-20363]** (blocked), which wires the app to
  Orbit's scoped application authority ([ORB-20362]). That task's
  operation list must add `office.config-set`, limited to the `scribe.*`
  keys. Until it lands, the editor shows the tags read-only, reports a
  save as `not_permitted` in the words the approval actions use, and
  offers the equivalent `orbit tool run office.config-set` command for the
  operator. `/scribe tags` in Claude Code reads the same block. No
  app-owned file outside the Library is used: the app never writes the
  Library or `.orbit/` directly, and Office could not read a file in the
  app's own directory.
- **The rules page.** `dom-scribe-rules` keeps **Topics**, **Capture**
  (what to skip) and **Models**, still changed only by proposals. Its v1
  **Tags** section is no longer read; the migration slice proposes
  removing it ([Migration](#migration)). The `scribe.handoff` page flag
  retires with the scribe handoff, since every v2 pass writes the SCR
  that is the handoff.
- **Models.** Unchanged from v1: `query` none, `ask` `haiku@low`,
  `scribe` `haiku@medium`.

## Answering

1. **Query, model-free.** `office.scribe-query` gains three modes and
   keeps its date, project and session filters and its private
   withholding:

   | Mode | Returns |
   |---|---|
   | `decisions` | Ledger entries filtered by scope, `decided_by`, date range, text, `task` and `workspace` from `context`, each with its derived `reversed_by` and a `current` flag (not reversed). |
   | `preferences` | Preference page sections by scope and `matter`, with `stated_at`, `review_by` and a stale flag, and any open preference proposal for that matter. |
   | `scr` | SCR cards: the latest SCR per project and the latest global SCR, or with `id`, one SCR and its `previous`. |

   The v1 modes map as follows during the window
   ([Migration](#migration)): `pref` to `preferences`, `why` to `decisions`
   (entries with `because`), and `pending` and `landed` to nothing (Orbit
   to-dos and `done` tasks, which the brief reports).
2. **Ask, model on the shortlist.** `office.scribe-ask` and `/scribe ask`
   work as in v1. The shortlist now draws on ledger entries, preference
   page sections and SCR tag entries, and every claim cites an entry id
   (`dec-…`, `dom-preferences-…#<matter>` or `<SCR id>#T<n>`) whose quote
   must match verbatim. "Does Daniel have a preference on X?" answers from
   the preference page. "Why is X like this?" answers from the decisions
   with `because`, or says "no recorded reason". Neither guesses.

Orchestrators check `preferences` before asking the user, as in v1. A
current preference is applied with a one-line note citing it, so the user
can override it.

## Privacy

A private session (the conversation sensitivity setting, or credentials in
the transcript) keeps its private SCR. Its markers are detected locally
and its tag entries are written there. The shared stores get references
only:

- a decision becomes a ledger entry with `private: true`, carrying its id,
  scope, `decided_by`, `decided_at`, `ref` and `source`, but no question,
  options, chosen, quote or because. Ledgers therefore stay `normal`;
- a preference finding is not proposed. The pass report says it was
  withheld, and the user can restate it in a normal session;
- scheduling and to-do requests are filed with the marker text withheld
  and a link to the private SCR entry.

## Retiring `sources/note`

Retired outright, with no compatibility window (D12).

- **Capture.** `office.capture` with plain text and no `kind` no longer
  falls back to `note`. That fallback is
  `crates/office-store/src/repository/library.rs`, the `SourceKind::Note`
  arm of the kind default. Instead it refuses with
  `source_kind_required`. The message names the paste kinds (`chat`,
  `email`, `doc`) and says that a decision goes in a `decision` input,
  which appends to the [decision ledger](#decision-ledger). Plain text is
  never appended to a ledger by default: a pasted thread is not a
  decision.
- **Kind.** `note` is removed from `SourceKind`, from the settings kinds
  (`retention.<kind>`, `sensitivity.<kind>`), from the app's Routines
  retention kinds, and from the test fakes. `decisions` is added where a
  kind list is needed.
- **Existing notes.** The five note sources in the Library (all Daniel's
  directions of 2026-11-01, including the two this spec cites) move into
  the decision ledgers. Each becomes one entry: the note's title as the
  `question` (`question_by: scribe`), `options: "unrecoverable"`, its body
  verbatim as the `quote`, its first line as `chosen`, `ref`
  `<src-id>#L1`–`#L<n>`, `decided_by: human`, `decided_at` from its
  `occurred_at` (else `captured_at`, else the migration time), and
  `source` set to its original `src-…` id. A note tagged
  `project:<name>` goes to that project's ledger; any other goes to
  `global`. A private note is left in place, since a ledger is always
  `normal`. The note directories are removed in the same commit. The
  ledger's `moved_sources` maps each old id to its entry, and citation
  resolution and `office.source-show` resolve an old `src-…` id (with or
  without an `#L<n>` anchor, which counts lines of the original body, now
  the quote) to that entry, with `moved_from` naming the old id. Search
  finds the text in its ledger. Every existing citation therefore still
  resolves.
- **Running it.** The migration is a step of `office.init`, like the
  legacy-layout move: an operator runs `office.init` on the Library, and a
  re-run with nothing left to move reports `already_initialized` and
  commits nothing.
- **The AGENTS.md rule.** The constellation root `AGENTS.md` says to
  "append Daniel's decisions, directions (verbatim) and facts to the
  session's decision log with `office.capture`". Its replacement says:
  turn on the scribe for a session that matters (`/scribe on`); mark the
  user's decisions and preferences with `#decision` and `#preferences`, or
  ask the user to; record your own decisions with `#decision` in your
  reply (recorded `by: agent`); outside a scribed session, append a
  decision with `office.capture`'s `decision` input; and keep the handoff
  current with `/scribe update`. Propose Library page changes as before.
  That edit is a separate task in ws_constellation, filed by the
  migration slice; this repository does not edit it.

## Relation to what is built

| Built | In v2 | Slice |
|---|---|---|
| Conversation adapter and scrubbing ([ORB-20685]) | Kept for parse. Paste detection for markers is added (built). | [ORB-21117] |
| `office.scribe-pending`, `office.scribe-append` ([ORB-20780]) | Reworked (built, behind `format: "v2"`): markers, tag definitions, sections, `T<n>` entries, ledgers and the `previous` chain. | [ORB-21117] |
| `office.scribe-query` ([ORB-20798]) | Reworked: `decisions`, `preferences` and `scr` modes (built); v1 modes deprecated, then removed. | [ORB-21117], [ORB-21122], [ORB-21123] |
| `office.scribe-ask` | Kept; shortlist over the new stores (built). | [ORB-21117] |
| Plugin `/scribe`, subagent, hooks ([ORB-20799]) | Reworked: opt-in, Stop-hook cadence, markers, `update`, `tags`; `note`, `pending`, `pref`, `landed`, `why`, `backfill` retired. | [ORB-21118] |
| `office.scribe-control` ([ORB-20756]) | Kept; gains `on_by` and scope. | [ORB-21118] |
| `dom-scribe-rules` page ([ORB-20686]) | Kept for topics, capture and models; Tags section and `scribe.handoff` retired. | [ORB-21122] |
| Web Scribe screen ([ORB-20696]) | Reworked into the one-card feed with a Tags editor (built). | [ORB-21120] |
| Global handoffs and `/handoff <project>` ([ORB-21063]) | See [sessions.md](./sessions.md#scribe-v2-the-latest-scr-is-the-handoff). | [ORB-21119] |
| Structured handoffs ([ORB-21065]) | See [sessions.md](./sessions.md#scribe-v2-the-latest-scr-is-the-handoff). | [ORB-21119] |
| Scribe › Handoffs view ([ORB-21075]) | Replaced by the feed (built); see [sessions.md](./sessions.md#scribe-v2-the-latest-scr-is-the-handoff). | [ORB-21120] |
| Scribe titles and the slug fallback ([ORB-21079]) | **Kept:** the first-pass scribe title (at most eight plain words), the user title winning, the placeholder rule as a hint filter. **Reworked:** the title is derived from the Summary on every pass while scribe-set. **Retired for v2 SCRs:** the custom-title and side-session precedence levels, the humanized-slug fallback, the slug filename and its rename, and `/scribe backfill --titles`. v1 SCRs keep their titles and slug fallback for display until removal. | [ORB-21117], [ORB-21123] |
| Conversation sources as `src-…/source.md`, live scribe, recall | Unchanged and unused by v2, as in v1. | — |

### Feed

Built ([ORB-21120]). The Scribe tab's feed shows exactly one card per
project, its latest SCR, plus one global card, the latest global SCR
(D8). Each card shows the title, scope, `updated`, Summary, Next, Outcome
and Blockers, and the counts of new Tags entries (`tag_counts`, entries
per Tags subsection, beside the `tag_entries` total). Opening a card
shows the whole SCR. Its **Previous** link walks the `previous:` chain
one SCR at a time, which is the only way to reach history (D8). The feed
has no list of past sessions and no per-entry stream. Today's Handoffs
section shows the same cards. The Pending, Preferences, Landed and Why
views retire. A Preferences panel reads the preference pages and a
Decisions panel the ledgers, both through `office.scribe-query`.

The cards are the `office.session-brief` `view: list` answer
([sessions.md](./sessions.md#handoff-list)): the same card shape as
`office.scribe-query` mode `scr` (one shared builder). A private SCR's
card withholds its summary, sections, Tags entries and counts
(`content_withheld`); only its id, path, scope and times remain. The app
reads them at `GET /api/scribe/feed` and `GET /api/scribe/feed?id=…`,
and the panels at `GET /api/scribe/preferences` and
`GET /api/scribe/decisions` ([web-app.md](./web-app.md#screens)).

## Migration

The v1 surfaces keep working for 30 days after the v2 commands ship (the
compatibility window, D11). Then they are removed. `note` is the exception
and is retired at once (D12).

**The window.** The v2 commands shipped on 2026-11-01 ([ORB-21118]) and the
mapping below on 2026-11-02 ([ORB-21122]), so the window runs from
**2026-11-02** to **2026-12-02**. S10 ([ORB-21123]) runs on or after
2026-12-02.

| v1 | During the window | After |
|---|---|---|
| `/scribe` (catch-up) | Runs `/scribe update` and turns the session on, with a one-line notice. | `/scribe` alone shows status. |
| `/scribe now` | Alias of `/scribe update`. | Removed. |
| `/scribe note <text>` | Treated as a marker: `#<tag>` text routes to that tag; untagged text is refused with "use `#preferences` or `#decision` in chat". | Removed. |
| `/scribe pending`, `landed` | Answer from v1 files read-only, with `deprecated: true` and the replacement (the brief's Waiting on you and Agents working on). | Removed. |
| `/scribe pref`, `why` | Answer from v2 stores first, then v1 files, marked `deprecated`. | Removed; use `/scribe ask` or the panels. |
| `/scribe backfill` (+ `--titles`) | Refused with `backfill_retired`: v2 never scribes a session that was not turned on. | Removed. |
| `office.scribe-query` v1 modes | As the commands above. | Refused as an unknown mode. |
| v1 SCR files `SCR-<dd-hhmm>-<slug>.md` | Stay in place, readable and grep-able. Never rewritten and not in any `previous:` chain. | Same; they remain evidence. |
| `#preference` and `#decision` / `#agent-decision` lines in v1 SCRs; `direction` and `decision` lines in conversation logs | **Not backfilled.** v2 starts with empty preference pages and decision ledgers; the v1 lines stay readable as evidence (and through the `pref` and `why` modes during the window). Daniel restates what still holds as markers. | — |
| `#why`, `#pending`, `#resolved`, `#landed`, `#todo` lines | Not migrated. They stay readable in v1 files. Open `#pending` lines are listed once in the migration report for the user to turn into to-dos. | — |
| `handoff` entries through `office.capture` | See [sessions.md](./sessions.md#scribe-v2-the-latest-scr-is-the-handoff). | Refused with `handoff_retired`. |

**No backfill.** Daniel, 2026-11-02: "no need for backfill btw. its going
to be noisy. I rather start clean". A backfill would have filed one
proposal per v1 preference matter (about 184 on the real Library) plus a
decision-ledger list per scope, all for review at once. v2 therefore starts
with empty preference pages and ledgers; nothing reads v1 lines into them
([decision](../4_decisions.md#scribe-v2-s9-no-v1-backfill-v2-starts-clean-the-tidy-up-is-its-own-operator-tool)).

### Built (S9)

[ORB-21122] built the mapping and the one-time tidy-up:

- **Commands** (the Claude Code plugin). `/scribe` alone turns the session
  on (`on_by: user`, which runs the first pass) or, when on, runs
  `/scribe update`; `now` is `update`. Each reply carries `deprecated: true`
  and starts with a one-line notice. `/scribe note #<tag> …` writes
  nothing itself: `office.scribe-pending` (format `v2`) reads a
  `/scribe note #<tag> …` or `/<plugin>:scribe note #<tag> …` line in a
  user's typed turn as that marker (`#preferences`, `#decision`, or any tag
  defined in the settings), so it follows the [trust rule](#trust-rule-whose-markers-count),
  and the plugin runs the pass when the session is on. Untagged text is
  refused with `note_untagged`, a v1-only tag with `note_tag_retired`.
  `/scribe backfill` is refused with `backfill_retired`.
- **Queries.** `office.scribe-query`'s `pending`, `landed`, `pref` and
  `why` modes keep their v1 results and add `deprecated: true`,
  `replacement` (the v2 mode or brief section) and, for `pref` and `why`,
  `v2_results`: the matching preference sections (`preferences` mode) or the
  ledger entries with `because` whose question, quote, chosen or reason
  contains the matter or topic. The plugin shows `v2_results` first.
- **Tidy-up.** `office.scribe-migrate` (mutating, operator only, MCP
  scope none, `dry_run` optional) backfills nothing. It:
  - files one `knowledge_update` removing the `## Tags` section and the
    `scribe:` mapping from `dom-scribe-rules`, when the page still has
    either. The proposal carries the `scribe-migration` tag and a hashed
    `rules` key tag; a re-run skips it when it was filed in any status
    (a missing or truncated Orbit list is refused,
    `proposal_history_incomplete`);
  - returns the open v1 `#pending` lines (no `#resolved` naming them) in
    `pending` for the user to turn into to-dos, never filing them; lines
    in private SCRs, and all of them when the conversation sensitivity
    setting is private, are skipped (`skipped_private`).
- **Retired:** `scribe.handoff` (the capture handoff offer is always
  dropped as `disabled`; a page that still sets the flag stays valid).
- **The AGENTS.md task** is [ORB-21239] in ws_constellation.

## Build order

Each slice is a proposed task in ws_orbit-office. Each updates the docs
its change makes stale in the same change, as the workspace guide
requires.

| Slice | Task | Depends on | Delivers |
|---|---|---|---|
| S1 (built) | [ORB-21114] | — | Decision ledger store (`decisions` kind, `sources/decisions/<scope>.md`, `dec-` ids and citation resolution, the D13 schema), `office.capture` `decision` input, and the `note` retirement and migration (D4, D12, D13). |
| S2 (built) | [ORB-21115] | — | `scribe:` settings block (tags, cadence), `office.config-set` keys and validation, defaults (D3, D9). |
| S3 (built) | [ORB-21116] | S2 | `preferences` template and pages, proposal-time contradiction check in `office.proposal-create`, apply mode (D3, D4, D5). |
| S4 (built) | [ORB-21117] | S1, S2, S3 | SCR v2 format, pending and append v2 (markers and the trust rule, sections, `T<n>` entries, `previous`, title from summary, ledger and page writes), query and ask over the new stores (D2, D6). |
| S5 (built) | [ORB-21118] | S4 | Claude Code plugin: opt-in `/scribe on\|off\|update\|status\|tags`, Stop-hook cadence, orchestrator on/ask, routing (proposals, scheduling markers as proposed tasks, `owner:you` to-dos, `orbit_task` tags) (D1, D2, D10). |
| S6 (built) | [ORB-21119] | S4 | Session brief and `/handoff` read the latest SCR per project and global; capture `handoff` compatibility (D7; ORB-21063, ORB-21065). |
| S7 (built) | [ORB-21120] | S2, S6 | Web app: one card per project plus the global card, `previous` chain, Preferences and Decisions panels, Tags editor (saves need [ORB-20363]) (D8, D9; ORB-21075). |
| S8 (built) | [ORB-21121] | S1, S3 | Weekly `office-scribe-contradictions` auto-task definition, seeded disabled (D5). |
| S9 (built) | [ORB-21122] | S4, S5, S6 | Migration: v1 command and query mapping with `deprecated`, no v1 backfill (v2 starts clean), rules-page Tags removal proposal and open-pending report, the ws_constellation AGENTS.md task (D11). |
| S10 | [ORB-21123] | S9 | Removal at the end of the 30-day window (on or after 2026-12-02): v1 commands and modes, capture `handoff`, `view: list`, the v1 title levels (D11). |

## Task References

- [ORB-21083] — designed scribe v2 (this spec).
- [ORB-20840] — the orbit-office on-call session where Daniel gave the direction.
- [ORB-20362] — landed Orbit's scoped application authority.
- [ORB-20363] — wires the app to that authority (blocked); the Tags editor's saves depend on it.
- [ORB-20685] — built the conversation adapter's scrubbing and the live scribe's commands.
- [ORB-20686] — built the `dom-scribe-rules` page.
- [ORB-20696] — built the web Scribe screen over v1.
- [ORB-20756] — built `office.scribe-control`.
- [ORB-20780] — built `office.scribe-pending` and `office.scribe-append`.
- [ORB-20798] — built `office.scribe-query`.
- [ORB-20799] — built the Claude Code `/scribe` commands, subagent and hooks.
- [ORB-21063] — built global handoffs and `/handoff <project>`.
- [ORB-21065] — built structured handoffs.
- [ORB-21075] — built the Scribe › Handoffs view.
- [ORB-21079] — built first-pass scribe titles and the humanized-slug fallback.
- [ORB-21089] — plans Orbit's decision model, a consumer of decision precedent.
- [ORB-21090] — designed the office precedent provider over the decision ledger ([decision-precedents.md](./decision-precedents.md)).
- [ORB-21219] — built contradiction reporting to the precedent provider.
- [ORB-21340] — built meeting decisions in the ledger, their contradiction check and `office.decision-resolve`.
- [ORB-21114] — built slice S1: decision ledger store, capture `decision` input and the `note` retirement.
- [ORB-21115] — built slice S2: `scribe:` tag and cadence settings.
- [ORB-21116] — built slice S3: preference pages, contradiction check and apply mode.
- [ORB-21117] — built slice S4: SCR v2, markers and the trust rule, query and ask over the new stores.
- [ORB-21118] — built slice S5: Claude Code opt-in, Stop-hook cadence and marker routing.
- [ORB-21119] — built slice S6: the brief and `/handoff` over the latest SCR.
- [ORB-21120] — built slice S7: the one-card feed, `previous` chain and Tags editor.
- [ORB-21121] — built slice S8: the weekly contradiction auto-task.
- [ORB-21122] — built slice S9: the v1 command and query mapping and `office.scribe-migrate` (no backfill); opened the window on 2026-11-02.
- [ORB-21239] — replaces the root AGENTS.md decision-log rule in ws_constellation (filed by S9).
- [ORB-21123] — proposed slice S10: removal of v1 surfaces after the window.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
