---
type: design
summary: "Spec: Library bootstrap — a new role's onboarding material becomes a curated Library in four phases: cheap digest per source, careful consolidation per topic, batch cost preview, setup progress, page rules with mechanical checks, and runbooks the agent performs as drafts"
last_validated: 2026-10-27
---

# Spec: Library bootstrap

Someone starting a new role gets a pile of material: onboarding pages, design
docs, KT videos, team decks, and the company's processes and policies. The
bootstrap turns that pile into a Library the agent can answer from and act on,
in an afternoon instead of a month, without filling it with a page per
document ([decision](../4_decisions.md#a-new-roles-material-is-bootstrapped-in-phases-digest-per-source-consolidate-per-topic)).

**The rule that keeps it clean:** sources keep everything, and pages keep only
what is worth maintaining. Every file is captured and searchable as a source,
so the agent can answer from any of them with citations, even when no page
exists. Pages are for knowledge that will be used again.

## Phases

Run in order, one batch per phase. Each phase is a tag on the sources it
captures (`bootstrap:<phase>`):

| Phase | Material | Typical result |
|---|---|---|
| `docs` | onboarding pages, Confluence design docs, project briefs | `project`, `topic` and `systems` pages, `person` pages for owners, glossary |
| `kt` | KT videos ([kt-videos.md](./kt-videos.md)) | runbooks, `topic` and `systems` updates, gotchas, questions for presenters |
| `team` | team docs, presentations, reports | updates to the pages above, artifact locations as `refs` |
| `process` | company processes (request an AD group, request PTO) and policies | runbooks, policy summaries on `topic` pages that link to the official policy |

The orchestrator leads the bootstrap phase by phase. It runs after the
onboarding interview ([knowledge-templates.md](./knowledge-templates.md#onboarding-interview)),
because the interview's `user`, `project` and `person` pages give the
consolidation pass names to attach claims to. For each phase it asks for the
material, captures it, shows the batch preview, and walks the user through
the proposals by subject. A phase can be repeated when more material turns
up; a repeated phase only adds and updates.

**Getting the material in.** For the event: export folders (Confluence space
exports in HTML or PDF, Word, PowerPoint, PDF) dropped into the app. The app
captures each file as one source. Fetching Confluence pages through a gateway
connection ([gateway.md](./gateway.md)) comes after the event.

## Document capture

`office.capture` converts each supported file to anchorable text:

| Format | Conversion | Anchors |
|---|---|---|
| HTML (a Confluence export) | Headings, paragraphs, lists and tables kept as Markdown; scripts, styles and navigation dropped | `#L<n>` |
| `.docx` | The document XML read from the zip, with headings and lists kept | `#L<n>` |
| `.pptx` | One section per slide: title, body text, speaker notes | `#slide=<n>` |
| `.pdf` | `pdftotext -layout` (poppler) per page | `#page=<n>`, `#L<n>` |
| `.md`, `.txt` | as is | `#L<n>` |

`pdftotext` is an optional program. Without it, a PDF is refused with
`converter_unavailable`, naming the program. A file whose conversion yields
no text, such as a scanned PDF, is captured with a `needs_ocr` note and left
out of the batch.

The built converter accepts UTF-8 HTML and document XML. Office packages are
read in memory, never extracted. Unsafe or duplicate member paths, malformed
XML (including DTDs), missing required parts and missing referenced slides or
notes are refused as `document_invalid` before a source is written. Bounds are
32 MiB per document and expanded package, 4,096 ZIP entries, 8 MiB per XML part
and converted body, and 128 XML levels / 100,000 elements. An exceeded bound is
`conversion_limit`.

PDF conversion runs `pdftotext -layout -enc UTF-8 <pinned-copy> -` with a cleared
environment containing only PATH, a temporary HOME/TMPDIR and `LC_ALL=C`.
The shared supervisor gives it 30 seconds and 512 KiB per output stream,
terminates and reaps its process group, and suppresses raw diagnostics in errors.
Nonzero exits are `conversion_failed`; timeout and output overflow are
`conversion_timeout` and `conversion_output_limit`. Poppler's form feeds delimit
pages, rendered as `# Page <n>`. Slides follow the presentation relationship order,
with `# Slide <n>` followed by the title, body and related speaker notes.
`office.source-show` returns that section, including its nested headings, for a
page or slide anchor. Markdown and text uploads retain their text.

Capture accepts up to 64 `tags`, each a nonempty string of at most 128 UTF-8 bytes
with no control characters. Tags are stored in source frontmatter; use
`office.search` with `tag: "bootstrap:<phase>"` and a nonempty query to filter
them. Capturing duplicate content merges new tags into the existing source;
conversation appends merge tags while preserving the earlier body.

## Stage 1: a digest per source (cheap)

`office.extract` with `profile: digest`: one fixed call per source, with the
standard model and no agent. It writes the source's **digest** as
`digest.json` beside `source.md`:

```json
{
  "summary": "Design of the ingest service's v2 pipeline …",
  "kind_hint": "design-doc",
  "as_of": "2025-11-03",
  "subjects": [
    { "name": "Ingest service", "template": "topic", "aliases": ["ingest", "ingest-v2"] },
    { "name": "Priya Raman", "template": "person", "role": "ingest owner" }
  ],
  "claims":     [{ "subject": "Ingest service", "text": "…", "cites": ["#L40-44"], "volatile": false }],
  "procedures": [{ "title": "Rotate the ingest keys", "inputs": [...], "steps": [{ "text": "…", "cites": ["#L88"] }], "done_when": "…" }],
  "terms":      [{ "term": "DLQ", "definition": "…", "cites": ["#L12"] }],
  "refs":       [{ "label": "Ingest dashboard", "url": "https://…" }]
}
```

- **Every claim, step and term cites an anchor in that source.** Uncited
  items and items with dangling citations are dropped and counted, as in
  the KT extraction. Line anchors count from the source body, after frontmatter;
  page, slide and timestamp anchors must resolve in that body. Empty procedures
  are dropped after filtering their steps.
- A source window holds at most 48,000 characters of annotated text, including
  absolute line labels. Oversized lines split without changing their line id or
  breaking UTF-8. Short sources use one standard-tier extraction; longer sources
  use one call per window and one merge. The result reports `calls` and
  `dropped: {uncited, dangling, tangent}`. Filtered window results are retained as
  `extract-window-NNNN.json`, including when a later call fails.
- Bootstrap-tagged sources require a confirmed `office.batch-plan` allowance.
  Every window and merge consumes its slot before inference; a partially spent
  group requires a new preview and confirmation before another attempt. The
  preview prices all windows and the merge. Untagged sources can be extracted
  directly. Videos use the confirmed `kt` profile.
- A digest extraction needs no Orbit task. Supplying `task` (or
  `context.task_id`) also attaches the result to that task. It creates no to-dos
  or knowledge proposals. `digest.json` is folded into the source's search row
  on writes and reindex, retaining the source id and path. Source Markdown
  remains the evidence; extraction provenance does not invalidate the remaining
  approved document allowances.
- `volatile: true` marks a time-bound fact (a sprint, a status, a date that
  will change). Volatile facts never become page claims. They stay
  searchable in the source.
- A KT video's extraction ([kt-videos.md](./kt-videos.md#3-process-the-office-kt-inbox-routine))
  writes the same digest shape, so its knowledge goes through the
  consolidation pass like any document. Its chapters and questions stay as
  they are.

## Stage 2: consolidate per topic (careful)

`office.consolidate` (mutating, `mcp_scope: none`) runs over every digest in
a confirmed batch:

1. **Group** subjects across digests, deterministically: normalized names and
   aliases, matched against existing pages' titles and `aliases`. A subject
   that matches an existing page joins that page's group.
2. **Apply the page rules** (below) to decide which groups may produce a
   page.
3. **One careful call per group** that passes. The `careful` model tier,
   Muse Wonder by default (`models.careful` in Library settings), gets the
   group's claims, procedures and terms with their citations, plus the
   current page if there is one. It returns one page body built from the
   subject's template, with conflicts flagged: the newer source wins, both
   are cited, and the older claim is noted as superseded.
4. **Propose.** A `knowledge_add` or `knowledge_update` per page, runbooks
   separately, tagged `bootstrap:<phase>` and grouped by subject in the app
   ([proposals.md](./proposals.md)).
5. **Report** what was proposed, merged, below the bar, deferred by the
   budget, and in conflict. This goes in the step result and in the setup
   progress.

## Page rules

Written for agents in the `library-conventions` skill and given to the
consolidation call. Checked mechanically wherever a rule can be checked:

| Rule | Mechanical check |
|---|---|
| **One subject per page.** A page is about one person, project, topic, system or procedure, named by a template. Never one page per document. | Consolidation only creates pages from subject groups, and a page's `template` is required for bootstrap proposals. |
| **The bar for a new About page.** The subject appears in at least two sources, or is a one-per-Library template, or the user asked for it. Anything else stays in sources. | `consolidate` counts distinct sources per group, and a group below the bar is reported as `below_bar` with no proposal. |
| **The bar for a runbook.** A repeatable procedure with inputs, at least two cited steps and a "done when", that someone will do again. | Procedures missing a field or citations are folded into their topic page as a short section, never proposed as a runbook. |
| **Update before you create.** | A candidate whose title or an alias matches an existing page (same normalized alias, or a token overlap of at least 0.6) becomes a `knowledge_update` of that page. |
| **Short pages.** About 800 words; split by subtopic past that. | `office.knowledge-write` refuses a page over 1,200 words with `page_too_long`, and consolidation splits before that. |
| **Policies are summaries with a source.** What matters for doing the work, with a link to the official policy and its date. No copying whole policies, and at most two quoted sentences. | A page or section tagged `policy` without an `authoritative` ref and an `as_of` is refused with `policy_without_source`. |
| **No volatile facts on pages.** | Claims marked `volatile` are never passed to the careful call. |
| **No secrets, and no personal data beyond work contacts.** | The redaction and contact rules of [knowledge-templates.md](./knowledge-templates.md#contact-details) apply. |
| **Don't flood the queue.** | A batch proposes at most one new page per three sources, rounded up; updates do not count. The overflow is deferred to the next run of the phase and reported. |

## Batch preview and confirmation

`office.batch-plan` (mutating, `mcp_scope: none`) works like the video plan
([kt-videos.md](./kt-videos.md#2-plan-and-confirm-officevideo-plan)), for a
whole phase:

- Counts each document's standard digest windows and merge, each video's own planned
  calls, and an estimate of careful calls (one per subject group expected
  from earlier phases and the files' headings, refined after stage 1).
- Prices them from `costs.*`, or shows the counts with rates unset, and shows the route per call group. A batch marked private at the preview routes every call to the private endpoint ([models-and-privacy.md](./models-and-privacy.md#what-private-changes)).
- Confirms with the digest the user saw (`plan_changed` if it moved;
  STD-05@1 §R21).
- No step makes more calls than the plan allows. Careful calls past the
  estimate need a second, smaller confirmation: "8 more topics than
  expected: about N more calls".

The `office-bootstrap` routine (every 15 minutes, seeded disabled) runs a
confirmed batch: the digests (and the KT steps for videos), then
`office.consolidate`.

## Setup progress

`office.bootstrap-status` (read_only, `workspace`) reports per phase: sources
captured, digested, in the consolidation pass, pages proposed, accepted and
dismissed, the overflow waiting, and **gaps**. Gaps are computed, not
guessed:

- a subject cited by three or more sources with no page;
- a procedure seen in a `process` source with no accepted runbook (for
  example "PTO policy captured, no runbook for requesting PTO");
- a `person` named as an owner with no page;
- a policy summarized without a current `as_of`.

The app shows this as **Library setup**: four phases with their progress and
gaps, each gap with one action (add material, or ask the agent).

## Runbooks the agent performs as drafts

A runbook may say that the agent can carry it out:

```yaml
template: runbook
performable: agent          # agent | you (default)
submission:
  system: ServiceNow        # where the request goes
  via: draft                # draft | gateway
  connection: servicenow    # gateway connection, when via: gateway
```

When the user says "request the ingest AD group for me", the orchestrator
files an `owner:agent` to-do with that runbook attached. The agent follows
every step up to submission and produces what is needed: the ticket text,
the form and its fields, the group and the justification, each citing the
runbook.

- **`via: draft`** produces a `draft` artifact with the filled fields and a
  link to the form. The user submits it, and Send records `sent_at`
  ([library.md](./library.md#artifacts)).
- **`via: gateway`** produces a `gateway.http-draft` for that connection.
  Approving it sends it once ([gateway.md](./gateway.md#writes-draft-approve-send)).
- Submission is always outward-facing, so it always waits for the user.
- **Out of scope for the event:** filling a portal through a browser MCP.
  SSO and company portals make it fragile, and it would need its own
  approval gate at the final submit.

## Day-1 checks

- Whether Muse Wonder (or another stronger model) is on the Meta Model API
  for the `careful` tier, and its context window.
- The size of a typical Confluence export, to set the digest call's input
  cap and the windowing for long documents.

## Built tool contracts

With no input at all, `office.batch-plan` is in list mode for routine
fan-out: it returns `{batches: [{batch_id, phase}]}`, in phase order and
capped at 256 entries. `batch_id` is the ledger key (one batch per phase).
A batch is listed only when it is confirmed and consolidation has not
started for it: no `consolidating` or `overflow` progress has been recorded
and no careful call (original or additional) has been consumed. It is
empty when no batch qualifies, and list mode writes nothing. Any other key
without `phase` is refused as `invalid_argument`. The same pending list is
returned as `pending_batches` by `office.bootstrap-status`.

`office.batch-plan` otherwise accepts `phase` (`docs`, `kt`, `team`, `process`), optional
`sensitivity: private`, and `confirm: true` with the preview's `digest`.
Preview and confirmation make no inference requests. Video metadata and frame
candidates are inspected locally through the same planner as `office.video-plan`;
its exact per-video previews are retained in `plan.videos`. A document with
`needs_ocr` metadata is counted as captured but excluded from the batch.

The response is the durable phase ledger: `plan`, `confirmed_at`, per-group
`used` call counts, `extra`, `extra_confirmed_at`, `extra_used`, `consolidating`
and `overflow`. `plan.counts` separates `digest`, `audio`, `frames`, `extract`
and estimated `careful` calls. Each entry of `plan.groups` names its source
(or null for consolidation), count and destination's route, label and model.
Private marking updates every pending source in the batch in the same
recoverable transaction as the preview. It never downgrades existing private
sources. A mixed batch routes its careful consolidation to the private route
when any input is private, including earlier-phase digest inputs.

Careful estimates use normalized heading subjects and earlier-phase digest
names/aliases, merging intersecting alias groups transitively, with one call as
the minimum for a nonempty batch. Text estimates use four characters per token,
a document output allowance of 1,024 tokens, and 2,048 tokens per careful
call. Video estimates use the shared video planner's audio, image and timeline
units. Prices use `costs.text_1k_tokens`, `costs.audio_minute`, `costs.image` and
`costs.currency`; when any applicable rate is missing, `estimate` is null and
`rates_unset` names the missing keys. These are estimates, never a billed price.

After stage one, call with `additional_careful: true` to preview only the
additional subject groups beyond the initial careful estimate. Confirm that
smaller preview with `additional_careful: true`, `confirm: true` and its own
`extra.digest`. Missing source digests refuse with `digests_incomplete`;
unconfirmed batches refuse with `plan_unconfirmed`. Changed source content,
phase membership, settings, routes or preview evidence refuse confirmation
with `plan_changed`. Repeating an unchanged confirmation preserves its original
timestamp and writes nothing.

Execution integrations use `Library::mutate_batch` to reserve a call before
inference. Each original group and the separately confirmed additional careful
allowance has a hard ceiling; an uncertain/failed request still consumes its
slot. Additional previews derive counts from stored digests, never caller-supplied
numbers. The consolidation runner records active subject ids and subjects
waiting under the page cap with the port's `Progress` mutation. Building that
runner remains the responsibility of `office.consolidate`.

`office.bootstrap-status` accepts an empty object and returns all four phases in
order, plus `pending_batches`: the confirmed, unconsolidated batches
described under `office.batch-plan` list mode, as `[{batch_id, phase}]`. It reads complete source files, validated `digest.json` sidecars and
knowledge pages, plus the complete projected Orbit task list. It refuses a
truncated task list rather than reporting partial counts. Knowledge proposals
tagged `bootstrap:<phase>`, `proposal`, and `proposal:knowledge_add` or
`proposal:knowledge_update` count as proposed until decided, accepted at `done`,
and dismissed at `rejected`. Consolidating and overflow counts come from the
phase ledger, with zero before a runner has recorded any work.

Gap kinds are `subject_without_page`, `procedure_without_runbook`,
`owner_without_person` and `policy_without_current_as_of`, each with a
normalized subject and sorted distinct supporting source ids. Subject evidence
is counted per distinct source in the phase. Titles and aliases match pages;
procedures require an accepted page under `knowledge/runbooks/` or using the
runbook template, and an owner requires the person template. An owner is a
person subject whose role contains the word `owner`. A policy digest is
identified by `kind_hint` containing the word `policy`; maintained policy pages
use the `policy` tag. A current `as_of` is a valid date or timestamp, no later
than now, no older than `thresholds.stale_days`, and (for pages) at least as new
as the latest cited digest's date. Status never creates a ledger, index or
layout, and refuses an abandoned write pending recovery by a mutating operation.

`office.consolidate` accepts `{batch_id: "docs"}` (the phase ledger key from
`office.batch-plan` list mode). Optional `requested: ["Subject name"]` explicitly
requests those subjects and bypasses the new About page's source-count bar. It
reads stored digests; caller-supplied digests or page targets are not accepted.
Every source in the confirmed plan must have a digest. Earlier phases enrich
matching subjects, but never create an unrelated candidate in this batch.

Names use lowercase alphanumeric tokens with punctuation replaced by spaces.
Aliases merge transitively. Existing page titles and aliases bridge groups;
exact matches take precedence over Jaccard token overlap (intersection divided
by union, at least 0.6). Page ids break ties deterministically. Runbooks and
About pages have separate identities. An ambiguous alias component spanning two
existing pages refuses with `ambiguous_subject`. One-per-Library templates
combine their subjects and update their existing singleton when present.

A complete digest procedure is treated as repeatable: nonempty inputs, at least
two steps with resolvable source citations, and a nonempty `done_when`. It forms
a separate runbook candidate. An incomplete procedure belongs to its matching
About subject, otherwise the digest's first topic in normalized order (a topic
candidate is introduced if none exists, still subject to the source bar). An
incomplete procedure labeled as a runbook subject is folded the same way. Terms
belong to a glossary subject when present, otherwise the first topic. The digest
format has no explicit subject field for procedures or terms. Uncited or dangling
claims, steps and terms are excluded again before inference. Volatile claims are
excluded entirely. Current pages use their contact-withholding model view.

The closed careful reply is `{body, conflicts: [{current: {text, cites},
superseded: {text, cites}}]}`. Each conflict must identify exact supplied claims
from different sources with valid dates; current must be strictly newer. Both
claims and citations are materialized in a conflict section. Invalid replies,
pages missing required template sections, invalid citations and pages exceeding
the word cap are reported as failed subjects without filing their proposals.
The model never selects page ids, templates or update revisions.

The report contains `batch_id`, `new_page_budget`, `proposed`, `merged`,
`below_bar`, `deferred`, `conflicts`, `failed` and `suppressed`. Proposal entries
are `{subject, page, proposal}`; `merged` is the subset updating an existing
page. `below_bar` and `deferred` list normalized subject keys. Failures contain
`{subject, code}`. A recently dismissed equivalent proposal is `suppressed` by
`office.proposal-create`. Proposals retain `bootstrap:<phase>` tags and require
normal proposal acceptance; consolidation never writes an accepted page.

Consolidation claims the batch with a compare-and-swap progress write, reserves
each careful call before inference, and makes no automatic retries. All needed
original/additional allowances must already be confirmed. A started batch
refuses another invocation with `batch_started`, including after an uncertain
failure; inspect its proposals and failure progress before planning again.
Successful subjects leave `consolidating`; failures remain visible, while
page-cap overflow stays in `overflow`. An additional phase preview carries
`plan.deferred_subjects` (omitted for ordinary plans) and reuses the phase's
stored digests without spending digest calls. After its fresh confirmation it
considers the deferred subjects and any newly captured sources, so earlier
proposals are not filed again. Each separately confirmed batch has its own
new-page budget. Changed carried digests invalidate that new approval.
