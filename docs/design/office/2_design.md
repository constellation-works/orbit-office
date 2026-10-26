---
title: office — Design
owner: daniel
last_updated: 2026-10-26
last_validated: 2026-10-26
status: Draft
feature: office
doc_role: design
type: design
summary: How office is layered and how one exec invocation flows from stdin to stdout.
tags: [office, plugin, orbit]
paths: ["crates/**", "plugin/**"]
related_features: [office]
related_artifacts: []
---

# office — Design

## 1. Layering

See [ARCHITECTURE.md](../../../ARCHITECTURE.md): protocol domain
(`office-core`), composition (`office::app`), surface (`office::cli`,
`office::output`).

## 2. The life of an invocation

1. `main` parses argv with clap; a parse failure is a usage error (exit 2).
2. `app::serve_exec` reads stdin to a string.
3. `office_core::serve` parses the envelope, checks `schema_version`,
   dispatches on the tool table, and refuses unknown tools and unknown
   input keys.
4. The handler returns its output payload; `serve` wraps it in
   `{"ok": true, "output": ...}` or an error envelope with a stable code.
5. `output` writes the one envelope to stdout; a closed pipe is a silent
   exit 0.

## 3. Contracts

The envelope shapes are specified in
[specs/output-contract.md](./specs/output-contract.md).

## 4. Gates

`make ci` runs fmt, the structure scripts, the vendored standards check,
clippy, cargo-deny, tests and goldens — the template's gates, kept.
