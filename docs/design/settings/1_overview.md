---
title: settings — Overview
owner: daniel
last_updated: 2026-11-08
status: Draft
stage: shipped
next: "—"
feature: settings
doc_role: overview
type: design
summary: The app Settings screens for connections and API calls, following the gateway's shipped wire contract with secrets kept out of tool input.
tags: [settings]
paths: ["app/**"]
related_features: [office]
related_artifacts: [ORB-18798]
---

# settings — Overview

The Settings screens cover connections and API calls ([ORB-18798], done).
They follow the gateway's shipped wire contract; connection metadata and
secret storage are separate operations, and secrets never appear in tool
input. Navigation reachability was fixed in [ORB-18905] (done). The decisions and their rationale are in
[4_decisions.md](./4_decisions.md).

## 1. Motivation

The user manages gateway connections and credentials from the app without
exposing secrets to the tool surface.

## 2. Core Concepts

- **Connection:** gateway connection metadata in a fixed slot, written without its secrets.
- **Secret:** a credential stored through a separate Orbit operation, reported only as set or unset.

## 3. At a Glance

| Concern | File | Task |
|---|---|---|
| Connections and API calls views | [4_decisions.md](./4_decisions.md) | [ORB-18798] done |
| Settings screen reachable from navigation | [4_decisions.md](./4_decisions.md) | [ORB-18905] done |

## Task References

- [ORB-18798] — built the Settings Connections and API calls views.
- [ORB-18905] — fixed the built Settings screen being unreachable from navigation.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
