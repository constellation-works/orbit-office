---
type: design
summary: "Spec: web app — screens, rules, its authority as the user's console, and delivery as an installable PWA"
last_validated: 2026-10-27
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
| **Today** | Library setup card when missing; otherwise Next meeting (time and prep, hidden when the day's meetings are over), Needs you (proposals, with the onboarding set as one "Set up your Library" card; approvals, including routine script outputs and drafted API writes; bootstrap proposals grouped by subject; a shared-library sync's updates, conflicts and skipped documents as one card; the user's to-dos), Agents working on (one line each, live), Done today (openable results), and a drop zone |
| **Source view** | The original (video or audio player, or image) beside the anchorable text; links to the tasks and pages that cite it. A video adds the chapter list with tangents folded and gotchas shown, and keyframe thumbnails on the timeline; video citation links open the player at their second ([kt-videos.md](./kt-videos.md#app)) |
| **Calendar** | Week view (day view on a phone) of the user's meetings, read only from Outlook or an ICS feed, with each meeting's prep and capture state in words and the routines' runs as margin marks, and tasks scheduled for a date as blocks; a meeting opens its attendees, agenda, cited prep brief and **Add notes or transcript** ([calendar.md](./calendar.md#app)) |
| **To-dos** | You and Agents columns; each to-do shows its source and attached knowledge, and its scheduled time when it has one; **Schedule…** sets or clears the time ([calendar.md](./calendar.md#scheduled-tasks)) |
| **Agent work** | The Orbit tasks agents are carrying across the office workspace and followed workspaces: Working now, In review, Queued and Done today groups, with a workspace filter and an Open in Orbit link |
| **Routines** | The next hour as one track (a mark per scheduled run, hollow where a paused routine skips its slot), then On and Paused tables: a switch, the routine and what it does, when it runs (plain words, with the cron line beneath), when it runs next (worked out from the schedule in the computer's time zone) and how its last time went (from `last_fire` in `orbit.auto_task.list`: when, and its task's state in words; task ids stay on the server). Retention settings below. Scheduled routines use the same tables, with their last run outcome when available. Only shipped component definitions are listed, with a count of hidden Orbit developer routines (see below). Full run history and produced outputs are managed in Orbit. |
| **Library** | **Library setup** (the four bootstrap phases with progress and computed gaps, each gap with one action; [library-bootstrap.md](./library-bootstrap.md#setup-progress)); search; About pages grouped by template (You, People, Clients, Projects, Topics, Glossary and the rest) and How-to pages; one-per-Library pages not written yet shown as "Not set up yet"; Outputs by kind and series; a page view with citations, refs and back-links (a person's meeting sources and other Library documents; task links are deferred until a page-citation read exists); history for every page and artifact; "Shared" and "Shared · from <member>" labels and a **Share with the team** switch on page, source and artifact views ([shared-library.md](./shared-library.md#app)) |
| **Settings** | Library settings; **Shared library** (remote, branch and member name, **Sync now**, the last sync in words; [shared-library.md](./shared-library.md#app)); **Private route** (choose the endpoint kind, URL, model and label; the token goes straight to `orbit plugin secret set`; the check's capabilities in plain words; "Built with Muse Spark Open"; [models-and-privacy.md](./models-and-privacy.md#the-private-route)); **Connections** (add, edit or disable an API connection, and enter its secret, which goes straight to `orbit plugin secret set` and is never shown again; a secret address takes the feed's full address; a sign-in-with-a-code connection shows the user code and verification address, then its signed-in state, and "Sign in again" once it reads `auth_expired`); **API calls** (the gateway audit, newest first) ([gateway.md](./gateway.md)) |
| **Insights** | Automation ratio line, metric tiles, "what got automated" |
| **Review** | Generated reviews with their citations, and a regenerate button |

There is no Chat screen: conversation happens in the user's agent CLI.
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

Today and `GET /api/today` are built ([ORB-18727]). The endpoint polls `orbit.task.list`
through a shared Orbit client, with separate active and completed listings
(200 items each). Its shared contract is `Today` in `app/packages/contracts`.
The browser polls the endpoint five seconds after the preceding read settles.
Navigating away stops polling and prevents a late response replacing the new
screen. Open results, background panels and keyboard focus survive refreshes.

- Needs you selects `proposed` tasks tagged `gateway-draft`, then
  `script-output`, then `proposal`; all `review` tasks; then unfinished
  `owner:you` tasks. This order also determines the label when a task
  matches more than one kind. Proposals tagged `onboarding` share one
  "Set up your Library" card; proposals tagged `bootstrap:<phase>` share a
  card per `subject:<name>` tag, or per phase without one.
- Agents working on selects `in-progress` tasks tagged `owner:agent`,
  excluding `owner:you`, and shows one line per task.
- Done today selects `done` tasks whose latest transition to `done` falls
  between local midnight and the snapshot time. Last-edit timestamps do not
  establish completion. Each result opens its recorded execution summary;
  missing summaries are explicitly identified. “About this work” opens the
  recorded task description as provenance on every item. The Source screen also provides direct anchored navigation.

Empty sections have their own messages. A failed or malformed Orbit read
refuses the whole snapshot; the UI reports that failure and retains previous
data when available. A capped listing and completed work without a usable
completion date are explicitly reported. Limits may omit older unfinished
work or a result whose record was subsequently edited; this is a bounded
snapshot, not a complete history. The view uses the shared dark theme.
An empty `drop-zone` slot reserves the upload integration point; uploading
remains a separate feature. Each Needs-you card except the user's own to-dos
carries its `approval-actions` (below).

### Calendar implementation

Calendar, its server feature and Today's Next meeting card are built
([ORB-19195]), and so are the scheduled-task blocks ([ORB-19216]). The shared
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

The rail lists Calendar under Work between Today and To-dos, with a plain
count of today's past meetings that have no notes (hidden at zero or on a
failed read). Today's Next meeting card shows the meeting now or next today,
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

The original and text appear side by side on desktop and stack on phones.
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
`GET /api/sources/citation?id=…&kind=…`. Mutations use POST at
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
  `office.proposal-reconcile` once, which moves its evidence page to
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

### Agent work implementation

Agent work is built ([ORB-19735]) and registered as a screen, rail item
and count ([ORB-19741]). `GET /api/agents` reads the followed workspaces
from `office.status` (`brief.task_workspaces`, [ORB-19740]) and lists
Orbit tasks in progress, in review, queued and done today for the office
workspace and each followed workspace. An unreachable workspace is
reported per workspace without failing the whole response. The screen is
read-only: it offers no task actions.

### To-dos implementation

To-dos are built ([ORB-18728]). `GET /api/todos` lists You and Agents
columns; each to-do shows its source and attached knowledge, and the user
can close their own. An agent to-do has no close action.

Each to-do also carries `scheduled` (its `not_before`, or null) and
`schedulable` (its status is `proposed`, `backlog`, `someday` or `blocked`).
A scheduled one reads "Scheduled Fri 09:00". A schedulable one offers
**Schedule…**, or **Reschedule…** and **Clear schedule** once it has a time.
Schedule… opens the shared picker from `ui/when.ts`. The poll pauses while
the picker is open, so a reread cannot close it. `POST /api/todos/schedule`
takes `{id, not_before}` (null clears) and writes it with
`orbit.task.update`. Its refusals are `invalid_input` (400), `not_found`
(404), `not_schedulable` (409) and `unavailable` (502).
`web/test/todos.browser.mjs` covers schedule, reschedule, cancel and clear
at 1440px and 375px.

## Look

One dark theme, taken from Orbit's dashboard and written fresh for the app:
Geist (bundled under the OFL in `web/static/fonts/`), a side rail that
stacks above the content on a phone, and panels drawn with a border on the
edge and hairlines inside. `web/static/style.css` defines the colour tokens
and the shared patterns. Screens style themselves with `var(--…)` tokens,
never literal colours:

- **Card**: an optional kicker (a short coloured label naming the kind of
  item), the title, the content, then actions.
- **Pill**: a short label or chip; its tone says needs you (amber), an agent
  is on it (cyan), finished (green), private (violet) or failed (red).
- **Tile**: one number and its label, in a wrapping row (Today's counts,
  Insights).
- **Section label**: small upper-case heading over a panel.
- **Rail**: screens in labelled groups (Work, Knowledge, Observe, Manage),
  each with an icon; a screen can nest under a parent (Library › Source,
  Insights › Review, Automation › Routines). Counts sit at the right: routines
  on of total, today's past meetings without notes beside Calendar, and
  Today's waiting cards as an amber pill. A failed read hides
  a count rather than showing 0. The layout lives in `web/src/nav.ts`.
- **Table rows** (Routines): a header row of column labels over hairline
  rows; on a phone each row stacks and every cell carries its label.

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
  server, and there is no offline mode.
- **Webview-safe UI.** Relative URLs, and no browser-only API without a
  fallback, so a desktop shell can host the same UI later.
- **After the event: a Tauri shell** — one installer bundling the runtime
  (today the user needs Node 20+), a tray icon, native notifications for
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
always says "Private means not sent to Meta's hosted API, sent only to the
endpoint you chose". Saving writes the whole `open_weights` block with
`office.config-set`, then runs `office.private-route-check` once. A token, when
one was typed, goes only to `orbit plugin secret set office open_weights_token`
on stdin and is never shown again. The check's capabilities are shown in plain
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

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.

The Library browses the newest 100 matching outputs from `office.artifact-list`; when more match, it says “Showing the latest 100”. Kind and exact series filters narrow the listing, so older outputs and series may not appear in an unfiltered view.
