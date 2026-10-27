---
type: design
summary: "Spec: calendar — a read-only snapshot of the user's Outlook or ICS calendar beside the routines' schedule, a cited prep brief before each meeting, and notes or a transcript captured from the event after it"
last_validated: 2026-10-27
---

# Spec: Calendar

The Calendar screen shows the user's day the way the agents see it: their
meetings, read from Outlook or any ICS feed, and the routines that will run.
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
  and verification address. The resulting refresh token goes into the slot,
  through `orbit plugin secret set` on stdin like every other secret. Each
  call exchanges it for an access token held only in memory. A rotated
  refresh token is written back to the slot before the response is returned.
  A revoked one is refused as `auth_expired`, and Settings offers to sign in
  again.

Both stay generic: neither knows about calendars.

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

Server: `GET /api/calendar?from&until` calls `office.calendar-list` and
`orbit.auto_task.list`. `POST /api/calendar/sync` calls
`office.calendar-sync`. `POST /api/calendar/capture` stages the upload and
calls `office.capture` with `event_key`.

## Demo

The demo uses `ics_file` with a synthetic `demo/calendar.ics`: a week of
invented meetings with invented people that match the seeded person pages,
one weekly series with two earlier captured meetings, and one cancelled
instance. It is never a real calendar.

## Not built

- Writing to the calendar: RSVP, creating or moving events.
- Fetching transcripts automatically from Teams or Zoom. Graph's
  `onlineMeetings` transcripts need tenant consent beyond `Calendars.Read`.
- Recording or capturing calls that aren't recorded
  ([decision](../4_decisions.md#the-hero-pack-is-kt-videos-not-meeting-recordings)).
- Email sync. When it arrives, it can reuse the Graph connection with a
  `Mail.Read` scope.

## Open questions

- Whether an Orbit plugin's callback descriptor may call another plugin's
  tool (`gateway.http-get`). If it may not, `office-calendar-sync` becomes
  a two-step job: a `plugin.tool_call` of `gateway.http-get` that writes the
  response to the job's scratch space, then `office.calendar-sync` reading
  that file.

## Task References

None yet.
