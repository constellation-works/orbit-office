---
type: design
summary: "Glossary: office"
last_validated: 2026-10-26
---

# Glossary: office

Terms with a specific meaning in this project. General CLI vocabulary (TTY,
stdout, exit code) is excluded.

| Term | Meaning |
|------|---------|
| **Source** | An immutable record of something that happened (meeting, chat, email, doc, receipt, conversation), with anchorable text. See [specs/library.md](../specs/library.md). |
| **Knowledge page** | A derived, citable Library page: `domain` (About) or `runbook` (How-to). Rewritten freely; every claim cites a source anchor. |
| **Artifact** | Agent output the user approved, promoted from a task into the Library, versioned by `series`, never edited in place. |
| **Friction** | An Orbit record of something that went wrong or slowed work; the plugin adds `cites` and `runbook` conventions, never a second store. |
| **Library** | The three layers together — sources, knowledge, artifacts — as its own git repo inside the user's workspace (ignored by the workspace repo), with a rebuildable SQLite index in plugin state. |
| **Proposal** | An Orbit task in `proposed`, tagged `proposal:<kind>`, whose acceptance applies a change and whose dismissal is remembered for 30 days. See [specs/proposals.md](../specs/proposals.md). |
| **Signal** | A mechanical condition computed by `office.signals.scan` (repetition, friction recurrence, staleness, gap, unused automation) that produces a proposal candidate. |
| **Orchestrator** | The user's own agent CLI (Muse Code by default) running the `office-orchestrator` skill over Orbit MCP. Not code in this repo. See [specs/orchestrator.md](../specs/orchestrator.md). |
| **Implementing agent** | An Orbit crew run carrying out an `owner:agent` to-do; its output is a task artifact until approved. |
| **Dispatch pass** | The app server's 30-second loop submitting `owner:agent` backlog tasks by id. The workspace never runs an auto drain. |
| **Needs you** | The app's queue of proposals, approvals and `owner:you` to-dos — everything waiting on the user. |
| **Selection** | A typed reference (object + anchor) the user shared from the app to the agent, held in the tray for at most 15 minutes. See [specs/selection-sharing.md](../specs/selection-sharing.md). |
| **Tray** | The app's "Shared with agent" pill holding up to five fresh selections. |
| **Anchor** | A citable position in a source: `#t=00:12:31` for meetings, `#L14-20` for line-based bodies. |
| **Envelope** | One Orbit exec call: `{"schema_version": 1, "tool", "input", "context"}` in, `{"ok", "output"}` or `{"ok": false, "error"}` out. See [specs/output-contract.md](../specs/output-contract.md). |
| **Tool table** | The one list of `office.*` verbs, accepted input keys and handlers in `office-core`; the manifest, schemas and goldens are tested against it. |
