---
type: design
summary: "Spec: selection sharing — typed references from the app to the agent CLI, their tray, lifetime and safety"
last_validated: 2026-10-26
---

# Spec: Selection sharing

The conversation happens in the agent CLI while the user reads in the app.
Selection sharing lets the user point at something in the app, then refer
to it in the CLI ("make this a to-do for me", "why does this page say
March?") without copy-pasting.

Source: spec §13.3. Should-have for the event.

## Selection record

It stores a typed reference, not a DOM path. Every view already renders
one object at an anchor, and a DOM path would break with each UI change
and mean nothing to the agent.

```yaml
id: sel-…
object: src-20261028-acme-qbr        # source, knowledge page, artifact or task id
anchor: "#t=00:12:31-00:13:05"       # or #L14-20; null for the whole object
quote: "…we can't go above 3% this year…"   # omitted for private objects (below)
quote_sha256: …                      # detects that the object changed since
view: source                         # source | page | artifact | task
sensitivity: normal
selected_at: 2026-10-28T15:20:04-04:00
consumed_at: null
```

## Storage and lifetime

- Selections live in their own table, `plugin_state/selections.sqlite`, written through `office.selection-put`.
- They are short-lived UI state, so they are kept apart from the rebuildable Library index and never committed to the Library.
- A selection expires after 15 minutes or once the agent has used it.
- The **tray** holds up to five fresh selections, so "compare these two" and "merge these into the Acme page" work.
- The app shows the tray as a "Shared with agent" pill, with a one-click clear, so the user always knows what the agent can see.

## How the agent gets it

Pulling works with every agent CLI. Pushing through a hook is an
enhancement.

- **Pull:** `office.selection-get` over MCP, plus the skill rule: on "this", "that", "here" or "these", the agent calls it first. This works in Muse Code, Claude Code and Codex. `selection-get` is mutating because it marks selections consumed.
- **Push:** a prompt-submit hook adds the fresh, unused selections to the user's next prompt and marks them consumed.
  - Claude Code supports this through its `UserPromptSubmit` hook, which is the reference implementation shipped in the plugin.
  - Whether Muse Code has a prompt hook is checked on day 1. Without one, the pull rule covers it.

Only the app writes selections: `office.selection-put` is
`mcp_scope: none`.

## Safety

- **Private objects.** A selection from a `private` object carries only the reference and anchor, never the quote. Otherwise an orchestrator running on a hosted model would send private text off the machine, bypassing the routing in [models-and-privacy.md](./models-and-privacy.md). To use the content, the agent goes through the office tools, which route private material to the open-weight model.
- **Quoted text is data.** Selections, whether injected or pulled, are wrapped as quoted Library data with their object id. Instruction-like text inside a highlighted chat or email is never treated as instructions.
- **Staleness.** `quote_sha256` detects that the object changed since it was selected; the agent re-reads through the tools rather than trusting the quote.
