---
type: design
summary: "Spec: models and privacy routing — which model does what, and how private sources stay on open weights"
last_validated: 2026-10-26
---

# Spec: Models and privacy routing

Only inference leaves the user's machine. Which model it goes to depends
on what the input contains.

Source: spec §9.

| Use | Model | Notes |
|---|---|---|
| Orchestrator | Muse Code (default) running Muse Spark or Muse Wonder | Any MCP-capable agent CLI works. The orchestrator plus agents loop is the candidate for the agentic bonus prize |
| Meeting transcription | Muse Voice Transcribe | Speaker turns and timestamps feed the anchors |
| Summaries, decisions, to-do extraction | Muse Spark | Structured output validated against a JSON Schema |
| Daily digest | Muse Spark, one call a day | Through the same routing; private items go to open weights or are listed by reference ([sessions.md](./sessions.md#daily-digest)) |
| Receipt and bill field extraction | Muse Spark with image input | Confirm the request shape in the API reference on day 1. Fallback: local OCR, then Muse Spark on the text |
| Sources marked `private` | Muse Spark Open (`meta-models/muse-spark-open-1026` on Baseten, or local) | Candidate for the open-weights bonus prize |
| Implementing agents | Orbit's existing crews, including its Muse crews | Pre-existing: not counted as new Meta-tooling work |

## Routing rules

- **Sensitivity.** Set per source (default by kind, for example `email` → `private`) or by the user. Any model call whose input includes a `private` source goes to the open-weight model. The routing decision is logged in the source's `derived_by` and in the task.
- **Artifacts inherit sensitivity:** an artifact's `sensitivity` is the strictest of what it cites ([library.md](./library.md)).
- **Selections from private objects carry no quote** ([selection-sharing.md](./selection-sharing.md)); the agent re-reads through the tools so the routing applies.
- **Secrets.** The Meta API key and Baseten token are stored with `orbit plugin secret set office <name>`, never in config or the Library. Knowledge pages never hold secrets; `refs` name files, links, contacts and templates only.
- **Other people's words.** Pasted chats contain third parties' messages. They stay local, and the demo uses synthetic content only.

The plugin's network grant is `any` (the only setting that reaches remote
hosts), but the code calls only the Meta and Baseten endpoints named in
config. That restriction is a code rule, reviewed, not a sandbox setting.
