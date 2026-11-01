---
type: design
summary: "Spec: scribe v1 — a dedicated, cheap scribe agent that turns operational chat with orchestrators into one tagged, grep-able SCR file per session and answers questions from it with exact citations"
last_validated: 2026-11-01
---

# Spec: Scribe v1

Status: build step 1 built (`office.scribe-pending`, `office.scribe-append`;
see [Build order](#build-order)). Step 2's Claude Code plugin part is built
(ORB-20799): the `scribe` agent, the `/scribe` commands and automatic hooks
off by default ([plugin guide](../../../../plugin/claude-code/README.md));
`office.scribe-query` is being built separately. The web Scribe screen is
built over v1 (ORB-20696; [web-app.md](./web-app.md#screens)).
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
- The desktop pane. (The web Scribe screen, first listed here, is built
  over v1's SCR files: ORB-20696.)

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
| `/scribe backfill [--since <date>] [--project <name>]` | Run the catch-up over the user's existing transcripts, oldest first. Each session gets its SCR file; sessions already scribed to their end are skipped. Headless agent-to-agent runs, whose first conversational record carries `entrypoint: sdk-cli` (Orbit workers, `claude -p`), are skipped as headless: counted in `skipped`, never refused and never handed to the scribe agent. An explicit `/scribe` inside such a session still scribes it. |
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
transcript ──parse (no model)──▶ <library>/.office/state/scribe/<session-id>.jsonl
                                        │  messages after scribed_through
                                        ▼
                         scribe agent (cheap model) ──▶ entries (JSON)
                                        │
                     append (no model, validates) ──▶ SCR-<dd-hhmm>-<title>.md
```

1. **Parse: model-free.** The Claude Code plugin stages the provider
   transcript into the workspace first (see
   [Staging](#staging-tool-payloads-are-not-staged) below), then
   `office.scribe-pending` reads the staged file through the
   existing conversation adapter ([sessions.md](./sessions.md#adapter-and-provenance)).
   That adapter keeps only the text exchanged between Daniel and the agent:
   no tool calls, tool results, thinking or sidechains. Credentials are
   scrubbed as today. Harness wrapper text in a user message is not Daniel's
   words: `<system-reminder>` blocks and `<local-command-*>` output are
   dropped, and a slash command is kept as one `/<name> <args>` line
   ([sessions.md](./sessions.md#adapter-and-provenance)). Output is
   `<library>/.office/state/scribe/<session-id>.jsonl`, one message per line:
   `{"id":"m-0042","role":"user|assistant","at":"<rfc3339>","text":"…"}`.
   Ids are stable: the nth kept message is always `m-<n>`, so re-parsing a
   longer transcript only adds lines. The file is rewritten on every run and
   stays private (`0600`, private directory). It sits in the Library's
   git-ignored `.office/state/`, inside the plugin's `{{workspace}}/library`
   write grant (Orbit refuses any plugin write grant under the workspace's
   `.orbit`), and follows a configured `library_dir`. The `parsed` path
   (tool output and SCR front matter) is workspace-relative, such as
   `library/.office/state/scribe/<session-id>.jsonl`. The Claude Code
   plugin's own hook files stay under `.orbit/tmp/scribe/`.
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

### Staging: tool payloads are not staged

The plugin's stager (`scribe_v1.py::V1.stage`) does not copy the provider
transcript whole. Per record it stages only what the adapter reads: the
record envelope (`type`, `sessionId`, `timestamp`, `cwd`, `gitBranch`,
`isMeta`/`isSidechain`/`isCompactSummary` and ids), the message role and
model, string content, and `text` content blocks verbatim. Every non-text
block (`tool_use`, `tool_result`, `thinking`, …) is staged as a minimal stub
carrying its type and small ids with its payload dropped, and other large
record fields (for example `toolUseResult`) are dropped. Malformed lines are
kept verbatim, as is a trailing complete record; a trailing partial record
is left out, as before.

- **Message ids are unchanged.** Ids count kept text messages only, so a
  staged parse assigns exactly the `m-<n>` ids an unstripped parse would.
- **Prefix stability.** Stripping is a pure per-record function with no
  whole-file threshold or positional logic, so re-staging a growing
  transcript leaves the staged bytes of unchanged records unchanged (the
  staged bytes feed the transcript and per-line prefix digests).
- **Privacy is unchanged.** Credential-shaped fragments in a dropped
  payload — provider token prefixes (`sk-`, `ghp_`, `AKIA`, `eyJ`, …), PEM
  blocks, secret assignments, bearer tokens and credential-bearing URLs,
  mirroring `office-core`'s `scrub` — are kept verbatim in a parser-ignored
  `_preserved` field on the stub (or on the record for dropped record-level
  fields). Office's scrub of the staged bytes therefore still marks such a
  session private and `scribe-pending` withholds it, as for the unstripped
  file. No other tool-payload text is staged.
- **Over-cap conversations.** Office's 8 MiB cap (`conversation::INPUT_LIMIT`)
  applies to the staged file. A tool-heavy session whose provider file is
  far over the cap normally stages under it and scribes normally. If the
  conversational text alone (with preserved credential fragments) still
  exceeds 8 MiB, `office.scribe-pending` still refuses the whole call with
  `session_transcript_too_large`: there is no windowed parse of the messages
  after `scribed_through`, because message ids need the full prefix. The
  practical remedy today is to scribe a long session in passes as it grows;
  a windowed reader in Office is future work, not part of this staging
  change. The legacy automatic-capture snapshots (`provider-scribe-…`) are
  unaffected: they still copy provider bytes whole under their own 8 MiB
  bound, and no `from_line` capture reads a `scribe-<id>.jsonl` file.

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

Until a title is set, the slug comes from Daniel's first message with
remaining text after harness stripping (above): a message emptied by
stripping never names the file, and a bare slash command such as
`/scribe backfill` names it when nothing else exists. `/scribe
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
parsed: library/.office/state/scribe/0a31040c-….jsonl
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

   The Office tool `office.scribe-ask` is this level for callers without a
   subagent (the web Scribe screen calls it through `orbit tool run`). Its
   input is `question`, plus optional `since`, `until`, `project`,
   `session_id` and `limit`; unknown keys are refused. The shortlist is
   model-free: the same date, session and project filtering as
   `office.scribe-query`, plus lexical matching of the question against
   tags, `matter`, topics and quotes, bounded by `limit`. It then makes
   one call through the Models port at the `ask` level (default
   `haiku@low`, mapped to the `Standard` tier the port supports). Every
   returned claim cites `SCR file:line` and the message id, and its quoted
   span must match the cited SCR line verbatim; unmatched quotes (and
   claims citing lines outside the shortlist) are dropped and counted in
   `dropped_claims`. With nothing found it returns `not_captured: true`
   with the searched date range, topics, and the sessions searched with
   their `scribed_through` cursor (sessions not scribed past their last
   message). Private SCR entries follow
   [models-and-privacy.md](./models-and-privacy.md): with no open-weights
   route they are cited by reference only (`private_references`, no quote
   sent to any model), as `office.recall-answer` does; on a configured
   route the call is `private` and citing claims return with
   `content_withheld`.

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
| Desktop pane (ORB-20697) | Unused by v1. |
| Web Scribe screen (ORB-20696) | Built over v1, commanded only: the feed and the Pending, Preferences, Landed and Why views read `office.scribe-query`; a note is appended through `office.scribe-append` at the session's own `scribed_through`, as `/scribe note` records it; the ask box calls `office.scribe-ask`. No automatic capture, heartbeat or capture switch. `office.scribe-query` results carry the file's `title`, `project` and `scribed_through` for it. |

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
   - Plugin part built (ORB-20799). A structured `scribe` MCP tool runs
     each command: model-free ones finish there, and a catch-up, an
     untagged note, `ask` and backfill return a request for the `scribe`
     subagent. The plugin stages the transcript inside the workspace for
     pending, sets `through` itself, holds a note or title on a session
     with no `scribed_through` for the first catch-up, and quote-checks
     answers before display. Decisions:
     [claude-code-scribe](../../claude-code-scribe/4_decisions.md#scribe-v1-command-surface).
3. **Rules page sections:** a proposal to `dom-scribe-rules` with the default
   tags, topics and capture rules.

Each step is one task and lands before the next is filed.
