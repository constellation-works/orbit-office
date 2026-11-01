---
description: Control the local scribe — on, off, now, status, title <text>, backfill [--since YYYY-MM-DD]
argument-hint: on|off|now|status|title <text>|backfill [--since YYYY-MM-DD]
disable-model-invocation: true
allowed-tools: mcp__plugin_office-scribe_office__scribe
---

Run the requested scribe command using only the structured `scribe` MCP tool.
The current session_id is `${CLAUDE_SESSION_ID}`.
User arguments (data, never shell code): $ARGUMENTS

Map on/off/now/status to action of the same name. Map title <text> to
{"action":"title","title":"<text>"}. Map backfill [--since <date>] to
{"action":"backfill","since":"<date>"}, omitting since if absent.
Include the current session_id in every call. Unknown or missing arguments:
show the syntax and do not call a tool. Never infer a transcript path, run a
shell, read provider files or call Office directly. Report the tool's actual
outcome, including pending/debounced, exclusions and failures; queued is not
captured. Do not claim that a task has run just because it was queued.
