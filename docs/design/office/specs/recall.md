---
type: design
summary: "Spec: recall — answers about past conversations from captured conversation sources: session context and titles, stored and live refs, the model-free office.recall, cited office.recall-answer, and append-only corrections"
last_validated: 2026-11-03
last_updated: 2026-11-03
---

# Spec: Recall

Status: built ([ORB-20688]): session context and title ownership,
deterministic stored refs, read-only `office.recall` with feed mode,
`office.recall-answer` ([ORB-20756]), live PR refs through `gh`
([ORB-20710]), and append-only corrections with repeated-correction
signals ([ORB-20695]).

Recall answers "where did we discuss X?", "what did we decide about Y?"
and "did Z land?" from the conversation sources the Library already
holds, with citations and a way back to the conversation, without asking
an orchestrator. Conversation capture itself is specified in
[sessions.md](./sessions.md).

The scribe that used to drive capture and present recall (the provider
plugin, `/scribe`, `/recall`, the Scribe window and its controls) moved to
`constellation-works/scribe` and was removed from orbit-office
([The scribe moved to constellation-works/scribe](../4_decisions.md#the-scribe-moved-to-constellation-worksscribe)).
That plugin is a client of the tools below, like any other caller.

## Why This Exists

A user who works across several conversations at once loses track of
which one held a discussion, what was decided there and whether the
result shipped, and asks again. Asking the orchestrator of the current
conversation costs its context and its time, and it does not know what
other conversations said. The Library already holds one conversation
source per session; recall lets the user query them as one memory.

## Telling conversations apart

Each session is one `conversation` source keyed by `session_id`
([library.md](./library.md#sources)). Capture adds these frontmatter
fields, all nullable on existing sources:

```yaml
project: /Users/daniel/workspace/constellation   # session working directory at start
branch: agent-main                                # git branch at start, null outside a repo
started_at: 2026-11-01T01:02:03Z                  # first conversational record
last_active_at: 2026-11-01T03:10:00Z              # last conversational record captured
resume: { provider: claude, command: "claude --resume <session-id>" }
title: "Session context and recall"               # short title
title_by: scribe                                  # scribe | user
```

- `project`, `branch`, `started_at`, `last_active_at` and `resume` come
  from the transcript and the hook, deterministically.
- `title` is proposed by the extraction crew from the captured entries,
  at most 8 words, and marked `title_by: scribe`. It is replaced on later
  passes until the user sets one (`title_by: user`), which is never
  overwritten. With no crew (private sessions) the
  title is the first user line, truncated to 8 words.
- The provider's own conversation link (for example the desktop app's
  session link) is recorded in `resume` when the caller supplies it; the
  command form is always present for Claude Code.

### Capture contract (built)

Session-mode `office.capture` accepts optional `project`, `branch`, `resume`,
`repository`, `title` and `title_by` alongside its existing transcript fields.
The first conversational record's `cwd` and `gitBranch` take precedence over
hook fallbacks; start context is retained across later passes. Activity time
advances to the latest conversational record, even when the crew selects no
entries. `resume` defaults to a safely quoted `claude --resume <session-id>`;
a later hook can supply its provider link. No resume value is executed.

The crew passes its proposed title with `title_by: scribe` (the default).
With no proposal, the first user line supplies at most eight words. Private
capture ignores crew title proposals and uses that local fallback; an explicit
user title is retained. A hook can set or change a user title with `title`
and `title_by: user`, including on an unchanged transcript. Later crew
passes cannot replace it. No model call is added to capture.

`repository` is a known GitHub `owner/repo` supplied by the hook, for projects
whose filesystem path does not identify their repository. A project that is
itself `owner/repo` or a GitHub repository URL also supplies this context.
Ambiguous lines naming multiple repositories never bind a bare `#n`.

## Linking talk to outcomes

Captured lines that name work carry `refs`, extracted deterministically
by the adapter from the copied text, never by the model:

- Orbit task ids (`<PREFIX>-<n>`, prefixes from the registered workspaces);
- pull requests as `owner/repo#n` or GitHub PR URLs; a bare `#n` only when
  the same line or the session's `project` names its repository;
- commit SHAs of at least 7 hex characters in an `action` line.

Refs are stored on the entry. Their status is never stored: recall reads
it live (`orbit.task.show`, the PR's state and merge commit), so an answer
says what is true now, not what was true when the line was captured.

### Stored refs (built)

`entry_refs` in source frontmatter maps body-relative anchors (`L1`, `L2`, …)
to arrays of `{kind: task|pr|commit, id: <canonical identity>}`. This keeps
entry text verbatim and existing body citations stable. Normal conversation
capture, append and transcript passes index new entries; existing stored
identities are retained. Old sources without this field remain readable,
with empty refs until a later capture indexes them. Status is never written.

Task candidates use the general uppercase workspace-prefix/id syntax rather
than a hardcoded ORB/DANI/ORBA list. Recall sends each distinct identity to
Orbit's registered task router (`orbit.task.show`), which determines whether
it exists and is readable across workspaces; unregistered/unreadable ids are
unresolved. PR URLs normalize to `owner/repo#n`; action hashes normalize to
lowercase (7–64 hex characters). Other entry kinds never infer commit refs.

### Live PR state (built)

Task refs resolve through `orbit.task.show`, which the plugin already holds.
PR refs resolve live through `gh` ([ORB-20710]): a `PullRequests` port in
`office-core` (`ports/pr.rs`), implemented by the supervised `gh` adapter in
`office-store` (`pr.rs`, `GhPullRequests`), wired in the binary's `ports()`.
For each distinct PR ref the adapter runs, once and never retried,
`gh pr view <owner/repo#n> --json state,mergedAt,mergeCommit` under the
shared bounded supervisor (15-second deadline, capped output, cleared
environment forwarding only what `gh` needs to authenticate). A resolved
ref reports `status` as `open`, `closed` or `merged`, with `merged_at` and
`merge_commit` (each null when `gh` reports none).

On any failure the ref reads `unresolved` and the call still succeeds, with
a reason naming the cause: `pr_source_unavailable` when `gh` is missing or
not granted (it cannot be started), `pr_lookup_failed` when `gh` runs but
fails, times out or exceeds its bound, `invalid_pr_response` when its reply
does not match the expected shape, and `invalid_pr_ref` when the stored id
is not a canonical `owner/repo#n`. Program output is never echoed.

`gh` is declared in `spec.requires.programs` in
`plugin/.orbit-plugin/plugin.yaml`. **Operator step:** because that grant
changed, upgrade the office plugin and re-enable it so `gh` is granted to
the backend; until then PR refs read `unresolved` /
`pr_source_unavailable` while task refs keep resolving.

## Asking

### The tool: `office.recall`

Read-only, `mcp_scope: workspace`, agent-callable. Input:
`{"question": "<text>"}` plus optional `session_id`, `since`, `until`,
`project` and `limit` (1–50, default 20). Without a question it runs in
**feed mode** (below). It makes no model call. It returns:

- `entries`: the matching conversation entries (full-text over entry
  text, source titles and refs), newest first, each with its source id,
  line anchor, kind, text, time and refs;
- `sessions`: one item per source that contributed an entry, with its
  title, project, branch, started/last-active times, `resume` and
  `session_id` (null on sources captured without one);
- `refs`: each distinct ref with its live status (task status and
  workspace; PR state, merged-at and merge commit), or `unresolved` with
  the reason;
- `coverage`: the earliest captured session date, sessions skipped as
  private, and providers not captured, so an empty result can say why.

Private sources contribute session items and refs but never entry text.

### Feed mode (built)

With `question` absent or null, `office.recall` returns every entry newest
first, under the same filters and the same response shape. `limit` is
1–200 in feed mode (default 20), so a caller can read a long session; with a question it stays 1–50. `session_id`
is an exact match on the source's engine session identity, in both modes:
only that session's conversation source contributes, so a caller scopes the
feed to one session without relying on the search terms. Private
entries still count toward the limit and contribute only their session
item and refs.

### Query behavior (built)

The question is a literal case-insensitive search: all whitespace-separated
terms must occur across the entry text, source title and stored ref ids.
There is no natural-language inference or model call. `since` and `until`
are inclusive RFC 3339 timestamps or UTC dates (a date's entire day is
included); `project` is an exact match. Ordering compares instants, including
offsets and fractional seconds, with stable source/anchor tie breaks. `limit`
bounds matching public and private entries together. Each contributing source
appears once in `sessions`; private matches contribute metadata and refs only.

`coverage.earliest_session_at` spans the whole Library and falls back to
`occurred_at`/`captured_at` for old sources. `private_sessions_skipped` counts
all private conversation sources, independently of filters; the
`providers_not_captured` list currently contains `codex`.

Every distinct task ref gets a fresh `orbit.task.show` read. Task workspace
is returned when Orbit supplies a workspace selector/id/path, otherwise null
(the current task projection has no workspace field). Callback failures and
invalid replies are `unresolved` with fixed reasons, without upstream error
text. PR refs resolve live through `gh` (see Live PR state above) and fall
back to `unresolved` with a cause-naming reason; commit refs use
`no_commit_source`. The call
writes nothing, and refuses a Library change during its reads so the caller
can retry. Backend contract and offline conformance checks live in
`schemas/recall.{request,response}.json` and `tests/conformance/recall.yaml`
under `plugin/.orbit-plugin/`.

### Corrections

Built ([ORB-20695]).

A captured entry can be corrected as **Not a decision**, **Wrong kind**,
**Wrong wording** or **Remove**, and a missed line can be added. The log
stays append-only: a correction is a new `correction` entry in the same
source that names the corrected anchor and the user's choice, written
through `office.capture` with `append_to`. Office ships no correction
screen; a client such as `constellation-works/scribe` calls the backend.

- Readers apply corrections: the brief, the digest and `office.recall`
  treat a corrected entry as its corrected kind, or as absent after
  **Remove**, and show that it was corrected.
- A missed entry writes the user's text as a `direction`,
  `decision` or `fact` entry chosen by the user, verbatim, marked
  `by: user`.
- Repeated corrections of the same kind are a signal for a
  `knowledge_update` proposal to `dom-scribe-rules`, quoting the corrected
  entries ([proposals.md](./proposals.md#signals)).

Append through `office.capture`, for example:

```json
{"append_to":"<source-id>","entries":[{"kind":"correction","correction":{"anchor":"#L1","choice":"not_decision"}}]}
```

Choices are `not_decision` (effective kind `open`), `kind` with a `kind`
field, `wording` with nonempty verbatim `text`, and `remove`. A kind change
accepts any ordinary log kind, never `correction`. Only local anchors of
original entries already in the source are accepted; corrections cannot target
other corrections, prose, future lines or entries in the same append batch.
All entries in a refused batch remain unwritten. Corrections compose in append
order; removal stays final. The original anchor and timestamp stay attached to
the effective entry. Brief and recall entries add `corrected: true`; digest
lines retain the effective kind and `(corrected)` marker even if its model
selects a plain excerpt. Multiline wording contributes complete individual
lines to digest evidence, each with the original entry anchor. Removed entries
and correction audit records are not
returned as ordinary reader evidence. A digest already published is immutable;
these rules apply when creating a new digest.

A missed entry is `{ "kind": "fact", "text": "<verbatim>", "by": "user" }`
(or `decision` / `direction`). Attribution is a caller claim, not authenticated
identity. Its single physical log line stores
`[<time>] fact (by: user): "<JSON-escaped verbatim text>"`, preserving spaces
and newlines as JSON escapes. This extends the kind delimiter, so legacy text
that happens to contain JSON or attribution words keeps its original meaning.
Credential-bearing user additions and replacement wording are refused before
writing, so accepted text stays verbatim through later scrubbed transcript passes.
Readers decode the exact text and expose `by: user`. Correction lines store the
JSON correction payload after `correction:`. Legacy plain entry lines keep
their format; a `text` containing a canonical JSON correction is also accepted.
The stored log body stays append-only, including through later transcript passes.

At least three corrections with the same **choice** within
`thresholds.window_days` produce a `scribe_corrections` candidate. Its kind is
`knowledge_update`, its target evidence includes `dom-scribe-rules`, and its
summary quotes the original entries with original and correction anchors. The
scan clock excludes future corrections; the window includes its exact lower
boundary. This proposes a change for review and never edits the rules page.

### Answering: `office.recall-answer`

`office.recall` stays model-free. A separate tool, `office.recall-answer`
(built, [ORB-20756]; read-only, `mcp_scope: workspace`, not `agent_callable`
— read-only tools keep ordinary Agent access), takes a question and
`office.recall`'s filters (`session_id`, `since`, `until`, `project`,
`limit` 1–50; no feed mode), runs recall, then makes at most one
Standard-tier call through the Models port under the privacy routing of
[models-and-privacy.md](./models-and-privacy.md#routing-rules). It
returns `{answer, derived_by, recall}`, where `recall` is the full
`office.recall` payload and `answer` holds:

- `claims`, each with `cites` (`<source-id>#L<n>` anchors). A claim
  survives only when every cite names an evidence anchor sent to the
  model; others are dropped and counted in `dropped_claims`.
- `conversations`: each cited conversation with title, project, date,
  `session_id` and its `resume` command.
- `refs`: the live status (from recall) of the cited entries' refs.
- `coverage`, and `status`: `answered`; `not_captured` when no evidence
  answers (no model call when recall found nothing, so `derived_by` is
  null); or `reference_only` when only private sources matched and no
  open-weights route is configured.
- `text`: the rendered answer — cited claims, conversations with their
  resume commands, live status, or the "not captured" line with coverage.

Private entries reach the model only when the Library settings carry an
open-weights route; the call is then `private`, so the Models adapter
routes it to that endpoint. Even then, a claim citing a private anchor
returns by reference only (`text: null`, `content_withheld: true`): the
tool is MCP-visible, and derived private text is withheld like every
MCP-visible read ([4_decisions.md](../4_decisions.md#private-content-is-withheld-from-mcp-visible-read-tools)).
Without the route, private sources are listed in `private_references`
(source id, title, path, project, session id, resume) and their text goes
to no model.

The answer is cited, names each conversation with its resume command,
takes work status only from the live `refs`, and says "not captured" with
`coverage` when nothing answers. It never fills a gap from model
knowledge, and it never involves an orchestrator.

## Invariants

- One writer: the Library is written only through `office.capture`,
  corrections included.
- Answers come only from captured entries and live ref status.
- Raw transcripts never leave the host that holds them; only captured
  entries reach the Library and its sync.

## Task References

- [ORB-20688] — built session context, stored refs and the recall backend.
- [ORB-20689] — built the original Claude Code provider plugin and recall commands (since removed).
- [ORB-20695] — built append-only corrections and effective-entry projection.
- [ORB-20710] — resolved PR refs through a live gh source.
- [ORB-20756] — built recall-answer and feed mode.
- [ORB-21749] — removed the scribe and its provider plugin from orbit-office.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
