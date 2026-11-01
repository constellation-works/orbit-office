---
type: design
summary: "Spec: scribe v1 — a dedicated, cheap scribe agent that turns operational chat with orchestrators into one tagged, grep-able SCR file per session and answers questions from it with exact citations"
last_validated: 2026-11-01
---

# Spec: Scribe v1

Status: specified, not built. Supersedes, as the default way to capture
conversations, the automatic live scribe in
[sessions.md](./sessions.md#live-scribe) and the recall surfaces in
[recall.md](./recall.md). Those stay in the tree but are off by default and
unused by v1 (see [Relation to what is built](#relation-to-what-is-built)).

## Why

Daniel works with several orchestrators at once, plus meetings and
coworkers. Decisions, to-dos and "did that land?" facts are scattered across
chat logs, and he and the orchestrators keep asking each other again. v1 adds
one **scribe agent**: a cheaper model that does the bookkeeping. It takes
notes when told to, keeps them in an organized, grep-able form, and answers
"did Daniel decide on this?" or "which features landed today?" with exact
citations. Orchestrators stop carrying that load.

## Goals and non-goals

v1 does:

- Scribe a chat session on command, as often as asked, adding only messages
  not yet scribed.
- Take direct notes: "remember this", "I need to do X at 3pm", "decision is Y".
- Keep one SCR file per session, with entries tagged and file-level topics
  set from configuration.
- Answer questions from Daniel or an orchestrator with file, line and message
  citations, or say plainly that nothing was captured.

v1 does not do:

- Automatic capture on Stop or SessionEnd hooks.
- Reminders that fire at a time.
- Voice.
- A viewer for parsed conversations.
- The web Scribe screen or the desktop pane.

These are later steps; v1 leaves room for them.

## Commands

All commands are in the Claude Code plugin `plugin/claude-code/`. Each acts
on the session it is run in, identified by `${CLAUDE_SESSION_ID}`.

| Command | Does |
|---|---|
| `/scribe` | Catch up: parse the transcript, scribe the messages after the cursor, report what was added (counts per tag and the new lines). |
| `/scribe note <text>` | Record `<text>` verbatim as Daniel's own entry. The scribe agent picks the tag unless the text starts with one (`#todo …`). |
| `/scribe ask <question>` | Answer from the SCR files with citations. |
| `/scribe title <text>` | Set the session title, which renames the SCR file once (below). |

Orchestrators reach the same agent as the `scribe` subagent: "ask the
scribe whether Daniel decided on X". A standalone scribe session
(`claude --agent scribe`) is the place for notes taken during a meeting or a
call. Its own SCR file has `counterpart: self`. It is also the later voice
surface.

`/scribe` is safe to run any number of times. A run with nothing new adds
nothing and says so.

## Pipeline

```
transcript ──parse (no model)──▶ .orbit/tmp/scribe/<session-id>.jsonl
                                        │  messages after scribed_through
                                        ▼
                         scribe agent (cheap model) ──▶ entries (JSON)
                                        │
                     append (no model, validates) ──▶ SCR-<dd-hhmm>-<title>.md
```

1. **Parse: model-free.** It reads the provider transcript through the
   existing conversation adapter ([sessions.md](./sessions.md#adapter-and-provenance)).
   That adapter keeps only the text exchanged between Daniel and the agent:
   no tool calls, tool results, thinking or sidechains. Credentials are
   scrubbed as today. Output is `.orbit/tmp/scribe/<session-id>.jsonl`, one
   message per line:
   `{"id":"m-0042","role":"user|assistant","at":"<rfc3339>","text":"…"}`.
   Ids are stable: the nth kept message is always `m-<n>`, so re-parsing a
   longer transcript only adds lines. The file is rewritten on every run and
   stays private (`0600`, private directory).
2. **Scribe: cheap model.** The `scribe` agent (Haiku or Luna class) gets the
   rules and the messages after `scribed_through`, plus up to 10 earlier
   messages as read-only context. It returns entries:
   `{"msg":"m-0042","tag":"decision","topics":["recall"],"quote":"…","refs":["ORB-20756","PR#268"],"due":null}`.
3. **Append: model-free, Office tool.** It writes the SCR file:
   - It refuses an entry whose `quote` is not an exact substring of the cited
     message, or whose tag or topic is not in the rules. Refusals are
     reported, never written.
   - It appends one line per entry, merges new topics into the front matter
     and moves `scribed_through` to the last message given to the agent,
     whether or not that message produced an entry.
   - It commits to the Library repository like any source capture.

   The model never edits the file, so a bad run cannot corrupt earlier
   entries, and every citation points at real text.

Only Daniel's lines can become `decision`, `direction` or `todo` (the
existing guard: an assistant line never becomes a decision or direction).
Assistant lines are scribed only for `landed` and `fact`, and for an answer
to a question Daniel asked.

## The SCR file

Location: the Library's `sources/conversation/<yyyy>/<mm>/`, as a single
file `SCR-<dd-hhmm>-<title-slug>.md`.
- `dd-hhmm` is the session's start in the host's local time.
- The slug is at most 48 characters.
- On a clash, the first 6 characters of the session id are appended.

Until a title is set, the slug comes from Daniel's first message. `/scribe
title` renames the file once and records the old name in `aliases`.

```markdown
---
id: SCR-01-0307-scribe-v1-design
title: Scribe v1 design
session_id: 0a31040c-e7e2-411a-8100-1eba1b8c1dde
provider: claude-code
model: claude-opus-5-5
counterpart: orchestrator          # orchestrator | agent | coworker | meeting | self
counterpart_name: opus orchestrator (ORB-20690)
host: macbook
project: constellation
cwd: ~/workspace/constellation
branch: agent-main
transcript: ~/.claude/projects/-Users-daniel-workspace-constellation/0a31040c-….jsonl
parsed: .orbit/tmp/scribe/0a31040c-….jsonl
started_at: 2026-10-31T20:07:00-07:00
last_scribed_at: 2026-10-31T22:41:00-07:00
scribed_through: m-0187
topics: [scribe, orbit-office, recall]
aliases: []
---
- m-0185 2026-10-31T22:24-07:00 #decision #scribe "lets go one step at a time" refs: ORB-20690
- m-0186 2026-10-31T22:30-07:00 #todo #scribe "write the v1 spec" due: 2026-11-01T15:00-07:00
- m-0187 2026-10-31T22:33-07:00 #landed #recall "merged as 7266911" refs: PR#268, ORB-20756
```

Line format, one entry per line so grep finds it:

`- <msg-id> <time> #<tag> #<topic>… "<verbatim quote>" [refs: …] [due: …] [by: user]`

- Quotes are verbatim, with newlines written as `\n`.
- `by: user` marks `/scribe note` entries.
- `#corrected` follows an entry that a later `correction` line amends. Lines
  are never edited in place.

Most questions can be answered with grep alone:
- `grep -rn '#decision.*#recall' sources/conversation/2026/11/` finds the
  decisions about recall.
- `grep -l 'topics:.*recall' -r sources/conversation/` finds the sessions
  that touched recall.

## Configuration

The rules live on the existing Library page `dom-scribe-rules`
([sessions.md](./sessions.md#rules-page)). It changes only through
proposals, so Daniel holds the pen. v1 gives its body three structured
sections that the append step reads:

- **Tags:** name, one-line meaning and one or two examples each. Defaults:
  - `decision`: Daniel settles a choice.
  - `direction`: a standing instruction.
  - `question`: an open question for later.
  - `todo`: an action, optionally with `due`.
  - `reminder`: a time-bound note to self.
  - `fact`: a stated fact about hosts, people or configuration.
  - `landed`: a PR merged or a task done, reported in the chat.
- **Topics:** the allowed topic list, for example project names and feature
  slugs. The agent may only use listed topics. It may suggest a new one in
  its report, and adding it is a proposal.
- **Capture:** whose messages may take which tags (the defaults above), and
  what to skip, such as status chatter and restated plans.

With no page, the defaults above apply and topics fall back to the project
name.

## Answering

The `scribe` agent answers from the SCR files only. Its tools are Grep and
Read over the Library's `sources/conversation/`; it has no web access, no
writes and no Orbit calls.

- Every answer cites `SCR file:line` and the message id. It quotes the entry
  and names the counterpart and the session (`claude --resume <session-id>`).
- With nothing found, it says "not captured", lists which date range and
  topics it searched, and names any session not scribed past its last
  message. Those are sessions with messages after `scribed_through`; the
  agent can see them by comparing against the parsed file when it exists.
- "Did we land X?" answers from `#landed` entries. When the plugin's `gh`
  source is available, it adds the live PR state and says which part is
  live and which was captured.
- "Today" and similar words resolve in the host's local time zone and are
  stated in the answer ("today = 2026-11-01, PDT").

## Relation to what is built

| Built | In v1 |
|---|---|
| Conversation adapter and scrubbing (ORB-20685) | Reused for parse. |
| `dom-scribe-rules` page (ORB-20686) | Reused; body gains the three sections. |
| Claude Code plugin shell, `/scribe`, subagent (ORB-20689) | Reused; commands replaced as above, `recall` subagent becomes `scribe`. |
| Stop/SessionEnd automatic passes, debounce, window heartbeat | Off by default; `/scribe` is the only trigger. |
| Conversation sources as `src-…/source.md` | Left as they are; new sessions write `SCR-…md`. No migration in v1. |
| `office.recall`, `office.recall-answer`, `office.scribe-control`, corrections | Unchanged and unused by v1. Corrections may be reused later as `correction` lines. |
| Desktop pane (ORB-20697), web Scribe screen (ORB-20696, parked) | Unused by v1; ORB-20696 stays parked. |

## Build order

1. **Parse and append (Office tools, no model):**
   - `office.scribe-pending` parses the transcript, writes the `.jsonl`, and
     returns the SCR metadata, `scribed_through`, the messages after it and
     the rules.
   - `office.scribe-append` validates and writes entries.
   - New tool slots go through an integrator scaffold first (ARCHITECTURE.md
     § Integrator-owned files).
2. **The `scribe` agent and commands** in the Claude Code plugin: `/scribe`,
   `note`, `ask`, `title`, and the subagent definition with its model and
   tools. Turn the automatic hooks off by default.
3. **Rules page sections:** a proposal to `dom-scribe-rules` with the default
   tags, topics and capture rules.

Each step is one task and lands before the next is filed.
