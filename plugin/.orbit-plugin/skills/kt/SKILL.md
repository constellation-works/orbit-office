---
name: office-kt
description: Explain a processed KT video and answer questions about it from its transcript, frame readings and chapters, citing timestamps; never re-process without a new confirmed plan.
---

# KT videos

KT videos are processed by the `office-kt-inbox` routine, not by an agent.
Every 15 minutes (seeded disabled) the routine lists confirmed, unprocessed
plans with `office.video-plan` in list mode and, for each plan, runs
deterministic `plugin.tool_call` steps in order: `office.transcribe`,
`office.frames`, then `office.extract` with `profile: kt`. The knowledge
candidates that extraction writes as the source's digest go through
`office.consolidate` (`batch_id: kt`), which files one proposal per page;
the routine files no proposal itself. Each question extraction finds becomes
one `owner:you` to-do tagged `kt-question` and `source:<source-id>`, naming
the person to ask and citing the moment. No step calls an outward-facing
action, and no step makes more model calls than the confirmed plan counted.

## Explain a processed video

Read the source with `office.source-show` and its chapters, transcript and
frame readings. Explain, in plain words:

- the chaptered index: each chapter's start and end, its title, and whether
  it is labelled `core` or `tangent` — tangents are folded away, not deleted;
- the gotchas lifted out of tangents and passing warnings;
- what consolidation proposed from the digest (pages and runbooks awaiting
  the user's approval) and which questions are waiting as `kt-question`
  to-dos.

Cite every claim. A transcript claim cites `<source-id>#t=hh:mm:ss`; what is
on screen cites `<source-id>#frame=hh:mm:ss`; a chapter cites
`<source-id>#chapter=<n>`. Follow a knowledge page or proposal back to the
timestamp it cites before repeating it.

## Answer questions about the video

Search the indexed transcript, frame readings and chapters with
`office.search`, then inspect the evidence with `office.source-show` (and
`office.knowledge-show` for a page built from it). Answer only what the
video supports, with `#t=` citations for what was said and `#frame=`
citations for what was shown. If the video does not cover the question, say
so; do not fill the gap from general knowledge without labelling it as such.

## Never re-process without a new confirmed plan

Processing is spent only against a plan the user saw and confirmed. Do not
call `office.transcribe`, `office.frames`, `office.extract` or
`office.consolidate` yourself to re-run, retry or top up a video, and do not
work around a `plan_exceeded`, `plan_changed` or `plan_unconfirmed` refusal.
Tell the user what a new preview would cover, and let the app show the new
plan through `office.video-plan` for confirmation; the routine then picks
the newly confirmed plan up on its next tick. A video whose extraction has
already run is not listed again, so a routine tick alone never re-processes
it.
