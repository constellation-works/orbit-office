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
   Timestamps exist only where the model returns turns: a chunk whose
   reply returns none contributes text with no timestamps. With a VTT
   it does nothing.
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
  transcript beside them. Every citation link opens the player at its time.
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
