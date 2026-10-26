---
title: office — Overview
owner: daniel
last_updated: 2026-10-26
last_validated: 2026-10-26
status: Draft
feature: office
doc_role: overview
type: design
summary: The office plugin backend: an Orbit exec service whose first tool is status, growing into Library capture, knowledge, proposals, metrics and reviews.
tags: [office, plugin, orbit]
paths: ["crates/**", "plugin/**"]
related_features: [office]
related_artifacts: []
---

# office — Overview

`office` is the Rust backend for the orbit-office plugin. Orbit invokes
`office orbit-tool` with one JSON envelope on stdin; the backend answers
with one JSON envelope on stdout. The first tool is `office.status`; the
Library tools (capture, knowledge, artifacts, proposals, metrics, reviews)
land slice by slice, each with its schemas, handler, goldens and tests.

## 1. Motivation

Office work compounds when sources, knowledge and approved outputs are
kept locally, with citations, and agents act on them under approval. The
backend's job is to be the deterministic half of that: protocol dispatch,
validation, and (later) the Library. Model calls and the web app live
outside this workspace's layers.

## 2. Core concepts

- **Envelope:** `{"schema_version": 1, "tool": "office.<verb>", "input": {...}, "context": {...}}`.
- **Tool table:** the one list of verbs, accepted input keys and handlers;
  manifest, schemas and goldens are tested against it.
- **Context authority:** identity and secrets arrive in `context`, never in
  `input`; handlers report secret presence, never a value.

## 3. At a glance

Two crates: `office-core` (protocol domain, no I/O) and `office` (the
binary surface). See [ARCHITECTURE.md](../../../ARCHITECTURE.md).
