---
type: design
summary: "Spec: decision ledgers (an append-only decisions source per scope), preference pages changed by proposal, the contradiction checks over both (weekly and when a meeting is extracted), the precedent half-life setting, the sources/note retirement, and read-only legacy SCR files"
last_validated: 2026-11-03
last_updated: 2026-11-03
---

# Spec: Decision ledgers and preference pages

Status: built. These stores came from the scribe v2 design ([ORB-21083];
slices [ORB-21114], [ORB-21116], [ORB-21121], [ORB-21340]). The scribe
itself (per-session opt-in, inline markers, the SCR writer, `/scribe` and
its tools, the Scribe screen and the Claude Code plugin) moved to the
standalone plugin `constellation-works/scribe` ([ORB-21747]) and was
removed from orbit-office ([ORB-21749]; see
[The scribe moved to constellation-works/scribe](../4_decisions.md#the-scribe-moved-to-constellation-worksscribe)).
What stays here is everything Office itself reads or writes without the
scribe: the decision ledgers, preference pages, the contradiction checks,
meeting decisions, the precedent setting and the read-only use of SCR
files already in a Library.

The ledger's rules trace to two Library notes holding Daniel's words
verbatim: `src-20261101-scribe-v2-redesign-direction` (cited as **D3**,
**D4**, **D5** and **D13**) and `src-20261101-retire-sources-note-outright`
(**D12**). After the [note migration](#retiring-sourcesnote) they resolve
as decision ledger entries under the same ids.

## Stores

Both stores fit the Library's sources/knowledge split
([library.md](./library.md#layout)): a decision is evidence of what was
said, so it is a source; a preference is a standing understanding the
user approves, so it is a knowledge page changed by proposal. The fit is
recorded in
[Scribe v2 store paths](../4_decisions.md#scribe-v2-store-paths-a-decisions-source-kind-and-templated-preference-pages).

### Decision ledger

**Paths:** `sources/decisions/<project>.md` and
`sources/decisions/global.md` (D4). `decisions` is the source kind that
replaced `note` ([Retiring `sources/note`](#retiring-sourcesnote)).

- **Layout.** A ledger is one file per scope, directly under its kind
  directory, not `sources/<kind>/YYYY/MM/<source-id>/source.md`. A ledger
  spans years, and the Library cites ids, not paths, so a fixed path loses
  nothing. The Library's enumeration, index and citation resolution know
  this layout alongside the two others.
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
  [Private entries](#private-entries)) and `moved_sources` (the note ids
  migrated in, mapped to their entry ids). A ledger whose front matter,
  ids or entry sequence disagree is refused (`decision_ledger_invalid`),
  never skipped.
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
[decision-precedents.md](./decision-precedents.md#entry-schema) says how
each field serves as precedent and adds four optional, ingestion-only or
dedupe fields (option `id`/`consequence`, `orbit`, `same_as`, `confirms`)
without renaming any field below.

| Field | Required | Meaning |
|---|---|---|
| `id` | yes | `dec-<scope>-<n>`, the entry's permanent id. |
| `question` | yes | What was being decided. When a message asked it, the verbatim question with `question_ref`. Otherwise a one-line restatement, with `question_by: scribe` (the contract value names the restating writer, whatever wrote it). |
| `options` | yes | The options considered, each `{text, ref}` verbatim from the evidence, or the string `"unrecoverable"` when it does not state them. Never invented. Ingested options also carry `id` and `consequence` from the Orbit request; `office.capture` refuses option `id`/`consequence`. |
| `chosen` | yes | The chosen option: a span of the decision's evidence, matched without case, punctuation or a word repeated in a row ([ORB-21431]). |
| `quote` | yes | The decision's verbatim text, and |
| `ref` | yes | its anchor: a source anchor (`<source-id>#<anchor>`), a task comment, or, in entries written before the scribe moved out, an SCR tag ref (`SCR-…#T12 m-0187`). |
| `because` | no | The stated rationale, verbatim with its `ref`, only when one was stated (D3). |
| `scope` | yes | `global` or `project:<name>`. |
| `context` | yes | `{task_type, workspace, task, risk_tier}`, each null when unknown. Never guessed. |
| `decided_by` | yes | `human`, `agent` or `model`. `model` is for a decision model's answer, written only by Orbit decision ingestion's feedback path ([ORB-21089], [ORB-21090]; [feedback rules](./decision-precedents.md#feedback)); it is never precedent until a human confirms it. |
| `decided_at` | yes | The decision's time, RFC 3339 with offset. |
| `reverses` | no | The entry id this one overturns. |
| `orbit` | no | Ingestion only: `{request_id, gate, category, topic, answered_by, channel}` from the answered Orbit request, plus `model` (assessment id, policy digest, model identity, probability, cited precedents) when the model answered. `office.capture` refuses it. |
| `same_as` | no | The entry id recording the same decision from the other source. Accepted by `office.capture`, checked like `reverses` (`decision_not_found`); also set automatically when an append matches exactly one ingested entry (same task, `decided_at` within 30 minutes, chosen matches the chosen answer). An ambiguous match links nothing and never blocks the append. |
| `confirms` | no | Ingestion only: the `decided_by: model` entry this human entry confirms. `office.capture` refuses it. |
| `reversed_by` | derived | Never written, since entries are append-only. Readers compute it from later entries' `reverses`, and `office.source-show` returns it. |
| `source` | yes | The evidence's source id; for a migrated note, its original `src-…` id; for an answered Orbit decision request, `orbit-decision:<id>`; for an older scribe entry, its SCR id. |
| `private` | no | `true` for a reference-only entry ([Private entries](#private-entries)). |

```markdown
- dec-orbit-office-7 2026-11-01T22:39:00-07:00 #decision {"id":"dec-orbit-office-7","question":"Should sources/note get a compatibility window?","question_by":"scribe","options":"unrecoverable","chosen":"deprecate outright","quote":"#decision there are no users for this. so we can deprecate outright","ref":"SCR-20261101-2231-0a3104#T4 m-0187","scope":"project:orbit-office","context":{"task_type":"feature","workspace":"ws_orbit-office","task":"ORB-<id>","risk_tier":null},"decided_by":"human","decided_at":"2026-11-01T22:39:00-07:00","reverses":null,"source":"SCR-20261101-2231-0a3104"}
```

**Appending.** An agent or the user appends a decision with
`office.capture` and a `decision` input carrying the fields above without
`id` (assigned on append) or `reversed_by` (derived). `ref` names a source
anchor (`<source-id>#<anchor>`) or a task comment
(`orbit://task/<task-id>#comment-<n>`, read through Orbit); `source` is
set from it (`orbit-task:<task-id>` for a comment), and a different
`source` is refused. Every verbatim field is checked against its ref
(`quote_mismatch`), an unreadable ref is refused
(`decision_ref_unresolved`), a ref to a private source is refused unless
the entry is `private` (`decision_source_private`), `decided_by: model`
is refused (`decided_by_mismatch`), and `reverses` naming no existing
entry is refused (`decision_not_found`). `kind: decisions` with text is
refused (`decision_input_required`). The append takes the writer lock
and commits once. Orbit decision ingestion
([decision-precedents.md](./decision-precedents.md)) and
[meeting decisions](#meeting-decisions) are the other writers.

### Preference pages

**Pages:** one preference page per scope, `dom-preferences-global` and
`dom-preferences-<project>`, stored as `knowledge/domain/dom-preferences-global.md`
and `knowledge/domain/dom-preferences-<project>.md` (D4; the Library
model has only `knowledge/domain/` and `knowledge/runbooks/`, with the id
deciding the path, [library.md](./library.md#knowledge-pages)). They use
the bundled one-per-scope template `preferences`
([knowledge-templates.md](./knowledge-templates.md#bundled-templates)):

- id pattern `dom-preferences-<scope>`;
- one `## <matter>` section per class of question. Each section states
  the current preference in one or two sentences, cites its evidence, and
  records `stated_at` and `review_by` (default 90 days);
- attach policy: included whenever a task in that project, or any task
  for the global page, is attached.

A page changes only through an accepted proposal (D3, D4): `knowledge_add`
for a scope's first page, else `knowledge_update` pinned to the page's
`sha256`. A new preference on an existing matter replaces that section's
statement; the page's history keeps the old one. Pages keep the
1,200-word body cap; past 1,000 words the
[weekly check](#contradiction-checks) proposes a consolidation.

### Private entries

A decision whose evidence is private becomes a ledger entry with
`private: true`, carrying its id, scope, `decided_by`, `decided_at`, `ref`
and `source`, but no question, options, chosen, quote or because. Ledgers
therefore stay `normal`.

## Contradiction checks

Contradictions are checked when they can arise (D5).

- **At proposal time.** `office.proposal-create` renders a model-free
  same-`matter` conflict, and any supplied `conflicts`, as a `## Conflicts`
  section on the proposal's evidence page, quoting the current statement
  and its citation, so accepting is a deliberate override.
- **Weekly.** The `office-scribe-contradictions` auto-task definition
  (weekly, `dedupe: skip_if_open`, `complexity: low`, no crew pinned) ships
  **disabled**, registered under `spec.definitions.auto_tasks` in
  `plugin.yaml` like every office routine. Its name is kept so open to-dos
  and dedupe keep matching. It reads every decision ledger and preference
  page. For a preference that conflicts with another preference, or with
  a later decision, it files one proposal to settle the page. For two
  decisions that conflict with no `reverses` link, it files one `owner:you`
  to-do asking which stands. The user's answer is recorded as a new
  decision with `reverses`. One proposal or to-do per conflict, each
  citing both entries. It never edits a ledger. It also compares decisions
  within an Orbit category (an ingested entry's `orbit.category`, or a
  `same_as` partner's category), even when the question text differs, and
  ends each decision-conflict to-do with a machine-readable
  `Conflicting entries:` line. The precedent provider reports a conflict
  whose to-do is still open
  ([ORB-21219]; [decision-precedents.md](./decision-precedents.md#contradictions)).
- **When a meeting is extracted.** See [Meeting decisions](#meeting-decisions).

### Meeting decisions

Built by [ORB-21340]. `office.extract` with `profile: meeting` feeds the
ledger. After it saves `extract-result.json`, and before it files to-dos:

1. **Append.** Each cited decision becomes one `decisions-global` entry
   through `office.capture`'s `decision` path, so every verbatim check
   applies. `ref` is `<source-id>#t=hh:mm:ss` (the decision's first `#t=`
   cite, else its first anchor). `quote` is that turn as the source holds
   it. `chosen` is the turn's own span of the decision text, matched
   without case, else the decision text. A transcribed turn stutters
   ("monthly, monthly. Starting in December"), so `chosen` need only be
   said by the turn up to case, punctuation and a word repeated in a row
   ([ORB-21431]); a statement the turn does not say is still skipped as
   `quote_mismatch`. `question` is the decision text with
   `question_by: scribe`. `options` is `"unrecoverable"`, `decided_by` is
   `human`, and `decided_at` is the extraction time. A decision that fails
   a check is reported in `ledger.skipped` with its refusal code. It never
   blocks the rest. A private meeting feeds nothing (`ledger.private:
   true`): its text never reaches a ledger.
2. **Check.** One model call on the Standard tier through the Models
   port. It runs only when there are new entries and prior current
   entries (not reversed, not private, not an unconfirmed model answer;
   the newest 200 across every ledger), or open conflicts and to-dos. It
   returns the new entries that contradict a prior one (both settle the
   same matter differently), with the matter in a few words. It also
   returns the to-dos that act on a contested entry, meaning either side
   of a new conflict or of an open conflict to-do. Ids the call was not
   given are dropped. Ledger text is data, never instructions.
3. **Needs you.** Each new contradiction files one `owner:you` to-do
   tagged `office-scribe-contradictions` and `source:<id>`. Its title asks
   which decision stands. Its description cites both entries (id, time,
   chosen, ref) and ends with the `Conflicting entries:` line. It is
   exactly the weekly check's to-do, so the precedent provider reports it
   as an unresolved conflict. A failed read of open conflicts is reported
   as `ledger.conflicts_unread`.
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

## Precedent recency

`precedents.half_life_days` in the Library settings is an integer from 1
to 3650, default 90; null restores the default. It controls the
read-only, host-facing `office.decision-precedents` provider's
exponential decay
([decision-precedents.md](./decision-precedents.md#the-precedent-tool)).
`office.config-set` writes it, and `office.status` reports it under
`precedents`. Invalid values or unknown keys inside a hand-edited
`precedents` block are refused on load as well as on write
(`invalid_library_setting`), and the file is written without the block
while the value is the default.

Before the scribe moved out, this setting was
`scribe.precedents.half_life_days`. A settings file that still carries a
`scribe:` block loads without error: the block is accepted and ignored,
its value is not migrated (the provider uses the default until
`precedents.half_life_days` is set), and the next `office.config-set`
rewrite leaves it out. The Library's git history keeps the old text. The
`scribe.*` keys are unknown to `office.config-set`
(`unknown_library_setting`).

## Legacy SCR files

SCR files that the scribe wrote into `sources/conversation/<yyyy>/<mm>/`
stay on disk, untouched. Office never writes, renames or deletes them,
and does not index them as sources. Two read-only uses remain:

- **The session brief.** `office.session-brief` still reads them as
  handoffs, with its contract unchanged
  ([sessions.md](./sessions.md#session-brief)): `view: list` returns the
  latest SCR per project and global as cards beside the legacy handoff
  entries, and the brief and handoffs views use the newest SCR per scope.
  A new SCR written by `constellation-works/scribe` into the same layout
  is read the same way.
- **Ledger refs.** Ledger entries written by the scribe keep their SCR
  `ref` and `source`; they stay valid evidence and precedent.

A store write the scribe started under an older build and left
unfinished (journal kinds `scribe` and `scribe_pass`) still completes or
rolls back on the next writer-lock recovery, so the Library is never left
half-written. Nothing new writes those kinds.

## Retiring `sources/note`

Retired outright, with no compatibility window (D12).

- **Capture.** `office.capture` with plain text and no `kind` no longer
  falls back to `note`. It refuses with `source_kind_required`. The
  message names the paste kinds (`chat`, `email`, `doc`) and says that a
  decision goes in a `decision` input, which appends to the
  [decision ledger](#decision-ledger). Plain text is never appended to a
  ledger by default: a pasted thread is not a decision.
- **Kind.** `note` is removed from `SourceKind`, from the settings kinds
  (`retention.<kind>`, `sensitivity.<kind>`), from the app's Routines
  retention kinds, and from the test fakes. `decisions` is added where a
  kind list is needed.
- **Existing notes** move into the decision ledgers. Each becomes one
  entry: the note's title as the `question` (`question_by: scribe`),
  `options: "unrecoverable"`, its body verbatim as the `quote`, its first
  line as `chosen`, `ref` `<src-id>#L1`–`#L<n>`, `decided_by: human`,
  `decided_at` from its `occurred_at` (else `captured_at`, else the
  migration time), and `source` set to its original `src-…` id. A note
  tagged `project:<name>` goes to that project's ledger; any other goes
  to `global`. A private note is left in place, since a ledger is always
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

## Task References

- [ORB-21083] — designed scribe v2, where these stores come from.
- [ORB-21114] — built the decision ledger store, capture's `decision` input and the `note` retirement.
- [ORB-21116] — built preference pages and the proposal-time contradiction check.
- [ORB-21121] — built the weekly contradiction auto-task.
- [ORB-21219] — reports an open conflict through the precedent provider.
- [ORB-21340] — built meeting decisions into the ledger.
- [ORB-21431] — relaxed `chosen` matching for transcribed stutters.
- [ORB-21089] — Orbit's decision model design.
- [ORB-21090] — the decision ledger as a precedent provider.
- [ORB-21747] — designed the standalone scribe plugin, `constellation-works/scribe`.
- [ORB-21749] — removed the scribe from orbit-office.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
