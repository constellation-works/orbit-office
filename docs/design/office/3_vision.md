---
title: office — Vision
owner: daniel
last_updated: 2026-10-26
last_validated: 2026-10-26
status: Draft
feature: office
doc_role: vision
type: design
summary: Open questions and directions beyond the hackathon — routine packs, the business model, a human-owned task flag, and shared Library machinery.
tags: [office, plugin, orbit]
paths: ["crates/**", "plugin/**", "app/**"]
related_features: [office]
related_artifacts: [ORB-16585]
---

# office — Vision

Speculative. Everything here is a direction, not a commitment; the
must/should scope for the event is in
[1_overview.md](./1_overview.md#hackathon-scope-spec-3).

## Routine packs (pitch slide at the event)

Routine packs are data — routine and auto-task definitions, runbooks,
skills — not code paths, so new verticals need no plugin change:

- **Receipts** (should-have at the event): photos become a catalog; "when
  did I buy this?" is answered from it. Later: spending analytics,
  tax-filing help, dietary suggestions.
- **Bills:** utility and subscription bills are collected, anomalies
  reported, and a periodic research auto-task looks for cheaper or better
  alternatives.
- **Later still:** email triage into tasks; social media content creation;
  monthly groceries from fridge photos (stopping at a filled cart that
  waits for approval); lecture recording → homework tasks and notes; photo
  and video organizer; a daily diary from glasses video.

## Open questions (spec §17)

1. **Approvals:** reuse Orbit's decision records (`orbit.decision.*`,
   designed for deciding held work by email) for outward-action approvals,
   or keep them as `owner:you` to-dos? The to-dos are simpler for the
   event. Decision records may be the better long-term fit.
2. **Business model** for the startup-track pitch: a free local runtime
   with paid role-based routine packs (engineer, consultant, student, small office)
   and optional encrypted sync between devices.
3. **Relationship to a team knowledgebase:** the Library's ingest,
   provenance and freshness machinery could later be shared with a
   corporate-track team knowledgebase plugin.
4. **After the event:** a generic "human-owned" task flag in Orbit core.
   Such a task would never be admitted by a drain, which would let the
   office workspace run a normal drain and retire the dispatch-pass
   workaround in [2_design.md](./2_design.md#2-tasks-ownership-and-dispatch-spec-66).
   This is a core change, so it stays out of the entry.

## Sequencing after the event

Each capability that is still open lands as a tool-table entry with schemas
and goldens before the web app calls it, in the build-plan order of
[specs/delivery.md](./specs/delivery.md#build-plan-oct-27--nov-4): the KT
loop; knowledge, attach and promotion; proposal create and accept;
metrics, reviews and selections; then the receipts pack. Library
persistence and `reindex` ([ORB-18709], [ORB-18710]), open-weight routing
([ORB-18712]) and signals scan ([ORB-18721]) have landed, so they are not
remaining work.

## Task References

- [ORB-16585] — files the orchestrator assignment: rules, go/no-go, coordination, demo and submission for the office-work entry.
- [ORB-18709] — built the Library store adapter with the writer lock, git and settings.
- [ORB-18710] — built `office.init`, `office.reindex` and Library health.
- [ORB-18712] — built the Meta and Baseten model adapters with privacy routing.
- [ORB-18721] — built `office.signals-scan`.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
