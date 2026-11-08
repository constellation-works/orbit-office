---
title: synthetic-demo — Overview
owner: daniel
last_updated: 2026-11-08
status: Draft
stage: building
feature: synthetic-demo
doc_role: overview
type: design
summary: Synthetic, clearly simulated demo history and fixtures seeded through the tools, proving integration without claiming real inference or hosted availability.
tags: [synthetic-demo]
paths: ["app/**", "crates/**"]
related_features: [office]
related_artifacts: [ORB-18737]
---

# synthetic-demo — Overview

The synthetic demo seeds believable, clearly simulated history through the
tools ([ORB-18737], done). Its loopback inference fixture is labelled
simulated; it proves tool integration and route provenance, not real
inference quality. The decisions and their rationale are in
[4_decisions.md](./4_decisions.md).

## 1. Motivation

A short demo needs weeks of history; seeded data must never be mistaken for
real work or real inference.

## 2. Core Concepts

- **Seeded history:** synthetic meetings, chats and people created after initialisation through the capture tools.
- **Fixture:** the loopback model endpoint whose counters and provenance the seed verifies.

## 3. At a Glance

| Concern | File | Task |
|---|---|---|
| Synthetic demo data seeded through the tools | [4_decisions.md](./4_decisions.md) | [ORB-18737] done |

## Task References

- [ORB-18737] — built the synthetic demo data, seeded through the tools.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
