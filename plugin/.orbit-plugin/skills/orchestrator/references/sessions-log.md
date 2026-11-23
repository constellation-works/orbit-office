# Sessions: the conversation log

How to write the session's conversation source: what the extractor records, the entry kinds and when to write each.

## The extractor and the session source

The session-end extractor writes decisions, directions, facts, actions and
open questions. Do not call capture on every turn. It copies user wording,
scrubs credentials, treats unclear statements as open, and records the
session's `orchestrator`, exact `model` and `session_id` separately from
`captured_by: transcript-extractor`. Private evidence stays off the crew.

SCR files already in the Library (written by the scribe, the separate
`constellation-works/scribe` plugin) are read as handoffs: the
latest SCR per project and global is what the brief shows. Office never
writes them. Keep the rolling handoff yourself ([sessions-handoff.md](sessions-handoff.md)); the brief reads it
only while that scope has no SCR. With the host integration installed, when
the first handoff is needed, open the session source from its staged transcript
and keep the returned id:

```json
{"session_transcript_path":"<absolute-staged-transcript-path>","session_id":"<session-id>"}
```

Append handoffs to that id. The extractor later updates the same source,
preserving every handoff and anchor. It never generates handoffs or resolutions.

```json
{"append_to":"<source-id>","entries":[{"kind":"handoff","structured_handoff":{"objective":"Prepare the renewal deck for ORB-<id>.","next":"Draft the deck for the user's wording review.","authorization":"The orchestrator owns the draft; the user authorized drafting only.","evidence":["orbit://task/ORB-<id>"]}}]}
```

The plugin ships a disabled `office-conversation-extract` recovery auto-task.
Two prerequisites are separate from it:

- Manifest registration is a separate integrator change.
- Constellation must install the SessionEnd staging/queue hook beside
  `operations/scripts/session-brief.py`.

Until those prerequisites are installed, continue the existing manual log
(`kind: conversation`, `entries`, then `append_to`). Do not silently create an
extracted duplicate of a legacy manual source. Capture a fact immediately when
an onboarding or knowledge proposal needs a citation before session end.

## Entry kinds

Ordinary entries are `{kind, text}` (or the string `"<kind>: <text>"`);
new handoffs use `{kind:"handoff", structured_handoff:{...}}`
([sessions-handoff.md](sessions-handoff.md)). Capture adds
the timestamp. Each entry becomes one line, anchorable as `#L<n>`. The
capture response does not return line numbers: call `office.source-show`
with the source `id` to find them.

| Kind | Write it when | Rule |
|---|---|---|
| `decision` | The user explicitly decides | Extractor; no inferred consent |
| `direction` | The user gives a rule or instruction | **Verbatim** |
| `fact` | The user states something about themselves, their people or their work | **Verbatim**; onboarding pages cite these |
| `action` | The transcript reports something done | Extractor; reported, not independently verified |
| `open` | A question is left unanswered | One question per entry |
| `resolved` | An open question is answered | **Start the text with the open entry's anchor**, e.g. `#L4 — Acme stays on Pro.` |
| `handoff` | The plan changes | See [sessions-handoff.md](sessions-handoff.md) |

- A `resolved` entry closes an `open` only when its text begins with that
  `#L<n>` anchor, in the same source. A mention later in the line does not
  count.
- Pasted external content never goes in the log. Capture it as its own
  source first, then cite it in an `action` line.
- Keep entries to one line. Cite tasks and sources by id inside the text.
