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
| **Template** | A page template in `library/.office/templates/`: id pattern, required sections, attach policy and skeleton. `office.init` installs the bundled ones; pages name theirs with `template:`. See [specs/knowledge-templates.md](../specs/knowledge-templates.md). |
| **Onboarding interview** | The first session's short set of questions whose answers become `fact` entries and one `knowledge_add` proposal per About page. |
| **Model view** | A page as sent to a model or attached to a task: every `contact` ref keeps its label and loses its value. |
| **Scratch** | Throwaway files: `$ORBIT_SCRATCH_DIR` for agent runs, `.orbit/tmp/session-<date>/` for the orchestrator, a run directory for plugin tools. Never a result and never in the Library. See [specs/scripts.md](../specs/scripts.md). |
| **Script run** | One `office.script.run` call from a routine step: an approved script run offline in its own directory, its outputs filed as a `script-output` task for review. |
| **Connection** | A user-granted API endpoint in the `gateway` plugin: base URL, auth scheme, secret slot, allowed methods. Agents use it by name and never see its secret. See [specs/gateway.md](../specs/gateway.md). |
| **Gateway draft** | An API write an agent prepared, waiting under Needs you; approving sends it once, bound to the digest the user saw. |
| **KT video** | A knowledge-transfer recording, captured as a `video` source; the hero pack turns it into chapters, cited knowledge proposals and questions. See [specs/kt-videos.md](../specs/kt-videos.md). |
| **Chapter** | A timed topic segment of a video, labelled `core` or `tangent`; tangents are folded in the app, and their gotchas lifted out. |
| **Gotcha** | A warning said in passing, kept as cited knowledge even when its chapter is a tangent. |
| **Video plan** | The cost preview for a video: duration, keyframes, model calls and estimate. Processing waits until the user confirms its digest. |
| **Artifact** | Agent output the user approved, promoted from a task into the Library, versioned by `series`, never edited in place. The daily digest is the one artifact promoted by a routine without review. |
| **Friction** | An Orbit record of something that went wrong or slowed work; the plugin adds `cites` and `runbook` conventions, never a second store. |
| **Decision log** | The append-only `conversation` source of one orchestrator session: decisions, verbatim directions and facts, actions, open questions. See [specs/sessions.md](../specs/sessions.md). |
| **Handoff** | The agent's note to its next session: the newest `handoff` entry in the decision log, refreshed whenever the plan changes, and the first thing the session brief shows. |
| **Session brief** | What `office.session.brief` returns at session start: waiting on you, agents working on, open questions, the last session's decisions. Computed, never stored. |
| **Daily digest** | The day's cited summary, a `report` artifact in the series `digest-daily`, written by a routine with one Muse Spark call. |
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
