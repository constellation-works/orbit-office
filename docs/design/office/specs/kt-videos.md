---
type: design
summary: "Spec: the KT pack — an hour-long knowledge-transfer video becomes a chaptered index with tangents folded away, cited knowledge proposals, a runbook and questions for the presenter, behind a cost preview"
last_validated: 2026-10-26
---

# Spec: KT videos (the hero pack)

Knowledge-transfer (KT) videos are the long screen-share recordings a team
makes before someone leaves or hands a system over. They run an hour, half of
it tangents. Watching one costs an hour of attention, and what it teaches is
lost unless someone writes it down. This pack turns one into durable,
cited knowledge
([decision](../4_decisions.md#the-hero-pack-is-kt-videos-not-meeting-recordings)).

Meetings stay supported but are not the hero. A meeting's actions are cheaper
to tell the agent directly, and pasted notes and imported `.vtt` transcripts
cover the rest. The `meetings` pack remains, without its own routine.

## What the user gets from one video

1. **A chaptered index.** Topic segments with start and end times, each
   labelled `core` or `tangent`. Tangents are folded away, not deleted. A
   warning said in passing inside one ("never restart the ingest worker on a
   Friday") is lifted out as a **gotcha**. The source view plays from any
   chapter.
2. **Knowledge proposals**, every claim citing a timestamp:
   - a `runbook` for each procedure the video demonstrates;
   - `knowledge_add` or `knowledge_update` proposals for `topic`, `project`
     and `systems` pages, with repositories, dashboards and URLs seen on
     screen as `refs`;
   - `person` pages for owners and people to ask;
   - glossary terms.

   All of them are built from the About templates
   ([knowledge-templates.md](./knowledge-templates.md)) and go through the
   per-topic consolidation of the Library bootstrap, so one video adds to the
   pages other sources already feed, rather than making its own
   ([library-bootstrap.md](./library-bootstrap.md#stage-2-consolidate-per-topic-careful)).
3. **Questions for the presenter.** Gaps and promises ("I'll send you the
   config later") become `owner:you` to-dos tagged `kt-question`, each naming
   the person to ask and citing the moment.
4. **Ask the video.** The transcript, the frame readings and the chapters are
   indexed under the source, so "how do I rotate the ingest keys?" is answered
   with timestamp citations through the existing search.

## Library shape

A video is a source of kind **`video`** ([library.md](./library.md#sources)):

```
library/sources/2026/10/src-20261027-ingest-kt/
  source.md      # frontmatter + transcript, one paragraph per turn: [hh:mm:ss] Speaker: …   (#t=)
  frames.md      # one entry per frame read: [hh:mm:ss] what is on screen, visible text      (#frame=)
  chapters.md    # frontmatter chapters list + a readable index
  frames/        # the frame images, 640 px wide (git-ignored, like original.*)
  original.mp4   # git-ignored, pinned by sha256, dropped at retention expiry
```

- **Anchors:** `#t=00:12:31` for the transcript, `#frame=00:12:31` for a
  frame reading, and `#chapter=3` for a chapter.
- `source.md` frontmatter adds
  `video: { duration_s, plan_digest, confirmed_at, calls, frames_read }`.
  `derived_by` names how each derived file was made: transcription
  (`muse-voice-transcribe` or `vtt-import`), frames (`muse-spark`) and
  chapters (`muse-spark`).
- `chapters.md` frontmatter:
  `chapters: [{ n, start, end, title, label: core|tangent, summary, gotchas: [] }]`.
- Retention drops `original.*` and `frames/` together. The transcript, the
  frame readings and the chapters stay, so the knowledge outlives the video.

## Pipeline

A video costs real money to process, so nothing is spent before the user
sees the cost and confirms it. After that, a fixed routine does the work:
no agent loop, so the number of model calls is known in advance.

### 1. Capture

`office.capture` accepts `mp4`, `webm`, `mov` and `m4a`. With a `.vtt` next to
it (`transcript_path`), the VTT becomes the transcript and transcription is
skipped: speaker names and cue times come straight from the file, with no
model call.

Both VTT cue endpoints must fit in `u64` milliseconds, including the fractional
part. Hours above 99 are supported. Malformed or overflowing cue timestamps
refuse the entire capture with `invalid_argument` before saving a source or
committing Library changes; they are never wrapped or silently dropped.

### 2. Plan and confirm: `office.video-plan`

Mutating, `mcp_scope: none`; called by the app after an upload.

- `ffprobe` reads the duration. `ffmpeg` finds keyframe candidates with
  its scene-change filter and keeps at most one per 10 seconds. It drops
  near-duplicates by a 64-bit difference hash of a 9×8 grayscale thumbnail,
  computed by the plugin from `ffmpeg`'s raw output, with no image library.
  It caps the result at `video.max_frames_per_hour` (default 120), keeping
  the highest-scoring scene changes spread evenly across the video.
- The plan counts its model calls: audio chunks (0 with a VTT), one per
  frame, and the extract call or calls. It prices them from
  `costs.*` in Library settings. With no rates set, the preview shows the
  counts and says that rates are unset. It never invents a price.
- The plan is stored with its digest. The app shows it as "48 minutes, 96
  frames, about 110 model calls, ≈ $X" and a **Process** button.
  **Process** calls `office.video-plan` with `confirm: true` and the digest
  the user saw. A plan whose inputs changed since then is refused
  (`plan_changed`; STD-05@1 §R21).
- A video can be marked private at its preview, and then every call goes to
  the private route ([models-and-privacy.md](./models-and-privacy.md#the-private-route)).
  Muse Spark Open takes audio and images, so a private video is refused at
  planning only when the configured endpoint failed that capability check
  (`private_media_unsupported`, naming what is missing). It is never quietly
  sent elsewhere.

### 3. Process: the `office-kt-inbox` routine

The routine runs every 15 minutes, seeded disabled, and processes only
confirmed plans. Each is a `plugin.tool_call` step:

1. **`office.transcribe`** extracts the audio with `ffmpeg` as WAV or MP3 (the
   only containers transcription accepts, see
   [models-and-privacy.md](./models-and-privacy.md#long-recordings)) and
   transcribes it in chunks, adding each chunk's offset to the turns the
   model returned so those timestamps line up with the recording.
   When the model returns no timestamps, each nonempty transcript line gets
   a positional timestamp: its chunk offset plus its line index in seconds,
   capped at the last second of that chunk. These fallback anchors are stable
   positions, not measured speech alignment. With an imported VTT it does nothing.
   Completed chunks are published as immutable transcript versions before the
   next request; an uncertain request consumes its slot and cannot be retried
   against that confirmation.
2. **`office.frames`** (mutating, `mcp_scope: none`) extracts the planned
   frames to `frames/` and reads each with Muse Spark image input: what is on
   screen (slide, code, terminal, dashboard, diagram, a face) and the visible
   text. It writes `frames.md`. A frame read as only a face writes one short
   line.
3. **`office.extract`** with `profile: kt`. One Muse Spark call over the
   merged timeline (transcript turns and frame readings in time order),
   with a JSON Schema for: chapters, gotchas, knowledge candidates (template,
   target page, claims), runbook candidates (inputs, steps, done when),
   glossary terms and questions. A timeline too long for one call is
   processed in 15-minute windows, then merged in one final call.
   - **Every claim must cite** an anchor that exists in this source.
     Uncited claims are dropped and counted in the step result, never kept.
   - Content inside a `tangent` chapter becomes knowledge only as a gotcha.
   - It writes `chapters.md` and the extract task artifact.
4. **Consolidate and file.** The knowledge candidates are written as the
   source's digest (`digest.json`, the shape in
   [library-bootstrap.md](./library-bootstrap.md#stage-1-a-digest-per-source-cheap)),
   and `office.consolidate` turns them into one proposal per page, merged
   with what other sources say about the same subjects and held to the page
   rules. A video does not propose pages on its own. One `owner:you` to-do
   per question, tagged `kt-question`.
5. **Hard cap.** No step makes more model calls than the confirmed plan
   counted. A step that would is refused with `plan_exceeded`, and the
   source keeps what was done so far.

The step results and the source's `video.calls` record the actual calls, so
Insights can show what a video cost against its estimate.

## Settings

New keys in `library/.office/config.yaml`, validated like the rest
([plugin-surface.md](./plugin-surface.md#library-settings-shape)):

```yaml
retention:  { video: 90 }          # days to keep original.* and frames/
sensitivity: { video: normal }
packs: [kt, meetings]
video:
  max_frames_per_hour: 120         # 1–600
  scene_threshold: 0.3             # 0.05–0.95
costs:                             # per-call rates for the preview; null = not shown
  currency: null                   # ISO 4217 code
  audio_minute: null
  image: null
  text_1k_tokens: null
```

## App

- **Upload:** a dropped video shows the plan preview and **Process**, with
  nothing spent until it is pressed.
- **Source view for a video:** the player, the chapter list with tangents
  folded and their gotchas shown, frame thumbnails on the timeline, and the
  transcript beside them. Once the planned source screen ships, each citation
  will open the player at its time; until then, the anchor has no landing
  behaviour.
- **Needs you:** the video's proposals as one group, "From the ingest KT:
  6 pages, 1 runbook", plus its questions in your to-dos.

## Day-1 checks

These gate the plan above:

- **Audio:** Muse Voice Transcribe's maximum audio length and request size
  per call, which set the chunk size, and whether it returns timestamps and
  speaker labels.
- **Context:** Muse Spark's context window. An hour of transcript plus about
  120 frame readings is roughly 25k tokens; if that doesn't fit, use the
  15-minute windows.
- **Images:** the request shape and size limit for image input.
- **Private route:** the model takes audio and images. What still needs checking is that the serving stack the demo uses (Baseten's model API) accepts `input_audio` and image parts in chat completions.
- **Prices:** the published per-call prices, to set the default `costs` the
  demo shows.

## Built tool contracts

`office.video-plan` takes `id`, optional `sensitivity: normal|private`, and,
for approval, both `confirm: true` and `digest`. A preview updates the source
sensitivity immediately. It returns `plan`, `confirmed_at`, actual `calls`
and `frames_read`. The plan includes selected frame timestamps in milliseconds,
scene scores in millionths, 64-bit difference hashes, per-group `counts` and
`routes`, `estimate`, and the missing `costs.*` keys as `rates_unset`. An unset
applicable rate or currency makes `estimate: null`; zero is a valid configured
rate. Private routes show the endpoint and model for audio, frames and extract.
An imported VTT removes the audio requirement; a source with no audio stream
also plans zero audio calls. No model is contacted during preview/confirmation.

Selection requires at least 10,000 milliseconds between frames and drops a
candidate whose difference hash is at Hamming distance five or less from the
previous kept frame. The cap is `floor(duration_hours * max_frames_per_hour)`,
with a minimum of one for short clips. When candidates exceed it, equal-duration
buckets keep their highest-scoring candidate (earlier wins a tie), followed by
another adjacent deduplication. The result may contain fewer than the cap.
Media inspection accepts positive durations up to seven days and refuses invalid,
overflowing or inconsistent ffprobe/ffmpeg metadata. Each local media process
uses the shared supervisor's 30-second deadline and bounded captures; raw media
and diagnostics never reach an error message. Both `ffmpeg` and `ffprobe` must
be available on the runtime PATH.

Audio counts use ten-minute chunks. Extraction reserves one call up to fifteen
minutes, otherwise one per fifteen-minute window plus a merge. The text cost is
an estimate: transcript characters divided by four (rounded up), 256 tokens per
frame, and 200 projected transcript tokens per audio minute when transcription
is needed; windowed processing doubles that token allowance for the merge.
The preview prices actual audio duration, frame count and those estimated text
tokens using the configured rates. These are planning units, not provider usage
measurements. Confirmation binds the original bytes, transcript, sensitivity,
selection, call counts, estimate and video/cost/private-route settings. A matching
repeat confirmation preserves the original confirmation time and call ledger.

`office.frames` takes only `id`; it returns `id`, the Library-relative `path` of
`frames.md`, `frames_read`, and actual `calls`. It refuses `plan_unconfirmed`
before extraction or inference and `plan_changed` when approved inputs differ.
Each image request is reserved durably first. An unsuccessful or interrupted
request consumes that slot: a retry refuses `plan_exceeded`, preserving completed
readings and preventing unapproved spending. A completed run is idempotent.
The image adapter checks the approved settings digest again immediately before
constructing its routed client, so a changed private endpoint cannot redirect a
confirmed call. Each successful frame writes a timestamp entry and `#frame=`
link, its 640-pixel PNG, the actual route/model in `derived_by.frames`, and the
updated call ledger. Missing rates, private routing and refusal/success goldens
are exercised offline through the public tool entry point.

The source's `video` mapping exposes `duration_s`, `plan_digest`, `confirmed_at`,
`calls` and `frames_read`; its detailed `plan` and `readings` are quoted JSON
scalars so they remain readable within the Library's bounded YAML subset.

`office.transcribe` accepts `id`, optional `language`, and `duration_seconds`
(required for a meeting, 1–604800). A video's duration comes exclusively from
its confirmed plan. It decodes each window, at most ten minutes, to mono 16-kHz
WAV through supervised ffmpeg, then makes exactly one Models request for that
window. Returned turns use their start time plus the window offset. The output
contains `id`, `path`, `version`, `noop` and `chunks`. Provenance records the
actual model and route, and the video ledger distinguishes consumed audio calls
from `audio_completed`. Extraction refuses incomplete audio even when the last
failed call consumed the final slot. Generated transcript hashes keep the
original confirmation usable for the following frame/extract steps.

`office.extract` accepts `id`, `profile: kt|meeting` and `task` (or the trusted
`context.task_id`) for the structured result artifact `extract/<source-id>.json`.
A KT profile requires completed planned transcription and frame readings. The
single-call budget is fifteen minutes of recording; longer recordings use all
15-minute windows, including empty ones, then exactly one merge call, matching
the video's plan. Transcript and frame paragraphs are merged by absolute time.
Every model request carries a closed JSON Schema. Chapters have `title`,
`label: core|tangent`, `start` and `end` in absolute seconds; candidates use
`template`, `target_page` and cited `claims`. Runbooks contain `inputs`, cited
`steps` and `done_when`. Glossary items use `term`, `definition` and `cites`.
Questions and gotchas use `text` and `cites`.

`dropped` counts `uncited`, `dangling` and `tangent` items. Citations must name
supplied source turn/frame anchors; any invalid citation drops the whole item.
Tangent claims, runbook steps and terms are excluded; cited gotchas remain.
The source receives `chapters.md`, the bootstrap-compatible `digest.json`,
actual extraction provenance and `extract-result.json`. Completed window replies
are retained in `extract-window-<index>.json` if a later request fails. Each
question files an `owner:you`, `source:<id>`, `kt-question` to-do; this tool files
no knowledge proposal. A meeting instead produces `summary`, cited `decisions`
and `to_dos` with `owner: you|agent`; filed tasks carry `owner:<owner>` and
`source:<id>` tags. Both profiles attach the full structured output to `task`.
