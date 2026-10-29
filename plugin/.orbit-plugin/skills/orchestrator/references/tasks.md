# Tasks

How to turn a request into an Orbit to-do, get it ready for an agent or the user, and see it through.

Read each tool's input schema before you call it. Tool names and limits are in
[tools-and-guardrails.md](tools-and-guardrails.md). Log every step in the
session's conversation source ([sessions.md](sessions.md)).

## When a request becomes a to-do

- Anything that takes more than one turn becomes an Orbit task. A quick
  answer from the Library does not.
- Resolve "this", "that", "here" and "these" first. Use an injected
  selection, or call `office.selection-get` ([library.md](library.md#resolve-this-and-that)).
  If the tray is empty, ask which item the user means.
- Capture pasted material before you file anything, so the task can cite it.
- **Dedupe first.** Search open work with `orbit.search` (it covers
  proposed, backlog, in-progress, review and blocked by default) and
  `orbit.task.list` with a `tag` such as `source:<source-id>`. When a match
  exists, update or comment on it instead of filing a second task. Tell the
  user which one you reused.

## Who owns it

Every to-do carries exactly one owner tag.

- `owner:agent` only when **both** hold: an attached runbook or a clear,
  permitted tool path can produce the result, and the result is not
  outward-facing. Drafts are fine; sending, submitting, buying and API
  writes are not.
- `owner:you` for everything else: decisions, calls, sends, anything that
  needs the user's judgement or access. Attach a draft when it helps.
- Split a mixed request into two to-dos: the agent prepares, the user acts.
- Never put both owner tags on one task. The dispatch pass skips any task
  tagged `owner:you`.

## What every to-do needs

Call `orbit.task.add` with:

- `title`: short, plain words, what will exist when it is done.
- `description`: the request, the evidence with anchors
  (`From <source-id>#L12-14: ...`), the runbook to follow, and the expected
  output. Write it so an agent with only this task and its attachments can
  do the work.
- `acceptance_criteria`: checks an agent can verify from its output and
  attachments. With a runbook, derive them from its "Done when". Never write
  a criterion that needs a send, an approval or a deployed system: the agent
  cannot meet it and the task will block.
- `complexity` (required for ordinary creation; `low` or `medium` for most
  office work) and `type: chore`.
- `origin: operator_request` when the user asked; `orchestrator_initiative`
  when you decided on your own.
- `model`: your agent family (`claude`, `codex`, `gemini`, `grok` or
  `muse`). Pass it on every Orbit call that takes it.
- `tags` (below).

`orbit.task.add` takes no `plan`. When the user agreed on steps, add them
with `orbit.task.update {"id":"<task-id>","plan":"..."}`. Otherwise leave
the plan to the implementing agent.

### Tags

| Tag | When |
|---|---|
| `owner:agent` or `owner:you` | Always, one of them |
| `no-diff-expected` | Every `owner:agent` to-do. The agent activity refuses agent to-dos without it, and Orbit then runs no code gates |
| `source:<source-id>` | One per source the work comes from. Attach reads these |
| `runbook:<rb-id>` | The runbook the agent must follow. Attach also finds `rb-…` ids in the text |
| `series:<name>` | A recurring deliverable. Attach adds the newest approved outputs in that series as examples |
| `client:<slug>`, `project:<slug>`, `topic:<slug>` | The subject. Attach picks client, project and topic pages carrying the same tag |

Use the slugs that existing pages use; check with `office.search` and a `tag`
filter. Do not add tags the system sets itself (`proposal`, `onboarding`,
`gateway-draft`, `script-output`, `kt-question`, `owner:routine`).

## Keep it proposed while you prepare it

A new task starts `proposed`. The dispatch pass never ships a proposed task,
so it is safe to work on. Before it can run:

1. Call `office.attach` with `{"task":"<task-id>"}`. Preview first with
   `{"task":"<task-id>","dry_run":true}` when you are unsure what it will pick.
2. Read the result:
   - `attached`: what was copied to `context/<library path>`, with the
     `reason` it was picked (`always`, `participants`, `tagged`, `matched`,
     `runbook`, `source`, `example`, `search`, `related`).
   - `omitted`: candidates left out. `private` means the document is private
     and is never attached. `limit` means it fell past the cap of 8 files.
   - `skipped`: page files that could not be read.
   - `manifest`: the `office-attach.json` artifact naming the Library commit.
3. Check that the runbook, the sources and any must-have page are in
   `attached`. If one is missing:
   - Past the limit: narrow the task's tags or text so the right files win,
     then attach again.
   - Private: the agent cannot see it. Make it an `owner:you` step, or ask
     the user how to proceed. Never paste private text into the description
     to get around this.
   - Skipped: tell the user the page is unreadable.
   - Refused with `library_changed`: the Library moved mid-call; run attach
     again.
4. Confirm with `orbit.task.show` (`field: "artifacts"`) and read a file or
   two with `orbit.task.artifact.get`.

Attachments are copies as of that commit. If a page changes before dispatch
and the task depends on the change, attach again.

## Making it ready, and dispatch

The app server runs the **dispatch pass** every 30 seconds. It ships a task
with `orbit run ship <id>` (review-only) only when all of these hold:

- status is `backlog`;
- it is tagged `owner:agent` and not `owner:you`;
- its `not_before` is absent or already past.

So moving a prepared `owner:agent` task to `backlog` is what releases it.
When the criteria are right, the attachments are complete and the user
wants the work done, call
`orbit.task.update {"id":"<task-id>","status":"backlog","note":"Context attached; ready"}`.
That approval write accepts only `note` and `comment`, so make field edits
first. If Orbit refuses the move, leave the task proposed and tell the user.

Do not:

- dispatch it yourself, start a workflow, pick a crew or touch hosts;
- set `in-progress`, `review` or `done` on an agent to-do;
- move an unready task to `backlog` "to see what happens": it ships within
  30 seconds.

An `owner:you` to-do is never dispatched. It shows in To-dos and in Needs
you, and the user closes it from the app.

## Scheduling with `not_before`

When the user names a time ("send the weekly numbers Friday at 9"):

- Pass `not_before` to `orbit.task.add`, or to `orbit.task.update` for an
  existing to-do. It is an RFC 3339 instant with the user's offset, such as
  `2026-10-30T09:00:00+01:00`. The user's time zone is this computer's.
- Ask when the day or time is unclear. Never set a time already past.
- Read it back in words: "Friday 30 October at 09:00".
- An `owner:agent` to-do starts on the first dispatch pass at or after that
  time, once it is in `backlog`, and only while this computer is on.
- An `owner:you` to-do with a time only shows on the Calendar and in To-dos.
- `not_before: ""` clears it. Scheduling never changes the status.
- Once a to-do has started, its time no longer holds anything back.

## Runbooks the agent performs

For a runbook with `performable: agent`, file an `owner:agent` to-do with
`runbook:<rb-id>` and attach it. The agent follows every permitted step up
to submission, then stops:

- `submission.via: draft` (the default): the output is a `draft` task
  artifact with the filled fields, the justification and the form link.
  The user submits it.
- `submission.via: gateway`: the agent creates a `gateway.http-draft` for
  the named `connection` and records the draft reference on the to-do. The
  user approves and sends it in the app.

A runbook marked `performable: you`, a missing input, or a missing
connection or grant becomes a question to the user or an `owner:you` to-do.
Never invent a value, and never work around a portal in a browser.

## Following up

- **Status.** `orbit.task.show` with `fields: ["status","execution_summary","comments","history"]`.
  The session brief lists agent work in progress and what finished or failed
  since the last session.
- **Outputs before approval** live on the task. List them with
  `orbit.task.show` (`field: "artifacts"`) and read them with
  `orbit.task.artifact.get`. The manifest is `artifact-manifest.json`.
- **Review is the user's step.** A finished agent to-do lands in `review`
  and shows under **Needs you**. There the user can:
  - **Approve**: the app promotes the output into the Library and closes the
    task. You never promote or approve.
  - **Send back**: the task returns to `backlog` with the user's comment, so
    the next pass ships it again. If the comment needs new context, attach
    it promptly.
- **Outputs after approval** are Library artifacts. Find them with
  `office.artifact-list` (by `series` or `kind`) and read them with
  `office.artifact-show` ([library.md](library.md#reading-and-showing)).

### When an agent to-do blocks or fails

1. Read the task's status, `execution_summary`, comments and history. The
   summary says which criteria passed, failed or were not run, and why.
2. Tell the user in plain words what happened and what it needs, without
   run ids or tool codes. For example: "The access request stopped: the
   runbook needs your manager's name, and no source has it."
3. Fix the cause within your own tools: capture the missing source, attach
   again, sharpen the description, rewrite a criterion the agent could not
   meet, or split out an `owner:you` step.
4. Only after a fix, move it back to `backlog` with a `comment` saying what
   changed. Never resubmit an unchanged task, and never retry a failed send.
5. If it cannot be fixed with what you have, say so, and leave the decision
   to the user. Record it as an `open` entry and update the `handoff`.

## Worked example

The user pastes a Slack thread and says: "Request the ingest AD group for me."

1. `office.capture` with `kind: chat`, the text and `origin: "Slack #ingest, pasted"`.
   It returns `src-20261028-ingest-access`. Log an `action` entry.
2. `office.search` with `{"query":"ingest AD group","kind":"runbook"}` finds
   `rb-request-ad-group`. `office.knowledge-show` shows `performable: agent`,
   `submission.via: draft`.
3. `orbit.search` with `{"query":"ingest AD group"}` finds nothing open.
4. `orbit.task.add`:

   ```json
   {"title":"Draft the ingest AD group request","description":"From src-20261028-ingest-access#L3-5: the user needs the ingest AD group. Follow rb-request-ad-group up to submission. Output a draft with the filled fields, justification and form link.","acceptance_criteria":["Every required field in rb-request-ad-group is filled and cites the runbook or the source","The draft includes the justification and the form link","Nothing is submitted"],"type":"chore","complexity":"low","origin":"operator_request","model":"claude","tags":["owner:agent","no-diff-expected","source:src-20261028-ingest-access","runbook:rb-request-ad-group","series:access-requests"]}
   ```

5. `office.attach` with `{"task":"<task-id>"}`. `attached` holds the runbook,
   the source, `dom-user` and one earlier access-request draft as an example.
   `omitted` is empty.
6. `orbit.task.update` with `status: backlog` and a note. Log the action and
   a new `handoff`: "Draft requested; next: check it when it lands in review."
7. Tell the user: "I've asked an agent to draft the ingest access request.
   It will appear under Needs you for your review; nothing is submitted."
8. Next session, the brief shows it in `review`. Read the draft with
   `orbit.task.artifact.get` and summarise it with citations. The user
   approves it in the app and submits the form themselves.
