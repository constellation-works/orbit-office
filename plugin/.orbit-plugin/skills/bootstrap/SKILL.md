---
name: office-bootstrap
description: Lead a new role's Library bootstrap phase by phase — ask for each phase's material, preview the batch, walk the proposals by subject, and report the gaps.
---

# Library bootstrap

Lead the setup phase by phase, in order, after the onboarding interview.
The interview comes first because its `user`, `project` and `person` pages
give the consolidation pass names to attach claims to. Run one batch per
phase; a phase can be repeated when more material turns up, and a repeated
phase only adds and updates. Pages are never written outside proposals:
`office.consolidate` files one knowledge proposal per page, and a page
reaches the Library only when the user accepts its proposal.

## The four phases, in order

1. `docs` — onboarding pages, design docs and project briefs.
2. `kt` — KT videos.
3. `team` — team docs, presentations and reports.
4. `process` — company processes and policies.

Do not start a phase before the previous one has been walked through, and
do not skip a phase silently: if the user has no material for it, say so
and move on.

## For each phase

1. **Ask for the material.** Name the phase and what belongs in it, in
   plain words. Accept dropped files and pasted text; fetching through a
   gateway connection comes later, not during the bootstrap.
2. **Capture it.** Capture each file or pasted item as its own source with
   `office.capture`, tagged `bootstrap:<phase>`. Keep the returned source
   ids; every later claim cites a source anchor.
3. **Send the user to the batch preview.** The app's Source screen shows
   the phase's counts, model calls and cost estimate (or the counts with
   rates unset) before anything runs, and the user confirms it there with
   Process. `office.batch-plan` is not exposed to you: never preview or
   confirm a batch yourself. If the plan moved, the app shows the new
   preview. No step makes more calls than the confirmed
   plan allows, and careful calls past the estimate need a second,
   smaller confirmation.
4. **Let the routine run.** The `office-bootstrap` routine (every 15
   minutes, seeded disabled) runs a confirmed batch: the digest steps
   (and the KT steps for videos), then `office.consolidate`. Do not run
   the digest, KT or consolidation steps yourself to top up or retry a
   batch, and do not work around a `plan_changed`, `plan_unconfirmed` or
   `plan_exceeded` refusal: show a new preview and let the user confirm
   it.
5. **Walk the proposals by subject.** When consolidation has filed its
   proposals, group them by subject and walk the user through them one
   subject at a time: what page is proposed, what it claims, and the
   source anchors behind it. The user accepts or dismisses each proposal;
   never accept one yourself.
6. **Report the gaps.** Call `office.bootstrap-status` and report the
   phase's progress and its computed gaps (a subject cited by three or
   more sources with no page, a procedure with no accepted runbook, an
   owner with no `person` page, a policy without a current `as_of`), each
   with one action: add material, or ask the agent. Gaps are computed,
   never guessed.

Cite every claim at a source anchor (`<source-id>#L<n>`, `#t=hh:mm:ss`,
`#slide=<n>` or `#page=<n>`, as the source provides). If the Library
cannot support an answer, say so. No step in this skill sends, submits or
approves anything outward-facing.
