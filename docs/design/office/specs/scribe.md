---
type: design
summary: "Spec: scribe v1 — a dedicated, cheap scribe agent that turns operational chat with orchestrators into one tagged, grep-able SCR file per session and answers questions from it with exact citations"
last_validated: 2026-11-01
---

# Spec: Scribe v1

Status: build step 1 built (`office.scribe-pending`, `office.scribe-append`;
see [Build order](#build-order)); the agent and commands are not built yet.
Supersedes, as the default way to capture
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
| `/scribe pending`, `/scribe pref <matter>`, `/scribe landed [today\|<date>]`, `/scribe why <matter\|topic>` | Model-free queries over the tags; instant, cited lines ([Answering](#answering)). |
| `/scribe ask <question>` | Answer a free-form question with citations; runs the queries first and calls the model only on the shortlist. |
| `/scribe backfill [--since <date>] [--project <name>]` | Run the catch-up over the user's existing transcripts, oldest first. Each session gets its SCR file; sessions already scribed to their end are skipped. |
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
Assistant lines are scribed only for `landed` and `fact`, for an answer to a
question Daniel asked, and for the orchestrator tags below.

### Orchestrators scribe their own work

An orchestrator, and above all an unattended on-call shift, records what it
decided and what it left open. Its execution summary on the assignment task
no longer has to carry that alone. Four tags belong to agents:

| Tag | Meaning |
|---|---|
| `agent-decision` | What the orchestrator decided and why, with the task or run ids. |
| `pending` | An open issue that needs Daniel or a follow-up. |
| `resolved` | Closes a `pending`; carries `resolves: <SCR id>#L<n>`. |
| `incident` | Something broke, and what was done about it. |

- An agent entry carries `by: <crew>` (for example `by: opus`), and the
  append step refuses an agent tag without it. Daniel's tags (`decision`,
  `direction`, `todo`) are never accepted with an agent `by`, so no agent
  records a decision in Daniel's name.
- Orchestrators write with `/scribe note #<tag> …` as things happen. A
  note is model-free, so it is cheap enough for every wake. They also run
  `/scribe` at the end of each wake and of the shift. In the orchestrator's
  own session, its assistant lines may take the four agent tags.
- A `resolved` entry must name an existing `pending` line; the append step
  refuses one that does not.
- The assignment task stays the session's system of record. The scribe is
  the searchable index across sessions: short lines that cite the task
  (`refs: ORB-…`) instead of copying its comments, and the execution
  summary cites the SCR lines.

Open items are the `#pending` lines with no `#resolved` line pointing at
them. "What's pending from last night?" answers from exactly that set.

### Decisions keep their why

The scribe exists as much for *why* as for *what*. Before anyone, Daniel or
an orchestrator, removes or reverts a setting, rule or piece of code they
don't understand, they ask the scribe why it is there (Chesterton's fence).

- `decision` and `agent-decision` entries carry `why: <msg-id|note>
  "<verbatim reason>"`. The scribe agent pairs the decision with the message
  that gives its reason, which is often a different message. The append step
  checks the reason quote against that message, as it checks any quote.
- A decision with no stated reason is still written, tagged `#no-why`.
  `grep '#no-why'` lists the gaps, and Daniel or the orchestrator fills one
  with `/scribe note #why resolves: <SCR id>#L<n> "<reason>"`.
- "Why is X like this?" is a question type. The answer is the decision with
  its `why`, who made it and when, and its citations. With nothing recorded,
  it says "no recorded reason" and never guesses a reason.
- The `orchestration-session` skill requires the check: no removal or revert
  of something unexplained without asking the scribe, and with no recorded
  reason, asking Daniel.

### Preferences: answer once

Some of Daniel's answers settle a whole class of question, not one case.
Asked once, for example "release early and exclude pending defects?", the
answer should spare every later orchestrator from asking again.

- `preference` is one of Daniel's tags. It carries a `matter:` key, a short
  slug for the class of question (`release-with-open-defects`), plus the
  verbatim answer and its `why`. Matter keys are listed in the rules
  alongside topics; the scribe agent may propose a new one.
- The scribe agent tags a line `preference` when Daniel answers a question
  that is about a kind of situation, or says so ("always", "from now on",
  "next time"). When unsure, it tags `decision` and lets Daniel promote it
  with `/scribe note #preference matter: <key> resolves: <SCR id>#L<n>`.
- **Before asking Daniel, an orchestrator asks the scribe** "does Daniel
  have a preference on <matter>?"
  - A current preference whose matter matches: the orchestrator applies it
    without waiting. It tells Daniel in one line that it applied the
    preference, with the citation, so he can override.
  - A low-confidence match, or none: it asks Daniel as before.
  - An override is a new preference on the same matter. It carries
    `supersedes: <SCR id>#L<n>`, and the old line drops out of answers.
- **Preferences go stale on purpose.**
  - Each has `review_by`, by default 90 days after it was stated.
  - Past that date the scribe reports it as stale; the orchestrator asks
    Daniel and records his answer as a fresh preference.
  - "Same as before" renews it.
- A preference that keeps being applied is a candidate for the Library's
  `dom-standing-directions` page, through a proposal Daniel accepts.
- Preferences stated in chat are separate from the decision precedents an
  Orbit task's decision requests build (ORB-20747). Both follow the same
  rule: only current, cited answers count.

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

`- <msg-id> <time> #<tag> #<topic>… "<verbatim quote>" [why: <msg-id> "…"] [refs: …] [due: …] [matter: …] [review_by: …] [supersedes: …] [resolves: …] [by: user|<crew>]`

- Quotes are verbatim, with newlines written as `\n`.
- `by: user` marks Daniel's `/scribe note` entries; `by: <crew>` marks an
  orchestrator's entries. A note has no message id and uses `note` in its place.
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
  - `preference`: Daniel's standing answer to a class of question
    ([Preferences](#preferences-answer-once)).
  - `agent-decision`, `pending`, `resolved`, `incident`: orchestrator tags
    ([above](#orchestrators-scribe-their-own-work)).
- **Topics:** the allowed topic list, for example project names and feature
  slugs. The agent may only use listed topics. It may suggest a new one in
  its report, and adding it is a proposal.
- **Capture:** whose messages may take which tags (the defaults above), and
  what to skip, such as status chatter and restated plans.
- **Models:** one model and effort per level, as `<model>@<effort>`.

  | Level | Default | Why |
  |---|---|---|
  | `query` | none | Canned queries are model-free. |
  | `ask` | `haiku@low` | Reads a shortlist of tagged lines and quotes them. |
  | `scribe` | `haiku@medium` | Judgment that is written once and read many times: preference vs decision, pairing `why`, choosing `matter`. |

  In Claude Code the `scribe` subagent runs Claude models only. Where the
  scribe runs through Orbit or Codex, for example an on-call orchestrator
  on the box, a Luna equivalent (`luna@low`, `luna@medium`) may be set.
  The setting changes cost and quality without a code change.

With no page, the defaults above apply and topics fall back to the project
name.

## Answering

The model is spent at write time: the scribe agent reads each message once
and attaches the structure grep cannot infer (tag, `matter`, topics, `why`,
refs). Reading then needs a model only for questions that structure cannot
answer. Answering has two levels.

1. **Query, model-free.** The Office tool `office.scribe-query` filters SCR
   lines by tag, `matter`, topic, `by`, date range, refs and session, and
   resolves the links between lines:
   - a `resolved` closes a `pending`;
   - `supersedes` retires a preference;
   - a `#why` note fills a `#no-why`.

   It returns the matching lines with file:line. These questions are canned
   queries:

   | Question | Query |
   |---|---|
   | What's pending? | `#pending` with no `#resolved` naming it, oldest first |
   | Preference on X? | the current `#preference` with `matter: X`, with stale flag |
   | What landed today? | `#landed` in today's local date range |
   | Why is X like this? | `#decision`/`#agent-decision` on that matter or topic, with `why` |

   Orchestrators use the query to check for a preference before asking
   Daniel, so that check costs nothing. Any structured part of an answer can
   be reproduced with grep.
2. **Ask, model on the shortlist.** For free-form questions the `scribe`
   agent first runs queries and Grep to narrow the candidates. It then reads
   only that shortlist and composes the cited answer. It never reads the
   whole Library. It runs at the `ask` level (default `haiku@low`).
   - Before an answer is shown, the plugin checks each quoted span against
     the cited line, as the append step checks writes, and drops any quote
     that does not match.
   - A cheap model therefore cannot misquote; at worst it leaves something
     out.

The `scribe` agent answers from the SCR files only. Its tools are the query
tool, Grep and Read over the Library's `sources/conversation/`; it has no
web access, no writes and no Orbit calls.

- Every answer cites `SCR file:line` and the message id. It quotes the entry
  and names the counterpart and the session (`claude --resume <session-id>`).
- With nothing found, it says "not captured", lists which date range and
  topics it searched, and names any session not scribed past its last
  message. Those are sessions with messages after `scribed_through`; the
  agent can see them by comparing against the parsed file when it exists.
- "Why is X like this?" returns the decision and its `why`, or "no recorded
  reason" ([Decisions keep their why](#decisions-keep-their-why)).
- "Does Daniel have a preference on X?" returns the current preference for
  that matter with its citation, age and `review_by`, says if it is stale,
  or says "no preference recorded".
- "What's pending?" lists `#pending` lines with no `#resolved` line that
  names them, oldest first, each with its `by`, task refs and file:line.
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
   - Built (ORB-20780). A message is one transcript record's kept text;
     pending also writes `<session-id>.meta.json` beside the `.jsonl` (the
     session facts append needs) and withholds a private session as the
     capture preview does. Entries are `{msg, quote}` or a note `{text}`,
     each with `tag` and optional `topics`, `refs`, `due`, `resolves`, `by`.
     Every refused entry is returned with a reason code; a message at or
     before the file's `scribed_through` is read-only context. Front matter
     always carries every key above on one line (an untitled session has
     `title: null`), so `#L<n>` file lines never move. Times use the host
     zone from the clock port.
2. **The query tool, the `scribe` agent and commands.**
   - `office.scribe-query`, model-free, through an integrator scaffold.
   - In the Claude Code plugin: `/scribe`, `note`, `pending`, `pref`,
     `landed`, `why`, `ask` and `title`, and the subagent definition with
     its model and tools. Turn the automatic hooks off by default. The
     `orchestration-session` skill then requires orchestrators to scribe
     decisions and open issues as they happen.
3. **Rules page sections:** a proposal to `dom-scribe-rules` with the default
   tags, topics and capture rules.

Each step is one task and lands before the next is filed.
