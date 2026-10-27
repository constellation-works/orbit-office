---
type: design
summary: "Spec: calendar — a read-only snapshot of the user's Outlook or ICS calendar beside the routines' schedule and the tasks scheduled for a date, a cited prep brief before each meeting, and notes or a transcript captured from the event after it"
last_validated: 2026-10-27
---

# Spec: Calendar

The Calendar screen shows the user's day the way the agents see it: their
meetings, read from Outlook or any ICS feed, the routines that will run, and
the to-dos scheduled for a date.
The agents keep it current. Before a meeting they write a prep brief from
the Library. After it, the user drops in notes or a transcript, and the
meeting becomes a source with a cited summary, decisions and to-dos
([decision](../4_decisions.md#the-calendar-is-a-read-only-snapshot-with-prep-before-and-capture-after)).

The calendar never writes back. Accepting, declining or creating an invite
is an outward-facing action, and none is built
([orchestrator.md](./orchestrator.md#guardrails)).

## The loop

1. **Sync.** The `office-calendar-sync` routine reads the calendar every 15
   minutes through a gateway connection and replaces the snapshot.
2. **Prep.** The `office-meeting-prep` routine finds meetings that start
   within the lead time and have no prep yet. For each one, it writes a cited
   brief.
3. **Meet.** The user attends as usual. Nothing records the call.
4. **Capture.** On a past meeting, the user pastes notes or drops a `.vtt`,
   text or audio file. The app captures it as a `meeting` source linked to
   the event.
5. **Summarize.** The `office-meeting-inbox` routine transcribes audio,
   runs `office.extract` with `profile: meeting`, and saves `summary.md`
   beside the transcript. Decisions are cited, and to-dos are filed for the
   user and the agents.

## Reading the calendar

Two backends feed one tool. Both are read-only, and both go through the
`gateway` plugin, because anything leaving the machine apart from model
inference is an audited gateway read ([gateway.md](./gateway.md)).

| Backend | Connection | Notes |
|---|---|---|
| **ICS feed** | A gateway connection to the feed's origin, using the `secret_path` scheme: the slot holds the path and query, which carry the feed's token | Outlook's "Publish calendar" link, Google's secret address, any CalDAV server's export. Some company tenants disable publishing. |
| **Microsoft Graph** | A gateway connection to `https://graph.microsoft.com/v1.0/`, using the `oauth2_device` scheme with the `Calendars.Read` scope | `GET me/calendarView` over the window, paged through `@odata.nextLink` (each next link must stay under `base_url`). Graph expands recurrences itself. |
| **ICS file** | none | A `.ics` file inside the workspace, read once. Used for the demo and for one-off exports. |

### Gateway auth schemes this needs

Two schemes are added to `gateway`'s connection `auth`, beside `bearer`,
`basic`, `header` and `none`:

- **`secret_path`.** `base_url` is the origin only. The slot holds the path
  and query, which are appended at send time. The audit records the path as
  `<secret_path>`, and redaction masks the value in every response and
  error. A request `path` must be empty: the connection names exactly one
  resource.
- **`oauth2_device`.** For APIs that issue short-lived tokens. The connection
  names `token_url`, `device_url`, `client_id` and `scopes`, all of them
  `https`. Settings starts the device-code flow and shows the user the code
  and verification address. The resulting refresh token goes into the slot
  through Orbit's `secret_updates`, never through a tool result: a plugin
  backend is refused `orbit plugin secret set`, and relaying the token to the
  app would put it in the poll's output. Each call exchanges it for an
  access token held only in memory. A rotated refresh token is written back
  to the slot the same way, before the response is returned. A revoked one
  is refused as `auth_expired`, and Settings offers to sign in again.

Both stay generic: neither knows about calendars. Both are built
([gateway.md](./gateway.md#secret_path)).

## `office.calendar-sync`

Mutating, `mcp_scope: none`. It is called only by the `office-calendar-sync`
routine and by the app's "Sync now".

- **Input:** `{}`, or `{"ics_file": "<workspace-relative path>"}`.
- **Reads** `calendar` from `.office/config.yaml` (below). A missing block,
  or a missing `connection` without `ics_file`, is refused as
  `calendar_unconfigured`.
- **Fetches** through `gateway.http-get` as an Orbit callback, with a
  30-second deadline and capped output, and no retry. Graph pages are
  fetched until the window is covered, at most 10 pages. A full feed is
  refused as `calendar_too_large` when it exceeds the gateway's
  `max_response_bytes`.
- **Normalizes** each instance in the window into an event (below):
  - An ICS feed is expanded here: `RRULE` with `FREQ` `DAILY`, `WEEKLY` or
    `MONTHLY`, `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY` and `BYMONTHDAY`, plus
    `EXDATE` and `RECURRENCE-ID` overrides.
  - A rule outside that subset keeps its first instance, marked
    `recurrence_unsupported`, and is never guessed at.
  - `TZID` accepts IANA names and the Windows zone names Outlook emits,
    mapped through the CLDR table. An unknown zone refuses that event and
    is counted in `skipped`, never read as UTC.
- **Replaces** the snapshot with an atomic write (STD-03@2 §R5):
  `{{plugin_state}}/calendar/events.json`, owner-only (STD-05@1 §R8). A
  failed fetch or parse leaves the previous snapshot unchanged and records
  `last_error`, so a network failure never shows as an empty day.
- **Output:** `{events, skipped, window: {from, until}, synced_at}`.

### Event

```json
{
  "key": "040000008200E00074C5B7101A82E008…:2026-10-28T14:00:00Z",
  "series": "040000008200E00074C5B7101A82E008…",
  "title": "Acme QBR",
  "start": "2026-10-28T14:00:00-04:00",
  "end": "2026-10-28T15:00:00-04:00",
  "all_day": false,
  "status": "confirmed",
  "location": "Room 4B",
  "online": true,
  "organizer": {"name": "Jane Park"},
  "attendees": [{"name": "Jane Park", "email": "jane@acme.example", "response": "accepted"}],
  "agenda": "Q3 numbers, renewal terms",
  "sensitivity": "normal",
  "recurrence_unsupported": false
}
```

- **`key`** is the iCalUID plus the instance's original start, so a moved
  instance keeps its key. **`series`** is the iCalUID alone.
- **`status`** is `confirmed`, `tentative` or `cancelled`. Cancelled events
  stay in the snapshot, so prep and capture can tell "cancelled" from
  "gone".
- **`online`** says only whether a join link exists. The link itself, and
  any dial-in passcode, is never stored.
- **`agenda`** is the event body as plain text, cut at 8 KiB. It is
  untrusted data. An invite can come from anyone, so every model input
  quotes it as data, never as instructions
  ([selection-sharing.md](./selection-sharing.md)).
- **`sensitivity`** is `private` when Outlook marks the event private or
  confidential (Graph `sensitivity`, ICS `CLASS:PRIVATE|CONFIDENTIAL`).
  Otherwise it is `normal`.
- **Attendee email addresses** stay in the snapshot, in plugin state. They
  are used only to match Library person pages through `contact` refs. No
  model view carries them
  ([decision](../4_decisions.md#about-pages-start-from-templates-and-an-onboarding-interview-never-from-stubs)).

## `office.calendar-list`

Read_only, `mcp_scope: workspace`. The orchestrator can answer "what's on
tomorrow?" from it, and the app draws the screen from it.

- **Input:** `from` and `until` (RFC 3339; at most 42 days apart), plus an
  optional `include_cancelled`.
- **Returns** the snapshot's events in the range, each with:
  - `prep`: `null`, or the prep artifact's id;
  - `capture`: `null`, or `{source, summarized}`;
  - `people`: the matched person pages, by id and title.
- Each event's `attendees` carry names and responses only; emails are never
  returned.
- Also returns `synced_at` and `last_error`. An empty snapshot that has
  never synced reads `synced_at: null`, which differs from a synced, empty
  week.

## Prep: `office.meeting-prep`

Mutating, `mcp_scope: none`. It is run only by the `office-meeting-prep`
routine, every 15 minutes. Like the digest, the routine targets a one-step
job that inlines a deterministic `plugin.tool_call`
([sessions.md](./sessions.md#daily-digest)).

1. **Select.**
   - Selects events that are `confirmed` or `tentative`, start within
     `prep_lead_minutes`, have no prep artifact for their `key`, and have at
     least one attendee besides the user.
   - At most `prep_max_per_run` per run, so a busy morning cannot spend
     without bound.
   - All-day events are skipped.
2. **Gather** inputs, with no model call:
   - the event;
   - the person pages its attendees match;
   - the `client` and `project` pages those people link to;
   - the last three `meeting` sources in the same `series`, with their
     `summary.md`;
   - open to-dos tagged with a matched person or with an earlier source
     from the series;
   - the `rhythm` entry that names the series, if one does.
   Every input goes in as its model view.
3. **Write** with one Muse Spark call against a closed JSON Schema:
   - `context`: why this meeting, from earlier meetings;
   - `people`: one line each, from their pages;
   - `open_items`: to-dos and decisions still open;
   - `suggested_questions`.
   Every item cites a source anchor or a page. Uncited items are dropped.
   An event with nothing to cite gets a short brief that says so, without
   a model call.
4. **Route.** If any input is `private`, the call goes to the private route,
   as every call does ([models-and-privacy.md](./models-and-privacy.md)).
   With no private route configured, that event's prep is skipped as
   `private_endpoint_unconfigured`, and the screen says so.
5. **Save.** The brief is written as a Library artifact (kind `report`,
   series `meeting-prep`, with the event `key` in its frontmatter), the way
   the digest writes its own artifact. Its sensitivity is the strictest of
   what it cites. A moved meeting keeps its brief. A rerun for the same key
   is a no-op.

## Capture after the meeting

- **`office.capture`** accepts an optional `event_key`. It copies the
  event's title, start (as `occurred_at`) and attendee names (as
  `participants`) into the new `meeting` source, adds
  `event: {key, series}` to its frontmatter, and takes the event's
  sensitivity. An unknown key is refused as `unknown_event`. Several
  sources may share an event, for example notes now and the `.vtt`
  transcript later.
- **`office-meeting-inbox`**, every 15 minutes, seeded with the `meetings`
  pack, takes each `meeting` source that has no `summary.md`. For each one
  it:
  1. runs `office.transcribe` when the source has audio and no transcript;
  2. runs `office.extract` with `profile: meeting`;
  3. writes `summary.md` beside `source.md`: the summary and cited
     decisions, anchorable as `#L…` like any body.
  It is a deterministic job, like `office-kt-inbox`. The extraction's
  `to_dos` are filed as they are today, tagged `owner:<owner>` and
  `source:<id>` ([kt-videos.md](./kt-videos.md)).
- **The transcript is the source body.** It is never rewritten. A
  correction is a new version (`v2.md`), as for every source
  ([library.md](./library.md#sources)).

## Scheduled tasks

A to-do can wait for a date: "send the weekly numbers on Friday at 9". It is
an ordinary Orbit task that already exists, with a `not_before` time. Orbit
holds it out of every dispatcher until that time, then lets it go on the next
pass ([Orbit scope: scheduled tasks](https://github.com/constellation-works/orbit/blob/agent-main/docs/design/scheduled-tasks/1_scope.md)).
Office adds no scheduler of its own.

- **Setting the time:**
  - To-dos has a **Schedule…** action with a date-time picker in the
    computer's time zone, and **Clear schedule**. The app writes `not_before`
    with `orbit.task.update`, acting as the user.
  - The orchestrator sets it when the user says "on Friday at 9", and reads
    the time back in words.
- **The dispatch pass** skips `owner:agent` backlog tasks whose `not_before`
  is still in the future, and ships them on its first pass after the time.
  Orbit refuses an early ship as `task_scheduled` anyway; skipping keeps the
  pass from logging that refusal every 30 seconds.
- **It runs at or after the time, not at it.** The start depends on the next
  pass, and on the computer being on. The calendar shows the actual start
  beside the scheduled time ("Scheduled 09:00, started 09:42").
- **Your own to-dos** (`owner:you`) can carry a time too. They only appear on
  the calendar; nothing dispatches them.

## Settings

In `.office/config.yaml`, written with `office.config-set`:

```yaml
calendar:
  connection: outlook           # a gateway connection name; omit for ics_file only
  window_days_past: 14          # 1–90
  window_days_ahead: 30         # 1–90
  prep_lead_minutes: 120        # 15–1440
  prep_max_per_run: 5           # 1–20
```

`office-calendar-sync` and `office-meeting-prep` are seeded disabled. The
Calendar screen's setup card turns them on once a connection is chosen, and
both show on the Routines screen.

## App

**Calendar** sits under Work in the rail, between Today and To-dos.

- **Week view** at desktop widths (Monday first, today highlighted) and
  **day view** at phone widths.
- The hours run from the earliest event to the latest, with 08:00–18:00
  always shown. All-day events sit in a strip above the hours. A line marks
  the current time.
- **Meetings** are blocks. Their state appears in words, not colour alone:
  - upcoming: "Prep ready" or "Prep at 12:00";
  - past: "Add notes" or "Summary saved";
  - "Cancelled", struck through;
  - "Private", with the private badge.
- **Routine runs** are thin marks in each day's margin, from the same
  schedule reading as the Routines screen, hollow when paused. They are
  shown, not edited: a mark opens Routines. The schedule reader moves from
  `screens/routines/schedule.ts` into `ui/` so both screens share it.
- **Scheduled tasks** are blocks at their `not_before` time, read with
  `orbit.task.list`'s date-range filter. Their state is in words:
  "Scheduled", "Not approved" (still proposed), "Running", "Done", or
  "Started 09:42" when they started late. Opening one shows the task, with
  **Reschedule** and **Clear schedule**.
- **Opening a meeting** shows a panel:
  - when, where, and whether the meeting is online;
  - attendees, each linked to their person page where one matched;
  - the agenda, shown as quoted text;
  - the prep brief with its citations;
  - after the meeting, **Add notes or transcript** (paste, or drop
    `.vtt`/`.txt`/`.md`/audio), then the summary and to-dos once saved.
  Dropping a file uses the Source screen's upload path, with the event's
  key.
- **Today** gains a "Next meeting" card: the next meeting today, its time,
  and "Prep ready" with a link. It is hidden when nothing is left today.
- **Rail count:** the number of today's past meetings with no notes, shown
  as a plain count, not an alert.
- **Empty and failure states:**
  - never synced: "Connect your calendar", opening Settings › Connections;
  - a sync that failed: the last good snapshot, with "Last synced 10:15,
    the latest sync failed";
  - a week with no meetings, said in words.

Server: `GET /api/calendar?from&until` calls `office.calendar-list`,
`orbit.auto_task.list` and `orbit.task.list` with the `not_before` range. `POST /api/calendar/sync` calls
`office.calendar-sync`. `POST /api/calendar/capture` stages the upload and
calls `office.capture` with `event_key`.

## Demo

The demo uses `ics_file` with a synthetic `demo/synthetic/calendar.ics`: a week of
invented meetings with invented people that match the seeded person pages,
one weekly series spanning the two earlier meetings, one cancelled
instance, and one moved instance retaining its original-start key. It is never a real calendar.

## Not built

- Writing to the calendar: RSVP, creating or moving events.
- Fetching transcripts automatically from Teams or Zoom. Graph's
  `onlineMeetings` transcripts need tenant consent beyond `Calendars.Read`.
- Recording or capturing calls that aren't recorded
  ([decision](../4_decisions.md#the-hero-pack-is-kt-videos-not-meeting-recordings)).
- Email sync. When it arrives, it can reuse the Graph connection with a
  `Mail.Read` scope.

## Callback and snapshot implementation

The one-step `office-calendar-sync` job inlines `plugin.tool_call` of
`office.calendar-sync`. Its routine is registered, every 15 minutes, disabled
on seed. Orbit's descriptor callback guard checks exact granted tool names
without restricting their namespace, so the two-step fallback is unnecessary.
Office requests exactly two additional callback grants: `gateway.connection-list`
(to select the named connection's auth scheme, base and byte bound) and
`gateway.http-get` (all feed and Graph reads). `oauth2_device` selects Graph;
other supported connection schemes select an ICS feed. Each callback has the
existing 30-second deadline, capped capture and no retry. The backend's existing
60-second process deadline remains a whole-sync ceiling; a killed fetch cannot
publish partial events. A killed process may leave no failure receipt.

`events` in sync output is the number of normalized instances written, and
`skipped` counts records/instances with unknown or unresolved zones. Imports
are regular workspace-relative `.ics` files, at most 1 MiB; symlinks and traversal
are refused. Feed/pages also have a 1 MiB Office cap in addition to the
connection's lower `max_response_bytes`. Graph next links must remain inside
the HTTPS base origin and path prefix; credentials, fragments, duplicate query
keys and encoded path separators are refused. At most ten pages, 10,000 events
a 16 MiB normalized snapshot, at most 1,024 properties per event,
and 1,000,000 recurrence candidate days are processed. Reaching a bound refuses
the whole sync and keeps the last good snapshot.

Recurrences preserve wall-clock time through IANA DST transitions. The Windows
mapping uses CLDR release-46 territory `001` defaults. Floating datetimes and
nonexistent DST local times are unresolved and counted; ambiguous local times
use the earlier occurrence. All-day dates use UTC midnight. BYDAY accepts bare
weekdays and monthly ordinals (±1 through ±5); BYMONTHDAY accepts ±1 through ±31.
Unknown RRULE parts or invalid combinations are unsupported, retaining DTSTART
only with the unsupported flag. EXDATE removes instances; a cancelled
RECURRENCE-ID stays as a cancelled event. Overrides inherit unspecified master
metadata and duration. Ranges overlap events and are half-open; cancellation
is excluded by default and included with `include_cancelled: true`.

Snapshot files are private and versioned. A failed attempt writes only a safe
code to owner-only `calendar/last_error.json`, bound to the prior snapshot's
hash. `events.json` is byte-identical on failure. Successful publication changes
the generation and invalidates older failure metadata in one atomic snapshot
write. Reads never create state. The Calendar port and its fake share this
success/failure contract; a future snapshot version refuses reads and writes.
Library person pages match attendee addresses case-insensitively against `contact`
refs (plain addresses or `mailto:`). Only person id/title and attendee name/response
are returned, with attendee addresses also withheld if repeated in invite text.
Prep matches report series `meeting-prep` with `event_key` or `event.key`;
capture matches source `event.key`, with `summary.md` determining `summarized`.
Those producers are separate features. Invite markup, URLs and passcode/dial-in
lines are removed before persistence; agenda remains untrusted plain text,
limited to 8 KiB and quoted as data by downstream consumers.

Calendar settings may be absent for ICS-file use, receiving the defaults above.
`calendar.connection` is null or 1–48 ASCII letters, digits, `_` or `-`;
null clears the connection. The other four keys are integers with the bounds
above; unknown leaves and other types refuse the update.


## Task References

- [ORB-19191] — adds the gateway's `secret_path` and `oauth2_device` schemes.
- [ORB-19192] — builds `office.calendar-sync`, `office.calendar-list` and the snapshot.
- [ORB-19193] — builds `office.meeting-prep` and its routine.
- [ORB-19194] — builds capture from an event and the `office-meeting-inbox` routine.
- [ORB-19195] — builds the Calendar screen, its server feature and Today's card.
- [ORB-19216] — builds the Schedule… picker, scheduled tasks on the calendar and the dispatch pass's wait; needs an Orbit release with ws_orbit [ORB-19214] and [ORB-19215].
- [ORB-19214] — builds Orbit's `not_before` field and its admission hold (ws_orbit).
- [ORB-19215] — builds Orbit's date-range list filter and surfaces (ws_orbit).

### Meeting-prep implementation contract

`office.meeting-prep` accepts `{}` only and returns `written`, `skipped` and
`events`. Each attempted event reports `key`, `written`, nullable `reason`,
`artifact` and `path`. Selection orders starts by instant, then key, includes
starts from now through the configured lead boundary, and applies the run cap
after excluding all-day, cancelled, past, solo and already-prepped events.
Self-only attendee lists are excluded when the `user` page identifies the user
by a contact ref, title or alias. Without that identity, a nonempty invitee
list counts as another attendee; no account address is inferred.

Person matching uses contact refs (plain address or `mailto:`, ignoring case)
or exact display titles/aliases. Client/project links come from `related`,
path refs or wiki links on those people. Meeting sources match `event.series`
(or legacy top-level `series`) and precede this start by `occurred_at` (falling
back to `captured_at`). The latest three include their optional `summary.md`;
source-body line offsets are preserved. Open owner-tagged to-dos match
`person:<page-id>`, the page id as a tag, or `source:<earlier-source-id>`.
The rhythm view includes the lines naming the series. Unreadable or incomplete
Library/task evidence is refused, including a truncated task list.

Each model request uses the standard Muse Spark tier and a closed four-section
schema, with the agenda in an explicitly untrusted, blockquoted data field.
Every view is scrubbed of attendee addresses and all page contact-ref values,
even repetitions outside their contact fields, as well as email/phone text.
Only supplied page ids and resolvable anchors in supplied source bodies are
accepted as output citations. An empty, uncited or partly dangling citation
list drops the entire item. Model text is escaped before citation markup is
appended. If the model returns no supported items, the saved brief says so.
With no citable evidence, the short brief uses no model call.

Private events or gathered inputs require the configured private route; an
unconfigured route returns `private_endpoint_unconfigured` for that event with
no model call or artifact. Other model failures are reported per event and the
batch continues. Reports conservatively inherit the strictest sensitivity of
the event and gathered inputs. Each immutable bundle contains `prep.md`, has
routine approval and optional additive `event_key` frontmatter. The existing
Library writer lock enforces uniqueness of `(meeting-prep, event_key)`, even
for callers that both gathered before publication. Older artifact shapes keep
their existing serialization. `calendar-list.prep` resolves this field.

The registered `office-meeting-prep` routine is seeded disabled on
`*/15 * * * *` and targets `job:office-meeting-prep`; the job's only step
inlines `plugin.tool_call` with `tool: office.meeting-prep` and `input: {}`.
