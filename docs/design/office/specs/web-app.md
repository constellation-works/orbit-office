---
type: design
summary: "Spec: web app — screens, rules, its authority as the user's console, and delivery as an installable PWA"
last_validated: 2026-11-03
last_updated: 2026-11-03
---

# Spec: Web app

The web app is the user's console: everything except conversation happens
here. Plain words only — To-dos, Routines, Library, Needs you, Insights —
never ids, crews, runs, workflows or raw statuses.

Source: spec §13.1–§13.2 and §13.4.

## Authority

- The local app server is the only process the browser talks to.
- It acts **as the user**: it calls Orbit with `orbit tool run --json` for
  both `orbit.*` and `office.*` tools, including the tools hidden from MCP
  (`office.knowledge-write`, `office.proposal-accept`,
  `office.proposal-reconcile`, `office.artifact-promote`,
  `office.selection-put`, `office.config-set`, `office.reindex`). Settings changed in the app, such as retention on the
  Routines screen, are written with `office.config-set` ([ORB-18711]).
- It never reads `.orbit/` directly. Orbit's HTTP API is not used; it
  belongs to Orbit's own dashboard (spec §18.3).
- It owns the **dispatch pass**: every 30 seconds it lists `backlog` tasks
  tagged `owner:agent` and submits them by id with `orbit run ship <ids>`
  (review-only). It runs as a login service (launchd or a systemd user
  unit), so dispatch continues while the browser is closed. A listing Orbit
  does not return in the expected shape fails the pass and submits nothing;
  it never reads as an empty queue. A task whose `not_before` is still in
  the future is held, not submitted: Orbit would refuse it as
  `task_scheduled`. It is submitted on the first pass after that time
  ([calendar.md](./calendar.md#scheduled-tasks)).
- Live status comes from polling Orbit every few seconds. There is no event bus.

## Screens

| Screen | Content |
|---|---|
| **Today** | Library setup card when missing; otherwise a 48px top bar (**Today**, the date, and one **Add source** button that opens a file picker; a file dropped anywhere on Today uploads the same way and opens the new source, and pasted notes or a folder go through the Source screen), then two columns. The main column holds three row lists ([ORB-21727]): **Needs you** (its count in the header bar; proposals, approvals, routine script outputs, drafted API writes and the user's to-dos, each with "Due Fri 09:00", amber with **Overdue** once past, as on To-dos; the onboarding set ("Set up your Library"), each bootstrap subject and a shared-library sync's updates, conflicts and skipped documents are one row each that opens to their controls), **Since <meeting>** (the digest: decided lines with citation chips, shipped lines with their PR and merge, the meeting's other asks, and **Open meeting**) and **Done today** (collapsed; each row opens its result). The right column (below the main one on a phone) holds Next meeting (time, place and prep; "Nothing else on your calendar today." once the day's meetings are over) and Agents working (one line each, live, across the office workspace and followed workspaces, with the crew in mono). No greeting, intro line or helper copy |
| **Source view** | A back link to Library, the title and a meta line (kind, length, turn count, Private); the original (video or audio player, or image) with a timeline whose marks are the source's chapters (asides for tangents) and gotchas; the transcript as speaker turns (`[hh:mm:ss] Name: text` lines, each with its timestamp link and tagged with the chapter or gotcha marks that fall in it; untagged otherwise); the chapter list with tangents folded and gotchas shown, and keyframe thumbnails; and an aside with **Cited by** (the tasks and pages that cite it) and **Where it ran** (the recorded model routes, the sensitivity, Mark private and team sharing). Video citation links open the player at their second ([kt-videos.md](./kt-videos.md#app)) |
| **Calendar** | Week view (day view on a phone) of the user's meetings, authored locally or read from Outlook or an ICS feed, with each meeting's prep and capture state in words and the routines' runs as margin marks, tasks scheduled for a date as blocks, and the user's own to-dos as due items on their due day and time (overdue open ones stay in view); a meeting opens its attendees, agenda, cited prep brief and **Add notes or transcript** ([calendar.md](./calendar.md#app); local events [ORB-20246]) |
| **To-dos** | An **Agent to-dos go to** picker (the followed workspaces, or None); You and Agents columns, the Agents column led by the meeting agent to-dos (below); each to-do shows its source and attached knowledge, and its scheduled time when it has one: for your own, that time is the due date ("Due Fri 09:00", **Overdue** once past); a **New to-do** form creates one of your own; **Schedule…** (on yours, **Set due date…**) sets or clears the time ([calendar.md](./calendar.md#scheduled-tasks)) |
| **Routines** | The next hour as one track (a mark per scheduled run, hollow where a paused routine skips its slot), then On and Paused tables: a switch, the routine and what it does, when it runs (plain words, with the cron line beneath), when it runs next (worked out from the schedule in the computer's time zone) and how its last time went (from `last_fire` in `orbit.auto_task.list`: when, and its task's state in words; task ids stay on the server). Retention settings below. Scheduled routines use the same tables, with their last run outcome when available. Only shipped component definitions are listed, with a count of hidden Orbit developer routines (see below). Full run history and produced outputs are managed in Orbit. |
| **Library** | **Library setup** (the four bootstrap phases with progress and computed gaps, each gap with one action; [library-bootstrap.md](./library-bootstrap.md#setup-progress)); search; About pages grouped by template (You, People, Clients, Projects, Topics, Glossary and the rest) and How-to pages; one-per-Library pages not written yet shown as "Not set up yet"; Outputs by kind and series; a page or output chosen on the overview opens in the right column in place of Library setup and Outputs, with the list kept beside it, its entry marked and **Close** bringing setup back (search results open the full-width view); a page view whose body is rendered Markdown ([Markdown rendering](#markdown-rendering)) with citations, refs and back-links (a person's meeting sources and other Library documents; task links are deferred until a page-citation read exists); history for every page and artifact; "Shared" and "Shared · from <member>" labels and a compact **Share** switch in the page, source and artifact view header beside **Close** ([shared-library.md](./shared-library.md#app)) |
| **Settings** | **Models** first (below); Library settings; **Shared library** (remote, branch and member name, **Sync now**, the last sync in words; [shared-library.md](./shared-library.md#app)); **Private route** (choose the endpoint kind, URL, model and label; the token goes straight to `orbit plugin secret set`; the check's capabilities in plain words; "Built with Muse Spark Open"; [models-and-privacy.md](./models-and-privacy.md#the-private-route)); **Connections** (add, edit or disable an API connection, and enter its secret, which goes straight to `orbit plugin secret set` and is never shown again; a secret address takes the feed's full address; a sign-in-with-a-code connection shows the user code and verification address, then its signed-in state, and "Sign in again" once it reads `auth_expired`); **API calls** (the gateway audit, newest first) ([gateway.md](./gateway.md)) |
| **Insights** | Automation ratio line, metric tiles, "what got automated" |
| **Review** | Generated reviews with their citations, and a regenerate button |

There is no Chat screen: conversation happens in the user's agent CLI.
There is no Scribe screen either: the scribe, its feed, handoff cards,
ledger panels, Agreement view and Tags editor moved to
`constellation-works/scribe`
([decision](../4_decisions.md#the-scribe-moved-to-constellation-worksscribe)),
and the app has no `/api/scribe` routes. An open decision contradiction is
an ordinary `owner:you` to-do and is settled with `office.decision-resolve`
([decisions.md](./decisions.md#meeting-decisions)).
Citations the agent prints are `http://localhost` links carrying the
source anchor; source links open the Source screen using `/?source=<id>#t=hh:mm:ss` (or
`#L`, `#frame=` and `#chapter=` anchors).

### Review implementation

Review searches the quarterly and yearly review series, reads each returned artifact body, and orders the returned reviews by approval time. Search is bounded to 100 hits per series and matches indexed text, so it is not a complete review listing. At most 100 reviews are shown, with a notice at the limit. Review citation links retain their source references; the Source screen accepts anchored localhost URLs. Regenerate requests the review auto-task and explains when that request is refused.

### Library setup

`GET /api/setup` reads `office.status` Library health. When the Library is missing, Today replaces Needs you with a Set up your Library card describing the scaffold. Set up shows progress and calls `POST /api/setup` once; the server calls `office.init` as the user with full output. Created and already initialized both lead to normal Today and refreshed rail counts. Failures show a reason and fix without automatic retries. If the plugin is disabled or the workspace unregistered, the card directs the user to `orbit-office init` instead of offering the action. Other screens link back to Today while setup is needed.

### Distribution routines

The Routines screen includes only names shipped by the `office` and `gateway`
component plugins. At server feature startup, the server reads
`spec.definitions.routines` and `spec.definitions.auto_tasks` from their manifests,
then each referenced file's `name`. It caches those two sets until restart. A new
file declared in either manifest appears after restart when Orbit lists it; no
application name list needs updating. Files not declared by a manifest, prefixes,
tags and skip messages do not establish membership. The gateway currently ships
no automation definitions; its manifest is still read so additions work the same way.

The reader accepts the repository's literal YAML paths (flow or block lists) and
plain or quoted identifier names. A missing referenced file, invalid name or
unsupported list syntax refuses the snapshot rather than producing an empty filter.
Source installs read `plugin/.orbit-plugin` and
`gateway/plugins/gateway/.orbit-plugin` relative to the app. Standalone app bundles
must carry the same manifest and definition files under
`app/plugins/{office,gateway}/.orbit-plugin` (the bundle root is `app/`). These are
shipped resources, not reads of the user's `.orbit` state.

`GET /api/routines` filters `orbit.auto_task.list` and every page of
`orbit.routine.control` (`action: list`) separately by definition kind. It returns
`routines`, `hidden_count` and `routines_unavailable`; each visible row includes a
`kind` so equal names in the two namespaces stay distinct. The screen reports
"N Orbit developer routines hidden". This is a display filter: it never removes,
pauses or changes hidden automation. The count includes all excluded listing rows,
including custom workspace definitions outside the shipped component sets.

Routine controls still require Orbit operator authority; the app grants none.
If that listing is refused for lack of permission, filtered auto-tasks remain
available and the screen explicitly says scheduled routines are unavailable and
that the count covers auto-tasks only. Other listing failures refuse the snapshot.
Toggling a scheduled routine first observes its current enabled state and target,
then passes those to Orbit's guarded control. Auto-task switches continue to use
`orbit.auto_task.update`. No hidden name can be toggled through this screen's API.
Fresh office workspaces suppress Orbit developer defaults through the
[office preset](./install.md#office-workspace-preset). The name filter remains
for existing workspaces and custom definitions outside the shipped components.

### Today implementation

Today and `GET /api/today` are built ([ORB-18727]); the row-list layout is
[ORB-21727]. The endpoint polls `orbit.task.list`
through a shared Orbit client, with separate active and completed listings
(200 items each). Its shared contract is `Today` in `app/packages/contracts`.
The browser polls the endpoint five seconds after the preceding read settles.
Navigating away stops polling and prevents a late response replacing the new
screen. A redraw keeps open rows and lists (by their `data-focus` key), open
previews, typed comments and keyboard focus.

- Needs you selects `proposed` tasks tagged `gateway-draft`, then
  `script-output`, then `proposal`; all `review` tasks; then unfinished
  `owner:you` tasks. This order also determines the label when a task
  matches more than one kind. Proposals tagged `onboarding` share one
  "Set up your Library" row; proposals tagged `bootstrap:<phase>` share a
  row per `subject:<name>` tag, or per phase without one.
- Agents working selects `in-progress` tasks tagged `owner:agent`,
  excluding `owner:you`, and shows one line per task.
- Done today selects `done` tasks whose latest transition to `done` falls
  between local midnight and the snapshot time. Last-edit timestamps do not
  establish completion. Each result opens its recorded execution summary;
  missing summaries are explicitly identified. The Source screen also provides direct anchored navigation.

Every row is an `li.today-row` with a `data-key` (and `data-kind` or
`data-group`). Its title is a `<button>` named by the title alone with
`aria-expanded`; it opens the row's detail: the recorded task description
(its background, rendered Markdown) and, for a need, the files, request,
preview or pages its controls show. The row's own `approval-actions` sit
inline at the right; a send-back form and every outcome or error message
stay visible under the row, so a refusal is never hidden in a closed row. The
user's own to-dos have no inline action. The page-level outcome line under
the Needs-you header (`aria-live`) outlives the row it came from.

Empty lists show one quiet line ("Nothing needs you.", "No completed work
yet today.", "No agent work is underway."). A failed or malformed Orbit read
refuses the whole snapshot; the UI reports that failure (`role="alert"`) and
retains previous data when available. A capped listing and completed work
without a usable completion date are explicitly reported. Limits may omit older unfinished
work or a result whose record was subsequently edited; this is a bounded
snapshot, not a complete history. The view's root carries the
`drop-zone` slot: dropping a file anywhere on it uploads through the Source
upload, like **Add source**. Each Needs-you row except the user's own to-dos
carries its `approval-actions` (below).

### Morning digest

Built ([ORB-21345]), using the scoped digest from [ORB-21337] through
the read-only `office.source-digest` verb ([ORB-21376]). `GET /api/today/digest` (`TodayDigest` in
`app/packages/contracts`) picks the newest `source:<id>` tags on the user's
`owner:you` tasks and asks `office.source-digest {source}` for up to three of them in
turn: a source that is not a meeting is skipped, `meeting_not_extracted`
answers `not_extracted`, no candidate answers `no_meeting`, and any other
refusal (including `office.source-digest` being unavailable) answers
`unavailable`. Today's **Since <meeting>** list shows its decided lines, its
shipped lines and the meeting's other asks of the user, each with its shortened
citation linked into the Source screen, and **Open meeting**. Each state
has its own quiet line; the digest rereads every 30 seconds and never
invents items.

The digest counts each task id once across the office workspace and
`brief.task_workspaces`, keeping the office workspace's copy first
([ORB-21452]). Repeating the office workspace or a followed workspace
therefore adds no second to-do or decision request to Needs you or to its
header count.

### Calendar implementation

Calendar, its server feature and Today's Next meeting card are built
([ORB-19195]), and so are the scheduled-task blocks ([ORB-19216]) and the
local events with their source choices ([ORB-20246]). The shared
contracts are `CalendarView`, `CalendarMeeting`, `CalendarRoutine`,
`CalendarTask`, `CalendarSynced`, `MeetingNotes` and `MeetingOutcome` in
`app/packages/contracts`.

- **`GET /api/calendar?from&until`** takes two RFC 3339 instants at most 42
  days apart (any other parameter is refused) and calls
  `office.calendar-list` with `include_cancelled: true`, beside
  `orbit.auto_task.list` and the routine listing. Meetings come back sorted by
  start; attendees carry name and response, linked to the matched person page
  whose title is their name, and the other matched pages are listed apart.
  `synced_at: null` means never synced; `sync_failed` says the latest sync
  failed over the last good snapshot (its code stays on the server). Routines
  are the shipped set the Routines screen lists (the definition reader now
  lives in `server/src/orbit/definitions.ts`, shared by both features); a
  failed routine read keeps the meetings and sets `routines_unavailable`.
  Scheduled to-dos come from `orbit.task.list` over the same range
  (`not_before_from`/`not_before_until`) as `tasks`, each with its title,
  time, owner, state and last start. A failed, truncated or malformed read
  keeps the meetings and sets `scheduled_unavailable`
  ([calendar.md](./calendar.md#scheduled-task-implementation)).
  `prep_lead_minutes` is the configuration default, 120: no tool reads the
  configured value back yet.
- **`POST /api/calendar/sync`** calls `office.calendar-sync` with `{}`.
  `calendar_unconfigured` is a 409; any other refusal is `sync_failed`.
- **`POST /api/calendar/capture`** takes `{event_key, text}` as JSON, or a
  file as `application/octet-stream` with `?event_key&name`
  (`.vtt`, `.txt`, `.md`, `.wav`, `.mp3`, `.m4a`, `.webm`). Files are staged
  as Source stages them: owner-only inside `ORBIT_OFFICE_WORKSPACE`, bounded
  to 256 MiB, removed after capture. Either way it calls `office.capture`
  with `event_key`; an unknown key is a 404 `unknown_event` and creates
  nothing.
- **`GET /api/calendar/meeting?source=<id>`** reads a captured meeting's
  `summary.md` through `office.source-show` and its to-dos through
  `orbit.task.list` tagged `source:<id>`, as titles, owners and done state.

The screen draws Monday to Sunday above 820px and one day at a time below,
with a day picker that starts on today. Hours run 08:00–18:00, widened to the
week's earliest and latest meeting; all-day events sit in a strip above, and a
red line marks the current time. A block shows its title, its state in words
and its time: "Prep ready", "Prep at HH:MM" (the start less the lead,
rounded up to the prep routine's quarter hour), "Prep on its way",
"Happening now", "Add notes", "Notes added", "Summary saved", "Cancelled"
(title struck through), "Tentative" and "Private". Colour only repeats the
words. Routine runs are marks in each day's margin from `ui/schedule.ts`
(moved from the Routines screen, which imports it too), hollow when paused;
more than 24 runs a day draw as one band. Every mark opens Routines.
Opening a meeting shows a panel with when, where and whether it is online,
the organizer, attendees (a matched one opens their page's text), the agenda
as quoted text, the prep brief from `/api/sources/citation` with each
`src-…` citation a Source link and each page citation its title, and, after
the meeting, the notes link, summary and to-dos, then **Add notes or
transcript** (paste, pick or drop a file). The states are "Connect your
calendar" (never synced, linking to Settings), "Last synced HH:MM, the latest
sync failed" above the last snapshot, and "No meetings this week." "Sync now"
sits beside the last sync time.
Scheduled to-dos are dashed 30-minute blocks at their time, with "To-do ·
HH:MM", their state in words and "For you" or "For an agent". One opens a
panel with the time, the start, the owner and who starts it, plus
**Reschedule** and **Clear schedule** until it has started. Both actions go
through `POST /api/todos/schedule`, and then the week is read again.
A to-do of yours is a due item: "Due HH:MM", "Due" or "Overdue", and
**Change due date** and **Clear due date** in its panel. While the week holds
now, your open to-dos due before it are read too and listed in today's all-day
row with their date, so an overdue one stays in view
([calendar.md](./calendar.md#scheduled-task-implementation)).

On Today, a Needs you to-do carries `due` (its `not_before`) and reads "Due Fri
09:00", with **Overdue** once past, from the same `ui/when.ts` helpers.

The rail lists Calendar between Today and To-dos, with a plain
count of today's past meetings that have no notes (hidden at zero or on a
failed read). Today's Next meeting shows the meeting now or next today,
its time and place, and "Prep ready" linking to its panel
(`?meeting=<key>#calendar`, cleared once read); it rereads every minute and
is hidden when nothing is left today or the calendar cannot be read.
`web/test/calendar.browser.mjs` covers the week, the day view, the panel,
capture, the three states and the scheduled to-dos (blocks, Reschedule,
Clear and a failed read) at 1440px and 375px.

### Source implementation

Source is built. It accepts files, folder drops and pasted text; single uploads
call `office.capture` once and open the resulting source. Folder capture adds
`bootstrap:<phase>` to each supported file, skips unsupported extensions, and
then previews that phase's whole pending batch (including sources captured
earlier). A partial upload reports how many files were saved; it never retries
a capture automatically.

The app stages each upload in a random owner-only directory inside the configured
`ORBIT_OFFICE_WORKSPACE`, which is within the plugin's existing upload grant.
The filename must be a basename with a supported extension. The server streams
at most 256 MiB per file and removes its staging file and directory on success
or failure. JSON paste input is bounded to 1 MiB. Request reads have a 30-second
idle timeout. No browser-supplied filesystem path is accepted.

A video calls `office.video-plan` for a plan-only preview after capture. A folder
calls `office.batch-plan`. Neither starts model processing. The preview shows
minutes, frames, model calls and the recorded estimate, or “Rates unset”. Process
confirms once with `confirm: true` and the exact digest shown. Marking a preview
private fetches a new preview and disables the old confirmation. Privacy can
only be raised in this UI. Confirmation failures offer a fresh preview rather
than retrying a potentially completed mutation.

A selected source shows a back link to Library, its title and a meta line,
then the player, then the transcript (with chapters and keyframes below it)
beside an aside with Cited by and Where it ran; the two columns wrap to one on
a phone, and capturing another source folds away below. The timeline marks
come from `chapters` and gotcha cites; a turn's tags are the marks that fall
within it. Nothing else tags a turn, so a turn with no mark is untagged.
`/?source=<id>#t=hh:mm:ss`, `#L<n>-<m>`, `#frame=hh:mm:ss` and `#chapter=<n>`
select the Source screen through the app entry point, highlight and scroll the
span, and seek audio/video after metadata loads. Ordinary navigation hashes
still select their named screens. Chapter tangents start folded, while their
gotchas remain visible. Keyframe links seek the same player. Citing tasks and
Library pages open their recorded text through the server; unavailable or capped
task citations are identified. Route badges come from `derived_by`; a recorded
private route uses the configured endpoint label. Mark private calls
`office.source-set-sensitivity` for subsequent processing.

Source JSON is served by `GET /api/sources?id=…`, originals and frames by
`GET /api/sources/media?id=…[&frame=0000.png]`, and citing documents by
`GET /api/sources/citation?id=…&kind=…`. The digest since a meeting is
served by `GET /api/sources/digest?id=…`, which calls `office.source-digest`
([sessions.md](./sessions.md#digest-since-a-meeting)) and returns
its Decided, Shipped and Needs you sections as `SourceDigest` JSON.
Mutations use POST at
`/api/sources/upload`, `/paste`, `/private` and `/plan` (each below
`/api/sources`). Media stays behind `office.source-show` and its privacy checks;
byte ranges allow seeking, but the tool's existing **512 KiB** media cap still
applies. Larger, expired or refused originals show an unavailable message while
the source text remains readable. Source uses the shared dark theme.

### Approvals implementation

Approvals are built ([ORB-18733]). The server feature calls Orbit as the
user; the Today screen shows the controls and says every outcome in plain
words (tool codes are never shown). `GET /api/approvals/detail?key=` returns
what a card shows before the user acts: a proposal's text and the SHA-256 of
its exact `proposal.json`, the output file names of reviewed work or script
output, or a gateway draft's connection, method, URL, body and digest.

- **Approve** reviewed work or script output calls `office.artifact-promote`,
  then walks the task to `done` with a comment naming who approved. There is
  no `verdict` object or `complete` flag: the installed app holds no operator
  authority. A refused promote changes nothing, so the task stays waiting
  for review and the card says why.
- **Send back** moves reviewed work to `backlog` with the user's comment.
- **Accept** calls `office.proposal-accept` once with the digest shown.
  **Accept all** on a grouped card accepts each page in turn; a refused page
  leaves the others accepted, and the card names it.
- **Dismiss** rejects a proposal, script output or gateway draft, with an
  optional reason. After a proposal's rejection the app calls
  `office.proposal-reconcile` ([ORB-20247]) once, which moves its evidence page to
  `proposals/dismissed/`; if that call fails, the dismissal still stands and
  the next reconcile moves the page. The review preview reads the proposal's
  `proposal.json` by task id, never its Library page, so it is the same in
  any status folder.
- **Approve and send** on a gateway draft calls `gateway.http-send` with the
  draft id and the digest shown ([gateway.md](./gateway.md)). A lost reply
  is shown as `outcome_unknown`, never as a failure, and is never retried.
  Dismiss rejects the task and nothing is sent. No other approval sends
  anything: a Library `draft` for someone else is copied, never sent
  ([orchestrator.md](./orchestrator.md#guardrails)).

### Agent work read

`GET /api/agents` ([ORB-19735]) reads the followed workspaces
from `office.status` (`brief.task_workspaces`, [ORB-19740]) and lists
Orbit tasks in progress, in review, queued and done today for the office
workspace and each followed workspace. An unreachable workspace is
reported per workspace without failing the whole response. Today's
**Agents working** section reads it for the followed workspaces. The
Agent work screen that also showed it ([ORB-19741]) was removed in
[ORB-21641]: Today's Agents working, Needs you and Done today and the
To-dos Agents column already show the same work, so the rail has no
Agent work item and an old `#agents` link opens Today.

Since [ORB-21345] the list also follows `meetings.agent_workspace` (from
`office.status` `meetings`), carries each task's `source:` tag as `source`,
and, for In review and Done today (at most 24 tasks), the pull request from
`orbit.task.show {field: "delivery"}` as `pr` (`{number, url, merged}`; the
URL only when the delivery names an `owner/name` repository). Both fields are
omitted when absent.

### To-dos implementation

To-dos are built ([ORB-18728]). `GET /api/todos` lists You and Agents
columns; each to-do shows its source and attached knowledge, and the user
can close their own. An agent to-do has no close action.

A **New to-do** form above the columns creates one of the user's own
to-dos. `POST /api/todos/create` takes exactly `{title}` (a trimmed,
non-empty title of at most 500 characters; any other key or an empty
title is `invalid_input`, 400) and creates it with
`orbit.task.add` tagged `owner:you`, so the new to-do appears in the
You column on the next `GET /api/todos` and can be closed there.
`POST /api/todos` is the same create. A failed creation is
`unavailable` (502). The poll waits while a draft title is being typed,
so a reread cannot lose it.

Each to-do also carries `scheduled` (its `not_before`, or null) and
`schedulable` (its status is `proposed`, `backlog`, `someday` or `blocked`).
A scheduled one reads "Scheduled Fri 09:00". A schedulable one offers
**Schedule…**, or **Reschedule…** and **Clear schedule** once it has a time.
In the You column the same time is the due date ([ORB-21610]): "Due Fri
09:00", an **Overdue** pill once it has passed, and **Set due date…**,
**Change due date…** and **Clear due date**, writing `not_before` through the
same route.
Schedule… opens the shared picker from `ui/when.ts`. The poll pauses while
the picker is open, so a reread cannot close it. `POST /api/todos/schedule`
takes `{id, not_before}` (null clears) and writes it with
`orbit.task.update`. Its refusals are `invalid_input` (400), `not_found`
(404), `not_schedulable` (409) and `unavailable` (502).
`web/test/todos.browser.mjs` covers schedule, reschedule, cancel and clear
at 1440px and 375px.

Meeting agent to-dos ([ORB-21345]): `GET /api/todos/agents` (`AgentTodos`)
returns the target workspace (`meetings.agent_workspace`), the followed
workspaces, and the to-dos filed from meetings: `owner:agent` tasks with a
`source:` tag in the target workspace, each with its state (queued, working,
in review, done or merged, from the delivery record, at most 24 PR reads),
crew, PR and source, plus the user's `owner:you` tasks tagged `contested`,
shown as held until a contradiction is settled. A target that cannot be read
answers `target_unavailable: true`. The screen rereads it at most every 20
seconds and at once after the picker changes. The picker saves through
`POST /api/settings/agent-workspace` (`{workspace}`, a followed workspace or
null), which writes `meetings.agent_workspace` with `office.config-set`.

### Models

Built ([ORB-21345]). Settings › Models (`GET /api/settings/models`,
`ModelsView`) lists, per job, the model that runs it and where it runs:
transcribing meetings (`models.transcribe`), extracting decisions and to-dos
(`models.standard`), careful reasoning (`models.careful`), private sources
(`open_weights.model` on the private route) and agent to-dos (the agent
CLI's crews), as [models-and-privacy.md](./models-and-privacy.md) assigns
them. The hosted rows read `office.status`'s `hosted` block: its endpoint's
host ("Meta Model API (api.meta.ai)" by default) and models (the Muse
defaults while unset). The panel is titled "Models". Each row's check
is live: the hosted API key's presence (`set`/`unset` only), the private route's
last verification, and the agent CLI's connection; a job without a check reads
"off". Below the table: a private route summary linking to its tab and the
same **Where agent to-dos go** picker as To-dos. No model is called to build
the page.

## Look

One dark theme, taken from Orbit's dashboard and written fresh for the app:
Geist (bundled under the OFL in `web/static/fonts/`), a flat side rail on
the darkest surface (`--bg`), the content in an inset bordered panel beside
it (`--bg-panel`, 8px from the window, 10px radius) that goes edge to edge
under the rail on a phone, and panels drawn with a border on the edge and
hairlines inside. `--fg-mute` stays at least 4.5:1 on the panel, raised
surfaces and hover. `web/static/style.css` defines the colour tokens
and the shared patterns. Screens style themselves with `var(--…)` tokens,
never literal colours:

- **Card**: an optional kicker (a short coloured label naming the kind of
  item), the title, the content, then actions.
- **Pill**: a short label or chip; its tone says needs you (amber), an agent
  is on it (cyan), finished (green), private (violet) or failed (red).
- **Tile**: one number and its label, in a wrapping row (Today's counts,
  Insights).
- **Section label**: small upper-case heading over a panel.
- **Rail**: one flat list of 32px items with an icon and 13px label, no
  group labels or nested children ([ORB-21727]): Today, Calendar, To-dos,
  Library and Insights at the top; Routines, Settings and the
  private-route footer pinned at the bottom. The current item has a quiet
  filled background. Source opens from Library (**Add a source**, and
  **Open in Source** on a source document) and Review from Insights
  (**Reviews**); Library and Insights read as open while they are shown.
  A built screen the layout does not name still gets a link. Counts are
  plain mute numbers at the right (routines on of total, today's past
  meetings without notes beside Calendar), except Today's waiting rows in
  amber text. A failed read hides a count rather than showing 0. The
  layout lives in `web/src/nav.ts`; on a phone the rail wraps above the
  content.
- **Row list** (Today, `screens/today/rows.ts`): a 40px header bar (h2 and a
  mono count, or a toggle for a collapsed list) over hairline rows; each row
  has a 14px kind icon (to-do ring, half-filled review, dashed suggestion,
  outgoing-request arrow, amber swap for a contradiction) or a state dot,
  its title as a button that opens the row's detail, a short meta (amber when
  due), and its actions inline at the right as compact ghost (primary) and
  quiet (secondary) buttons, 44px tall on a phone.
- **Panel**: a bordered box with an eyebrow head (`ui/panel.ts`): plain,
  dashed (drop zones) or split into hairline columns; rows inside carry a
  state dot, a title, a mono meta line and a trailing pill or link.
- **Screen head**: date or eyebrow, the h1, a one-line intro and the screen's
  primary action. Today uses a 48px top bar instead: the h1, the date in
  mute and **Add source**.
- **Callout**: an attention-washed block for a notice that needs the user.
- **Table rows** (Routines): a header row of column labels over hairline
  rows; on a phone each row stacks and every cell carries its label.

The approved mocks the screens follow are in
[`../mocks/`](../mocks/) (`*.dc.html`, with `canvas.json`). Their data is
illustrative: screens read live data and never copy it.

## Rules

- Each item has one primary action (approve, edit or dismiss) and a provenance link on every agent output.
- The **selection tray** shows what the user has shared with the agent ("Shared with agent" pill, one-click clear) — [selection-sharing.md](./selection-sharing.md).
- Setup is one [install script](./install.md) that does everything automatically. No terminal is needed during use.

## Delivery

For the event the app ships as a local web app, not a desktop app
([decision](../4_decisions.md#a-local-web-app-installable-as-a-pwa-the-desktop-shell-waits)).

- **Installable PWA.** A web app manifest and a service worker make it
  installable: Chrome's or Edge's "Install app" (Safari: Add to Dock)
  opens it in its own window with a Dock or taskbar icon and no address
  bar, so the demo shows neither a terminal nor a browser. The service
  worker caches the app shell only; data always comes live from the local
  server, and there is no offline data mode. `make app-build`, `make app-install`
  and `make plugin-install` stamp the shell cache with a digest of the build
  commit and shell bytes, changing `sw.js` on each changed build without a
  manually bumped cache version. `/build-info.json` reports the commit and
  digest through the existing static-file server and always bypasses the worker
  cache. A replacement worker removes the previous cache and claims open
  windows; the page offers a new-version notice with **Reload**. It does not
  reload automatically or discard unsaved work. First installation shows no
  update notice. Offline navigation still falls back to the cached shell.

  After `make app-build`, verify the update notice, reload and offline fallback
  with `source "$ORBIT_BROWSER_KIT_ENV"` then
  `node --test app/packages/web/test/pwa.browser.mjs`. Set
  `PWA_SCREENSHOTS_DIR=<temp-directory>` to capture phone and desktop notices.
- **Webview-safe UI.** Relative URLs, and no browser-only API without a
  fallback, so a desktop shell can host the same UI later.
- **After the event: a Tauri shell** — one installer bundling the runtime
  (today the user needs Node 24+), a tray icon, native notifications for
  Needs you, and native drag-and-drop for recordings. Tauri does not
  bundle Node, so the server either ships as a sidecar or moves into the
  `office` binary.

The installable shell (manifest and service worker) and the agent-CLI
settings read have landed ([ORB-18735]). Connections and the API calls
view are built ([ORB-18798]).

### Private route

The Private route section is built ([ORB-18841]). It asks for the endpoint
kind (your company's server, a hosted provider, or your own hardware), the
URL, the model and the label, and shows where that material goes. The section
always says "Private means not sent to the hosted model API, sent only to the
endpoint you chose". Saving writes the whole `open_weights` block with
`office.config-set`, then runs `office.private-route-check` once. A token, when
one was typed, goes only to `orbit plugin secret set office open_weights_token`
on stdin, as one record bound to the endpoint's origin, and is never shown
again. A saved token is sent only to that origin
([models-and-privacy.md](./models-and-privacy.md#the-private-routes-token)):
leaving the field empty keeps it for an unchanged origin, and an endpoint on
another origin is sent no token. The token hint follows
`open_weights_token_binding` from `office.status`, and says so as soon as the
typed endpoint leaves the saved token's origin. *Remove the saved token* sends
`clear_token: true`, which runs `orbit plugin secret rm office
open_weights_token`. The check's capabilities are shown in plain
words (a refused audio probe says that private videos can't be transcribed).
`GET /api/settings/private-route` reads the block from `office.status`
(`open_weights`, or null). The footer and the section show "Built with Muse
Spark Open" only when a block is configured.

### Shared library implementation

Built ([ORB-19421]); the behaviour is in
[shared-library.md](./shared-library.md#app). The settings feature serves:

- `GET /api/settings/shared-library` — `shared` from `office.status` (or
  null) and the latest sync the app ran (`last_sync`, or null).
- `POST /api/settings/shared-library` — the closed `{remote, branch, member}`
  block, saved with `office.config-set` under the settings write lock. A
  refusal answers `invalid_input` with only the `field`; the answer carries the
  `office-shared-sync` routine's state (`on`, `off` or `unavailable`).
- `POST /api/settings/shared-library/routine` — `{"enabled": true}` only;
  turns the routine on through `orbit.routine.control` with the observed
  `expected_enabled`.
- `POST /api/settings/shared-library/sync` — `{}`; one `office.library-sync`
  at a time on a client with a six-minute timeout. It answers document counts
  (`shared`, `removed`, `received`, `incoming`, `conflicts`) and the skipped
  documents by name and reason code, or `sync_failed` with a closed `reason`
  (the tool's refusal code, `sync_timeout`, or `unknown`).
- `GET /api/settings/shared-library/last-sync` and
  `POST /api/settings/shared-library/dismiss-skipped` — the in-memory latest
  sync, which Today reads on every poll, and clearing its skipped list.

The library feature adds `POST /api/library/share` (`{id, shareable, also?}`)
for the user's own documents: it calls `office.share-set` for each `also` id,
sources first, then the document. `share_cites_unshared` answers the cited
documents with their titles (`documents`), at most 20; other refusals answer
their `reason`. The overview adds `shared: {configured}` (or null when
`office.status` can't be read), and document reads accept member-qualified
ids. Approvals pass an optional `choice` (`theirs` or `mine`) to
`office.proposal-accept`, and a proposal's detail reports `shareable` and
`conflict`. Today marks shared-library proposals `shared: "incoming"` or
`"conflict"` and groups them under the `shared-library` key. The label, the
switch and the sync wording live in `web/src/ui/share.ts`, shared by Library,
Source, Settings and Today.

The Library browses the newest 100 matching outputs from `office.artifact-list`; when more match, it says “Showing the latest 100”. Kind and exact series filters narrow the listing, so older outputs and series may not appear in an unfiltered view.

### Markdown rendering

Stored prose is Markdown ([ORB-21605]), and every screen shows it through one renderer,
`renderMarkdown` in `web/src/ui/markdown.ts` (exported from `ui/index.ts`),
rather than as raw text. It handles headings, paragraphs (line breaks kept),
bullet, numbered and nested lists with task boxes, bold, italic, strikethrough,
inline code, fenced code blocks, links, blockquotes, tables and rules, and builds
DOM nodes directly from the text. It is safe by construction:

- Nothing is parsed as HTML or assigned with `innerHTML`: `<script>`, `<img onerror=…>`
  and any other raw tag show as the characters typed, and no `on*` attribute is ever set.
- A link keeps its target only when it is `http(s)` (opened with `rel="noopener noreferrer"`),
  relative, or an in-app `#` route (`isSafeHref`). Any other scheme (`javascript:`,
  `data:`, `vbscript:`, `file:`), a protocol-relative `//host` target or one with control
  characters leaves the link's text without a link. Images are never loaded; their alt text shows.
- A citation token (`[src-…]`, `[dom-…]`, `[rb-…]`, `[art-…]`, and `[ORB-…]` on Review) is
  not a Markdown link: the caller's `cite` hook turns it into its own control, so
  **View source** keeps working inside rendered content.

Where it is used: the Library page, output and source body (citations open the document
in the same view); a Today row's background (a task's description) and a Done-today row's
result (its execution summary); Review text; and Calendar's prep brief.
The web app has no task detail view, so a task's acceptance criteria and comments are not
shown anywhere yet; a view for them renders through the same function. Tests:
`web/test/markdown.test.mjs` (the renderer, including an injected `<script>` and a
`javascript:` link) and the Library and Today browser tests.

## Task References

- [ORB-18711] — built `office.config-set` for Library settings.
- [ORB-18727] — built the Today screen and `GET /api/today`.
- [ORB-18728] — built the To-dos screen and `GET /api/todos`.
- [ORB-18733] — built the Needs-you approval actions and `/api/approvals/*`.
- [ORB-18735] — built the PWA shell and the settings feature's agent-CLI read.
- [ORB-18798] — built the Settings connections and API calls views.
- [ORB-18841] — built the Settings private route and the Muse Spark Open attribution.
- [ORB-19195] — built the Calendar screen, `/api/calendar*`, the rail entry and Today's Next meeting card.
- [ORB-19216] — built scheduled tasks on the calendar, the To-dos Schedule… picker, `POST /api/todos/schedule` and the dispatch pass's wait.
- [ORB-19421] — built Settings › Shared library, the sharing labels and switch, and the shared-library card in Needs you.
- [ORB-19735] — built the Agent work screen and `GET /api/agents`.
- [ORB-19740] — projected `brief.task_workspaces` in `office.status`.
- [ORB-19741] — registered the Agent work screen: route, rail item and count.
- [ORB-20246] — built local calendar events in the app: the editor, source choices and local endpoints.
- [ORB-20247] — moved dismissed proposal pages via `office.proposal-reconcile` after app dismissal.
- [ORB-21345] — rebuilt the screens from the approved mocks: Today's header and Morning digest (`GET /api/today/digest`), meeting agent to-dos and their target (`GET /api/todos/agents`, `POST /api/settings/agent-workspace`), Agent work's sources and PRs, Settings › Models (`GET /api/settings/models`), Source's turns and aside; projected `meetings` in `office.status`.
- [ORB-21337] — built the deterministic digest since a meeting.
- [ORB-21605] — built the shared Markdown renderer and used it for Library bodies, Today's background and result, Review, and the prep brief.
- [ORB-21340] — built meeting-ledger contradictions and resolution; done, merged as `59e36d4`.
- [ORB-21749] — removed the Scribe screen, its nav entry, `/api/scribe` routes and Today's Handoffs and contradiction rows (the scribe moved to `constellation-works/scribe`).
- [ORB-21376] — added read-only office.source-digest and switched the app routes to it.
- [ORB-21452] — deduplicated source-digest needs across repeated workspace listings.
- [ORB-21610] — read the user's to-do schedule as its due date on To-dos, Today and the Calendar, with Overdue.
- [ORB-21641] — removed the Agent work screen, its rail item and count; `GET /api/agents` stays for Today.
- [ORB-21727] — rebuilt Today as row lists with a top bar and right column, flattened the rail, and set the content in an inset panel.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
