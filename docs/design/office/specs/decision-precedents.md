---
type: design
summary: "Spec: the scribe v2 decision ledger as a precedent source for Orbit's decision model: the learning-fit entry schema, ingestion of answered Orbit decision requests with dedupe, the read-only office.decision-precedents provider over orbit.precedent-provider/1, feedback and contradiction rules, and a read-only agreement view"
last_validated: 2026-11-02
---

# Spec: Decision ledger as a precedent provider

Status: designed ([ORB-21090]). Slice P1 (entry schema additions, their
validation and refusals, `same_as` matching in `office.scribe-append` and
`office.capture`, and reading them through `office.source-show` and
`office.scribe-query`) is built ([ORB-21216]). Slice P2 (`office.decision-ingest`,
its callbacks and the disabled `office-decision-ingest` routine) is built
([ORB-21217]). Slice P3 (the provider and half-life setting) is built
([ORB-21218]). Slice P4 (contradiction reporting) is built
([ORB-21219]). The remaining slices are filed as proposed tasks (see
[Build order](#build-order)).

Daniel asked for a decision model (2026-11-01, in the orbit-office on-call
session [ORB-20840]). It is a cheap model that predicts the answer to a new
human decision from recorded precedent, so agents ask it first and come to
him only when its confidence is low. "Let's prepare our designs to support
this now."

Orbit owns the model, the measurement and the authority. Its design is
[ORB-21089]: the consumer spec `docs/design/decision-assistance/specs/precedent-decisions.md`
and the decision-channel changes in `docs/design/human-decision-channel/2_design.md`
§2.1 and §11, in the Orbit repository (`constellation-works/orbit`). This
spec is written against those documents as of Orbit `agent-main` commit
`266d234e95f522c86a09d0e67dba0da2a5619086`. It cites them as **Orbit
§<section>**: for example, Orbit §Precedent provider is that consumer
spec's section of that name.

This spec designs only the Office half: the
[scribe v2 decision ledger](./scribe.md#decision-ledger) as a precedent
source. Orbit core holds no Office-specific code (Orbit §Precedent
provider). Office implements Orbit's generic contract and never answers a
decision.

## Goals and non-goals

Goals:

1. The ledger entry carries every field a precedent needs, under the names
   the scribe already writes ([Entry schema](#entry-schema)).
2. Answered Orbit decision requests are written into the ledger, so both
   sources sit in one place, without counting one decision twice
   ([Ingesting Orbit decisions](#ingesting-orbit-decisions)).
3. A read-only tool, `office.decision-precedents`, implements
   `orbit.precedent-provider/1` ([The precedent tool](#the-precedent-tool)).
4. Model answers never become precedent until a human confirms them, and
   a human override is recorded as a precedent ([Feedback](#feedback)).
5. Unresolved conflicts between decisions reach the consumer
   ([Contradictions](#contradictions)).
6. The user can see, per category, how often the model agreed with them
   ([Agreement view](#agreement-view)).

Non-goals:

- Office never predicts, scores, answers, confirms or vetoes a decision.
  Those are Orbit's, under Orbit's authority rules (Orbit §Measured gate and
  enablement, §Recording, veto and feedback).
- Office never shows a model's per-request prediction. A visible prediction
  would bias the label it is scored against (Orbit §Purpose and limits). The
  agreement view shows aggregates only.
- No change to how the scribe extracts decisions, or to the trust rule
  ([scribe.md](./scribe.md#trust-rule-whose-markers-count)).

## Entry schema

The ledger and its D13 entry are specified in
[scribe.md](./scribe.md#decision-ledger) and built (S1, S4). This section
says how each field serves as precedent, and adds four optional fields.
Every existing field keeps its name and meaning; no field is renamed or
retyped. An entry still is one line, `dec-<scope>-<n>`, the time,
`#decision` and one compact JSON object, appended and never edited.

| Field | As precedent |
|---|---|
| `question` | The precedent's question. A scribe restatement (`question_by: scribe`) is allowed; a verbatim one has `question_ref`. An ingested Orbit request sets `question_ref: orbit-decision:<id>`. |
| `options` | The options considered, or `"unrecoverable"` (see [Entries without options](#entries-without-options)). Each option is `{text, ref}`, as built. **Added, optional:** `id` (the option's stable id; set only by Orbit ingestion, from the request's `answers[].id`) and `consequence` (the request's one-line consequence). |
| `chosen` | The chosen option. For a scribe entry, a span of the decision message (up to case, punctuation and a word repeated in a row); for an ingested entry, the chosen answer's label. |
| `quote`, `ref` | The evidence: the verbatim decision text and its transcript or Orbit reference. |
| `because` | The stated rationale, when one was stated. It goes into the precedent's `context`. |
| `scope` | `global` or `project:<name>`. Used to filter candidates. |
| `context` | `{task_type, workspace, task, risk_tier}`, each null when unknown. Ingestion fills `workspace`, `task` and `risk_tier` from the request (`risk_tier` from Orbit §Risk tiers; `unknown` stays `unknown`). |
| `decided_by` | `human`, `agent` or `model`. Only `human` entries, and `model` entries a human confirmed, are precedent ([Feedback](#feedback)). |
| `decided_at` | When it was decided. Recency weighting and the provider's `as_of` cut use it. |
| `reverses` / `reversed_by` | A reversal, written on the later entry; `reversed_by` is derived on read. A reversed entry is returned flagged, never as support. |
| `source` | The source id: an SCR id, a migrated note's `src-…` id, `orbit-task:<id>` for a captured task comment, or `orbit-decision:<id>` for an ingested request. |
| `private` | A reference-only entry. Never returned by the provider ([Privacy](#privacy)). |
| `orbit` | **Added, optional; ingestion only.** `{request_id, gate, category, topic, answered_by, channel}` copied from the answered request (Orbit human-decision-channel §2.1), plus `model` (the model citation: assessment id, policy digest, model identity, probability, cited precedents) when `answered_by` is `model`. |
| `same_as` | **Added, optional.** The id of an earlier entry that records the same decision from the other source ([Dedupe](#dedupe-against-scribe-entries)). |
| `confirms` | **Added, optional; ingestion only.** The id of a `decided_by: model` entry this human entry confirms ([Feedback](#feedback)). |

`office.capture`'s `decision` input and `office.scribe-append` refuse
`orbit`, `confirms` and option `id`/`consequence`, as they refuse
`decided_by: model`: only ingestion writes them (`decided_by_mismatch` for
`model`, and the existing unknown-key refusal for the rest). They accept
`same_as`, checked like `reverses` (`decision_not_found` when no such
entry exists).

### Entries without options

An entry whose `options` is `"unrecoverable"` stays precedent: it records
what the human chose, in their words. It is not a labelled choice, because
nothing says what was turned down. The provider returns it as a
single-option record: `options: [{"id": "chosen", "label": <chosen>}]`,
`chosen_option: "chosen"`, and its `context` starts with the fixed line
`Options not recorded; only the chosen option is known.`. That satisfies
Orbit's check that `chosen_option` is one of the precedent's own options
(Orbit §Host validation) without inventing an alternative.

Recovered options from a scribe entry have no ids. The provider numbers
them `o1`…`on` in ledger order, labels each with its `text`, and maps
`chosen` to the option whose text equals it after case and whitespace
folding, or else the one option whose text contains it or is contained by
it. When no single option matches, the entry is returned like an
unrecoverable one, with the recovered options listed in `context`. An
ingested entry keeps the request's own answer ids.

## Ingesting Orbit decisions

Answered Orbit human-decision requests are appended to the ledger, so the
Library's readers (`office.scribe-ask`, the Decisions panel, the weekly
contradiction routine) see both sources.

**Tool:** `office.decision-ingest` (mutating, `mcp_scope: none`), run by a
new routine `office-decision-ingest` (hourly at minute 20, seeded disabled like every
Office routine; its job's `max_active_runs: 1` keeps one run at a time, since
routines carry no `dedupe` key). Input `{since?, dry_run?}`; output
the entries appended, the requests skipped with a reason, and the
dedupe outcomes.

**Read.** `orbit.decision.list` with `include_closed: true` lists every
request with its effective state and, when answered, its `resolution`
(`answer_id`, `answered_by`, `provenance`, `note`, `answered_at`; Orbit
human-decision-channel §2.1). Ingestion calls it as a plugin callback under
the `orbit_tools` grants `orbit.decision.list` and `orbit.decision.show`:
once for the Office workspace and once per followed selector (`workspace`),
applying `since` itself. `orbit.decision.show` fills a resolution only when
a listed answered request omits it. The Office workspace failing refuses
the run; a followed workspace failing is reported in `unavailable`.
Ingestion never reads Orbit's stores directly.

**Which requests.** Every answered request in a followed workspace
(`brief.task_workspaces`, [sessions.md](./sessions.md#session-brief)) whose
`resolution` was written by a human label, by the `model` actor, or that
answers the `model_answer_review` gate. Requests answered by the
`precedent` actor (curated-rule auto-answer) are skipped: they are not
human decisions (Orbit §Precedent provider, Host validation). Superseded
and expired requests are skipped.

**Mapping.** One request becomes one entry in the ledger of its scope:
`project:<name>` when the request's workspace maps to a Library project,
else `global`.

| Entry field | From the request |
|---|---|
| `question`, `question_ref` | The request's question text (the agent question, the final-recovery summary or the gate's fixed wording); `orbit-decision:<id>`. |
| `options` | `answers[]`: `{id, text: label, consequence, ref: "orbit-decision:<id>#<answer id>"}`. |
| `chosen` | The chosen answer's label. |
| `quote` | The resolution note when there is one, else the chosen answer's label. |
| `ref`, `source` | `orbit-decision:<id>`. |
| `scope` | As above. |
| `context` | `{task_type, workspace, task, risk_tier}` from the request and its task. |
| `decided_by` | `human` for a human label; `model` for the `model` actor. |
| `decided_at` | The resolution's applied-at time. |
| `reverses` | See [Feedback](#feedback). |
| `orbit` | Request id, gate, category, topic, `answered_by`, channel, and the model citation when present. |

Ingestion is idempotent: the request id and its answer time are the key,
and a request answer already in any ledger is skipped (`already_ingested`).
A later answer to the same request (Orbit re-opens it on a veto) is a new
entry that `reverses` the newest earlier entry for that request. Each entry
is checked like any ledger append; one that fails is reported `invalid`
and not written, and the rest of the run still appends. One commit per
run, under the writer lock (`decision_ledger_changed`, nothing written,
when a ledger changed since it was read).

Build details ([decisions](../4_decisions.md#decision-precedents-p2-ingestion-reads-include_closed-keys-on-request-and-answer-time-and-writes-one-batch)):

- **Workspace and scope.** The request's workspace is its category's
  leading `<workspace_id>/`, else the followed selector's last `ws_`
  component; `ws_<name>` maps to `project:<name>` when `<name>` is a
  ledger slug, else the entry is `global`.
- **Question.** The request's `question`, else its `summary`, else a fixed
  wording per gate.
- **Quote.** The note when it contains the chosen label; otherwise the
  label, a newline and the note (the ledger needs `chosen` verbatim in
  `quote`); the label alone when there is no note.
- **Review binding.** A `model_answer_review` request names the reviewed
  request in `binding.request_id` (else `review_of`). Answers are ingested
  oldest first; a review whose model entry is not yet in a ledger is
  skipped `review_target_missing` and retried on a later run.
- **Output.** `appended[]` (`request_id`, `entry`, `ledger`, `gate`,
  `decided_by`, `same_as`, `confirms`, `reverses`), `skipped[]`
  (`request_id`, `reason`: `precedent_actor`, `superseded`, `expired`,
  `already_ingested`, `resolution_missing`, `answer_unknown`,
  `review_target_missing`, `unreadable` or `invalid` with `code` and
  `message`), `dedupe[]` (`same_as` or `dedupe_ambiguous` with
  `candidates`) and `unavailable[]`.

### Dedupe against scribe entries

The same decision can arrive twice: Daniel answers an Orbit request and
also marks `#decision` in the session where he did it. Both entries stay
(the ledger is append-only, and each is evidence), and the later one links
the earlier with `same_as`.

A scribe or captured entry and an ingested entry match when all hold:

- the same task (`context.task` equals the request's task);
- `decided_at` within 30 minutes of each other;
- the scribe entry's `chosen` matches one of the request's answers, by the
  same folding and containment rule as in
  [Entries without options](#entries-without-options), and that answer is
  the one chosen.

Exactly one match links. Ingestion sets `same_as` on the ingested entry
when the scribe entry came first. When the scribe entry comes later,
`office.scribe-append` and `office.capture` run the same match against
ingested entries and set `same_as`. More than one candidate links nothing
and is reported as `dedupe_ambiguous` in the ingest output. A rejected
match never blocks the append.

A `same_as` group is one decision. Readers show it once, citing both
entries. The provider never returns a group that has an ingested member
(see below).

## The precedent tool

`office.decision-precedents` (read_only, `mcp_scope: none`) implements
`orbit.precedent-provider/1` (Orbit §Precedent provider). The operator
names it in Orbit's host-only provider list (Orbit §Configuration); Orbit
calls it through the plugin callback route under its grant (Orbit
§Sources). `mcp_scope: none` keeps it out of agents' tool lists: it is a
host-facing contract, and `office.scribe-ask` already answers agents'
questions over the ledger.

**Input** is the contract request, exactly: `contract`, `workspace_id`,
`request` (`id`, `gate`, `category`, `question`, `options[]` of `{id,
label, consequence}`, `topic`, `raised_at`), `as_of` and `k`. Unknown keys
are refused. A different `contract` is refused
(`precedent_contract_unsupported`). `k` must be an integer from 1 to 8.
The host contract additionally requires `as_of` to equal `request.raised_at`,
1–8 options with distinct ids, nonempty labels and question, ids at most
256 bytes (option ids 64), every string at most 8,192 UTF-8 bytes, and the
encoded request at most 65,536 bytes. Optional `topic` and `consequence`
may be absent or null. Timestamps are RFC 3339 with explicit offsets.

**Output** is the contract response, exactly: `contract`, `precedents[]`,
`omissions[]` and `truncated`. Each precedent has `id`, `question`,
`options[]` (`id`, `label`), `chosen_option`, `context`, `scope`
(`workspace_id`, `category`, `topic`), `decided_at`, `decided_by`,
`decided_by_kind`, `reversed` and `citation`. Office adds no field.

**It never returns an answer.** It returns no option, score, probability,
recommendation or summary of the precedents. It is model-free: no model
call. It reads open contradiction to-dos through `orbit.task.list` and
writes nothing.

### Candidates

From every readable ledger, in this order of exclusion:

1. `decided_at` strictly before `as_of`. Orbit refuses the whole response
   otherwise (Orbit §Host validation).
2. Not `private`.
3. Scope `global`, or `project:<name>` where the project maps to
   `workspace_id`. The mapping follows ingestion: project `<name>` maps to `ws_<name>`. Entries of other projects are excluded.
4. Not an ingested entry (`source: orbit-decision:<id>`), and not in a
   `same_as` group with one. Orbit's built-in provider already serves
   Orbit's own answered requests (Orbit §Sources), and Orbit merges sources
   by `citation.digest` only. Office cannot reproduce Orbit's digest, so it
   leaves those decisions to the built-in provider.
5. `decided_by` is `human`, or `model` with a `confirms` entry from a human
   (see [Feedback](#feedback)). `agent` entries are excluded: they are not
   human decisions, and Orbit refuses any other kind.
6. Not an entry that only records a review (`orbit.gate:
   model_answer_review`) or a confirmation (`confirms` set). Those are
   feedback marks, not decisions.

Rule 4 means a confirmed model answer reaches the consumer from Orbit, not
from Office, while the ledger keeps it for the Library's own readers. Rule
5 still holds for every entry, so if rule 4 is ever relaxed, an unconfirmed
model answer still cannot be returned.

### Relevance and recency

Each candidate scores:

- **Relevance:** 1 for same workspace (`context.workspace` equals
  `workspace_id`), 0.5 for `global`; plus the lexical overlap of the
  request's question and option labels with the entry's question, options,
  `chosen` and `because` (the Library index's FTS5 tokenizer and BM25 ranking, normalized to
  0..1 by dividing each matching row's positive BM25 magnitude by the
  largest magnitude in the eligible snapshot); plus 0.5 when the entry's `orbit.category` equals the request's
  category.
- **Recency:** the score is multiplied by `0.5^(age_days / half_life)`,
  where `age_days` is `as_of - decided_at` and `half_life` is the Library
  setting `scribe.precedents.half_life_days` (default 90, matching Orbit's
  `precedent_half_life_days` default; Orbit §Drift). Newer entries rank
  higher.

The provider builds a transient in-memory FTS5 index over only eligible
question, option, chosen and because text; ledger entries are not documents
in the persistent Library search index. Query tokens from the question and
option labels are quoted prefix terms joined by OR, so any lexical overlap
can match; no caller text is interpreted as FTS operators. This writes no
Library or plugin state.

Candidates with no lexical overlap are dropped. The top `k` by score are
returned, ties broken by newer `decided_at`, then by entry id. Each
`same_as` group counts once. A reversed entry stays in the ranking; see
below.

### Fields of a returned precedent

| Contract field | From the entry |
|---|---|
| `id` | The entry id, `dec-<scope>-<n>`. Stable forever. |
| `question` | `question`, cut to the contract's byte bound. |
| `options`, `chosen_option` | The option ids and labels per [Entries without options](#entries-without-options). |
| `context` | Fixed-order lines: the options note when any; `because` when stated; one conflict line per unresolved partner ([Contradictions](#contradictions)); then `task_type`, `risk_tier`; within the contract's context bound. A cut sets `truncated`. |
| `scope` | The querying `workspace_id` (including global evidence), `category` from `orbit` when present, else the request category; optional `topic` from `orbit`, omitted when absent. Orbit's v1 validator requires workspace and category strings, and rejects a different workspace. These fields bind retrieval scope rather than inventing ledger metadata. |
| `decided_at` | `decided_at`. |
| `decided_by`, `decided_by_kind` | `user` (Library settings currently carry no owner-name field); `human`, or `human_confirmed` for a confirmed model entry. |
| `reversed` | `true` when the entry or a `same_as` partner has a `reversed_by` available before `as_of`. |
| `citation` | `source: "office-decision-ledger"`, `ref: "<entry id> decisions-<scope>#L<n>"`, `digest: sha256:<hex>` of the entry's ledger line bytes. The line terminator is excluded; the original bytes are hashed without JSON reserialization. Lines never change, so the digest is stable. |

**Reversed entries.** An entry reversed by a later one is returned with
`reversed: true` when it ranks, and its reversing entry is pulled into the
result even if it ranks below `k` (it replaces the lowest-ranked entry).
The consumer then sees the old choice, the flag and the newer choice
together (Orbit §Drift). Reversal chains are followed within the same bound.
When the pair or chain cannot fit (including `k: 1`), newer reversal evidence
wins and `truncated` is true. A reversing entry must pass every candidate
exclusion, but needs no lexical overlap. An excluded/unreadable reversing
entry is never exposed: its old entry stays flagged and an omission names
the old entry's ledger. Confirmations, reversals and dedupe links from an
entry at or after `as_of` cannot influence historical retrieval.

**Omissions and truncation.** An unreadable or invalid ledger
(`decision_ledger_invalid`) is skipped and named in `omissions` with its
ledger id. A failed or incomplete contradiction to-do read is named
`conflicts_unread` ([Contradictions](#contradictions)). The response has at most 32 omission ids and 262,144 encoded UTF-8 JSON
bytes. Question, option labels and context are cut on UTF-8 boundaries to
8,192 bytes; recovered options beyond the contract's eight-option bound
use the single-option representation with their text in context. Every
cut, rows dropped for the byte bound, or omission ids beyond 32 set
`truncated: true`. Ordinary top-`k` ranking alone does not mark truncation.
Entry privacy, private ledger front matter and private `decisions` or
`conversation` sensitivity settings all withhold text. Orbit
treats either as a provider gap that cannot produce a `suggested` result
(Orbit §Packet), which is the safe direction.

## Feedback

Orbit records every model answer with `answered_by: model`, raises a
`model_answer_review` request with answers `confirm` and `veto`, and lets a
human answer the original request again after a veto (Orbit §Recording,
veto and feedback). Ingestion writes each of those into the ledger:

| Orbit event | Ledger entry | Precedent? |
|---|---|---|
| A request answered by the `model` actor | `decided_by: model`, with `orbit.model` (the model citation). | No, unless confirmed. |
| `confirm` on its `model_answer_review` | `decided_by: human`, `confirms: <model entry>`, question and options of the review gate. | The model entry becomes precedent as `human_confirmed`; the confirm entry itself is a feedback mark, never returned. |
| `veto` on its `model_answer_review` | `decided_by: human`, `reverses: <model entry>`, question and options of the review gate. | A feedback mark, never returned. The model entry now has `reversed_by`. |
| A human's answer to the re-opened request | An ordinary ingested entry with `reverses: <model entry>`. | Yes: the human's override is precedent (served by Orbit's built-in provider, rule 4 above). |
| An expired review | Nothing. | The model entry stays unconfirmed and is never precedent. |

The rules, as invariants every reader keeps:

- **Model answers are excluded unless a human confirms them.** A
  `decided_by: model` entry is never precedent while it has no `confirms`
  entry from a human. With one, it is precedent with `decided_by_kind:
  human_confirmed`, and a later veto or reversal still flags it `reversed`.
  This stops the model learning from itself (Orbit §Feedback rules).
- **An override is a `reverses` entry, and it is precedent.** Whether it
  comes from an Orbit re-answer, an Orbit veto, a `#decision` marker that
  reverses an earlier entry, or `office.capture` with `reverses`, it is a
  human decision like any other. Only feedback marks (a review-gate
  `confirm` or `veto` entry) are not returned.
- **Only ingestion writes `decided_by: model` and `confirms`.** The scribe
  and `office.capture` keep refusing `decided_by: model`, and refuse
  `confirms`.

## Contradictions

Built in P4 ([ORB-21219]).

The weekly `office-scribe-contradictions` routine files one `owner:you`
to-do for two decisions that conflict with no `reverses` link
([scribe.md](./scribe.md#contradiction-checks)). It also compares current
decisions within one Orbit category, even when the question text differs:
an ingested entry's category is its `orbit.category`, and a scribe entry's
category is its `same_as` partner's `orbit.category`. Conflicting answers
to the same kind of request are the case the consumer most needs to know
about. Each decision-conflict to-do's description ends with the last line
`Conflicting entries: <entry id> <entry id>` (the two ids, one space
between them, lexicographic order, nothing else on that line).

Meeting extraction files the same to-do, with the same tags and last
line, when a meeting decision contradicts a prior entry. The provider
reads those to-dos alike. `office.decision-resolve` settles either kind
in one call: a `reverses` entry for the losing decision, then the to-do
closed ([scribe.md](./scribe.md#meeting-decisions), [ORB-21340]).

The provider reports a conflict that is still unresolved. It reads open
to-dos with `orbit.task.list` (tags `owner:you` and
`office-scribe-contradictions`, both required; status `proposed`,
`backlog`, `in-progress`, `review`, `blocked` or `someday`) and keeps a
to-do whose last non-empty description line is that `Conflicting entries`
line. The conflict is unresolved when both entries are eligible evidence
before `as_of` and neither has been reversed in that snapshot (a
`reversed_by`, including through a `same_as` partner, means a later
decision already stands). A partner that fails a candidate exclusion is
not named and is not returned.

For a returned entry with an unresolved conflict:

- its partner is pulled into the result, like a reversing entry: no
  lexical overlap is required, and the partner counts against `k`. When
  the pair cannot fit, the ranked entry and its reversal evidence stay,
  the partner is dropped, and `truncated` is true;
- each returned entry's `context` carries one line per unresolved partner,
  `Unresolved conflict with <partner id>; the user has not said which stands.`,
  after `because` (when stated) and before `task_type` and `risk_tier`.
  The line stays when the partner was dropped for lack of room.

`orbit.precedent-provider/1` has no conflict field, so this is how the
consumer learns of it: two records disagreeing within the request's reach,
which the model's `precedents_conflict` predicate is asked to see, and the
context line. A deterministic downgrade from a provider-reported conflict
needs an additive field in Orbit's contract (see
[Open questions](#open-questions)). A failed read, or a list whose
`truncated` is not false or whose `total` disagrees with the returned
rows, is named in `omissions` (`conflicts_unread`) and is never ignored.
Rows that did arrive are still applied. The token is kept when omission
ids are capped at 32.

## Agreement view

A read-only **Agreement** view in the Scribe screen's Decisions panel
([web-app.md](./web-app.md), [scribe.md](./scribe.md#feed)): per
category, how often the model agreed with the human.

- **Data.** Orbit's per-category readout `precedent-decision-readout/1`
  (Orbit §Per-category report), produced by the `decision-assistance-eval/4`
  evaluator (Orbit slice ORB-21096). Office reads it; it computes nothing
  and stores nothing. The app server reads it through one Orbit client
  read function (`orbit.decision.readout` with
  `{contract: "precedent-decision-readout/1"}`) behind
  `GET /api/scribe/decision-agreement`, at most every 60 seconds (the
  server caches a successful or hidden read for 60 seconds, and the
  screen throttles its own agreement fetch the same way). While Orbit
  offers no such tool, or the tool reports no readout, that read reports
  "no readout" and the view stays hidden; wiring to a differently named
  live Orbit surface later changes only that one read function.
  The readout's shape is pinned here (derived from the `/4` per-category
  score output; a JSON fixture of this shape lives at
  `app/packages/server/test/decision-agreement-fixture.json`):

  ```json
  {
    "contract": "precedent-decision-readout/1",
    "consumer_mode": "shadow | enabled | off",
    "generated_at": "<RFC 3339 time the readout was produced>",
    "window": { "start": "<RFC 3339>", "end": "<RFC 3339>" },
    "min_samples": 30,
    "categories": [{
      "category": "<workspace>:<kind>, e.g. ws_orbit-office:agent_request",
      "workspace": "<workspace id or null>",
      "kind": "<request kind, e.g. agent_request>",
      "mode": "shadow | enabled",
      "scored_requests": 42,
      "accuracy": 0.81, "accuracy_wilson_low": 0.68, "accuracy_wilson_high": 0.90,
      "coverage": 0.74, "calibration_error": 0.04,
      "abstention_rate": 0.12, "conflict_rate": 0.05,
      "min_samples": 30
    }]
  }
  ```

  Rates and accuracy are fractions in `0..1`; the Wilson fields are the
  95% interval bounds for `accuracy`. A category's `min_samples` overrides
  the readout's. The server forwards only these aggregate fields: any
  per-request field a readout carried (a prediction, a probability for
  one request, a request id) is dropped and never reaches the browser.
- **Rows.** One per category the readout names: the category in words
  (workspace and kind), its mode (shadow or enabled, from Orbit), scored
  requests, accuracy at the threshold with its Wilson 95% interval,
  coverage, calibration error, the abstention and conflict rates, and the
  readout's window and time. Sample sizes below the registered
  `min_samples` show "Not enough decisions yet" instead of a percentage.
- **Never per-request predictions.** The view shows aggregates only, never
  what the model predicted for any request, so it cannot contaminate a
  category under measurement (Orbit §Concerns, "Shadow requires hiding").
- **No actions.** No enable, disable, confirm or veto. Those stay in
  Orbit's own surfaces under its operator rules.
- **States.** Hidden when the consumer is off or no readout exists (the
  Decisions panel then shows only its ledger entries). A readout with no
  categories shows "No decisions yet." A category below `min_samples`
  shows "Not enough decisions yet" instead of its percentages.
  "Could not read Orbit's measurement" when the read fails (the route
  answers `502 {error: "agreement_unavailable"}`). Laid out from 320 px
  like the rest of the Scribe screen.

## Privacy

- A `private` entry is never a precedent: the packet leaves the host under
  Orbit's data-boundary grant (Orbit §Packet), and a reference-only entry
  has nothing that may leave.
- Entries are Daniel's own words. Orbit's enablement names precedent text
  as sent evidence (Orbit §Packet); this tool only returns what Orbit asks
  for, within the contract's bounds.
- The provider and the agreement view read; neither writes the Library.

## Configuration

One Library setting, under the existing `scribe:` block
([scribe.md](./scribe.md#configuration)):

- `scribe.precedents.half_life_days`: integer, 1..3650, default 90.

Orbit holds the rest (provider list, `k`, the consumer's mode, its
policy) in its host-only configuration. The `office-decision-ingest`
routine ships disabled.

## Open questions

- **Model answer shapes.** Orbit's `model` actor, its citation and the
  `model_answer_review` binding are proposed (human-decision-channel
  §11.2–11.3). Ingestion reads `resolution.model` and
  `binding.request_id`; if Orbit names them differently, only
  `decision_ingest.rs` changes.
- **Provider-reported conflicts.** A deterministic downgrade from an
  Office-detected conflict needs an additive `conflicts` field in
  `orbit.precedent-provider/1` (or a `/2`). P4 reports conflicts in
  context as described above until such a field is available.
- **Readout surface.** The agreement view depends on the read surface the
  evaluator slice (ORB-21096) gives the readout.

## Build order

Each slice is a proposed task in ws_orbit-office. Each updates the docs its
change makes stale in the same change.

| Slice | Task | Depends on | Delivers |
|---|---|---|---|
| P1 | [ORB-21216] | Scribe v2 S1, S4 | Entry schema additions (option `id`/`consequence`, `orbit`, `same_as`, `confirms`), their validation and refusals, `same_as` matching in `office.scribe-append` and `office.capture`, and reading them through `office.source-show` and `office.scribe-query`. |
| P2 (built) | [ORB-21217] | P1; Orbit's `include_closed` read | `office.decision-ingest` and the disabled `office-decision-ingest` routine: request mapping, idempotence, dedupe, and the feedback entries (model, confirm, veto, re-answer). |
| P3 (built) | [ORB-21218] | P1; Orbit [ORB-21093] for conformance | `office.decision-precedents`: the contract input and output, candidates, relevance and recency, single-option records, reversed and pulled-in entries, omissions, citations, and the `half_life_days` setting. |
| P4 (built) | [ORB-21219] | P3 | Contradictions: the routine's category comparison and machine-readable line, and the provider's conflict reporting. |
| P5 | [ORB-21220] | Scribe v2 S7 ([ORB-21120]); Orbit [ORB-21096]'s readout surface | The Agreement view and `GET /api/scribe/decision-agreement`. |

## Task References

- [ORB-21090] — designed this spec.
- [ORB-21089] — designed Orbit's decision model and its provider contract (Orbit repository).
- [ORB-21083] — designed scribe v2 and the D13 ledger schema.
- [ORB-20840] — the orbit-office on-call session where Daniel asked for the decision model.
- [ORB-21120] — proposed scribe v2 slice S7, the Scribe screen's Decisions panel.
- [ORB-21093] — proposes Orbit's precedent-provider contract and built-in provider (Orbit repository).
- [ORB-21096] — proposes Orbit's `decision-assistance-eval/4` evaluator and its readout (Orbit repository).
- [ORB-21216] — proposes slice P1: ledger schema additions and dedupe links.
- [ORB-21217] — built slice P2: Orbit decision ingestion and feedback entries.
- [ORB-21218] — built slice P3: the `office.decision-precedents` provider.
- [ORB-21219] — built slice P4: contradiction reporting.
- [ORB-21220] — proposes slice P5: the Agreement view.
- [ORB-21340] — built meeting-fed decisions, their contradiction check and `office.decision-resolve`.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
