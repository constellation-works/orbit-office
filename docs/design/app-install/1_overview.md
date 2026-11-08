---
title: app-install — Overview
owner: daniel
last_updated: 2026-11-08
status: Draft
stage: shipped
next: "—"
feature: app-install
doc_role: overview
type: design
summary: Installing or upgrading the plugin rebuilds and restarts the web app, publishing only generated directories with rollback material retained.
tags: [app-install]
paths: ["scripts/**", "app/**"]
related_features: [office]
related_artifacts: [ORB-21110]
---

# app-install — Overview

Installing or upgrading the plugin must leave the web app running the new
build: the installer rebuilds and restarts it ([ORB-21110], done; the stale-app
report [ORB-21077] is done). Publication replaces only generated directories
and retains rollback material ([ORB-18965], done); the decisions and their rationale are in
[4_decisions.md](./4_decisions.md).

## 1. Motivation

An install that updates the plugin but not the running app leaves the user on
a stale build that does not match the installed tools.

## 2. Core Concepts

- **Candidate:** the pinned archive build produced away from the running service.
- **Publication:** the per-directory replacement of generated output, verified before success is reported.

## 3. At a Glance

| Concern | File | Task |
|---|---|---|
| Installer rebuilds and restarts the web app | [4_decisions.md](./4_decisions.md) | [ORB-21110] done |
| Standalone install provisions the app | [4_decisions.md](./4_decisions.md) | [ORB-18965] done |

## Task References

- [ORB-21077] — reported installs leaving the web app stale.
- [ORB-21110] — built the installer upgrade path that rebuilds and restarts the web app.
- [ORB-18965] — fixed standalone installs never provisioning the app.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
