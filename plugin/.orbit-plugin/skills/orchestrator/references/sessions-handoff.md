# Sessions: handoffs

How to write the rolling handoff, its structured fields and a global handoff, so an abrupt end loses nothing.

Append a new structured handoff whenever the plan changes: after a decision
that changes next steps, after filing or finishing work, and when the user
signals they are done. Keep the last note ready for an abrupt session end.
The newest note supersedes earlier ones in its thread; never edit them.
Do not write an end-of-session summary.

## Structured handoff contract

Write `{kind:"handoff", structured_handoff:{...}}` instead of `text`.
The old text keys remain accepted; existing one-line entries render unchanged
with `legacy:true` (and the brief labels them `(legacy)`). A structured entry
cannot also supply text, correction or by. `scope` and `until` work as before.

The brief renders these fields in this order:

| Field | Meaning | Budget (Unicode characters) |
|---|---|---|
| `objective` | Goal and complete assignment task id; required | 500 |
| `next` | Immediate action, acceptance checks, evidence location and blockers; required | 1000 |
| `authorization` | Owner of the next step, what Daniel already authorized, and constraints | 1000 |
| `observations` | Live facts as `{text, observed_at, recheck}` | 2000 total, including timestamps and rechecks |
| `evidence` | Specific task, comment, PR or report links/references | 2000 total |

Optional fields default to empty; the display says when none was recorded.
Do not infer consent or invent observation times. Every observation requires a
nonempty `text`, RFC 3339 `observed_at` with an offset, and nonempty `recheck`.
Keep completed work only when it affects the next decision; its full history
belongs on the assignment task. Use plain sentences and complete task ids,
with linked PRs instead of compressed numbers.

Capture refuses a handoff for any of these reasons:

- Unknown fields, credentials or control characters.
- Empty required fields (`handoff_required_field`).
- Incomplete task references (`handoff_task_id_incomplete`).
- Missing observation times (`handoff_observed_at_required`) or invalid times
  (`handoff_observed_at_invalid`).
- Any over-budget field (`handoff_budget_exceeded`).
- No assignment id in the objective (`handoff_assignment_required`).
- A malformed shape (`handoff_structure_invalid`).
- Bare numeric tokens of four or more digits, and bare numbers after
  task/tasks/assignment labels: they are ambiguous.

Use complete task ids, links, timestamps, versions or numbers with units for
other quantities. The validator cannot determine the semantics of every
number. All strings must be single-line text. The structured object is stored
as compact JSON after `(structured)` on one physical log line, retaining
`#L<n>` anchors; global and scribe markers precede it. The brief exposes
`structured_handoff` alongside the ordered rendered `text`, keeping timestamps
and evidence links.

Good example (replace angle-bracket placeholders before capture):

```json
{"append_to":"<source-id>","entries":[{"kind":"handoff","structured_handoff":{"objective":"Finish the repair for ORB-<id>.","next":"Run the acceptance checks on the repair and read its report before requesting publication. Waiting on the report.","authorization":"The orchestrator owns the repair. Daniel authorized edits and checks; publication requires his approval.","observations":[{"text":"Two workers were active.","observed_at":"2026-11-01T10:00:00Z","recheck":"Recheck worker state before dispatch."}],"evidence":["orbit://task/ORB-<id>","https://github.com/<owner>/<repo>/pull/<number>#issuecomment-<comment>"]}}]}
```

Bad example (bare task numbers and an observation with no timestamp):

```json
{"append_to":"<source-id>","entries":[{"kind":"handoff","structured_handoff":{"objective":"Finish the repair for ORB-<id>.","next":"Repair task 12345/67890 next.","observations":[{"text":"Workers active.","recheck":"Before dispatch."}]}}]}
```

This is refused, not saved as an opaque line. Restore the complete task ids
and record when the live fact was observed before resubmitting.

## Global handoffs

For something every project's orchestrator must know
(an Orbit binary to upgrade, a common Orbit defect and its workaround),
declare the scope explicitly; it is never inferred from the text:

```json
{"append_to":"<source-id>","entries":[{"kind":"handoff","text":"Orbit 0.9 drains need --resume after a host restart.","scope":"global","until":"2026-11-08T00:00:00Z"}]}
```

`until` (RFC 3339, after now) is optional. A global stays in every
project's brief until `until` passes or a later `resolved` entry cites it:
`{"kind":"resolved","text":"<source-id>#L<n> — fixed in Orbit 0.9.1."}`
(from any conversation; `#L<n>` alone works only in its own source). A
global never replaces your project handoff, and your project handoff never
replaces a global. Text starting with `(global)` without `scope` is refused.

## Handoffs from the extraction crew

When the transcript-extraction crew offers a handoff, it uses the same
structure. Invalid or legacy offers are dropped with a reason, retaining the
pass's other evidence. A valid crew note also needs a plan-changing entry
and valid local `#L<n>` citations; its `(scribe)` marker remains visible.
