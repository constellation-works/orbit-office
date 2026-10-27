---
type: design
summary: "Spec: models and privacy routing — which model does what, and how private sources stay on open weights"
last_validated: 2026-10-26
---

# Spec: Models and privacy routing

Only inference leaves the user's machine. Which model it goes to depends
on what the input contains.

Source: spec §9.

| Use | Model | Notes |
|---|---|---|
| Orchestrator | Muse Code (default) running Muse Spark or Muse Wonder | Any MCP-capable agent CLI works. The orchestrator plus agents loop is the candidate for the agentic bonus prize |
| Video and meeting transcription | Muse Voice Transcribe | In chunks with our own offsets, so anchors hold without model timestamps; skipped when a `.vtt` exists |
| KT keyframe reading | Muse Spark with image input | One call per deduplicated keyframe, capped per hour ([kt-videos.md](./kt-videos.md)) |
| KT chapters, knowledge candidates, questions | Muse Spark | One call over the merged timeline, or 15-minute windows and a merge |
| Summaries, decisions, to-do extraction | Muse Spark | Structured output validated against a JSON Schema |
| Daily digest | Muse Spark, one call a day | Through the same routing; private items go to open weights or are listed by reference ([sessions.md](./sessions.md#daily-digest)) |
| Receipt and bill field extraction | Muse Spark with image input | Confirm the request shape in the API reference on day 1. Fallback: local OCR, then Muse Spark on the text |
| Sources marked `private` | Muse Spark Open (`meta-models/muse-spark-open-1026` on Baseten, or local loopback) | Candidate for the open-weights bonus prize |
| Implementing agents | Orbit's existing crews, including its Muse crews | Pre-existing: not counted as new Meta-tooling work |

## Routing rules

- **Sensitivity.** Set per source (default by kind, for example `email` → `private`) or by the user. Any model call whose input includes a `private` source goes to the open-weight model. The routing decision is logged in the source's `derived_by` and in the task.
- **Open-weights hosts.** A configured open-weights endpoint is a Baseten HTTPS URL (`inference.baseten.co`, or a host ending in `.api.baseten.co`) or a loopback HTTP(S) URL (`localhost`, `127.0.0.1`, `::1`). That is the open-weights subset of the model host allowlist. Hosted calls may also use `api.meta.ai`; that host is not an open-weights endpoint. Any other host is refused when Library settings load, and again before a socket opens. Non-loopback endpoints must use HTTPS.
- **Artifacts inherit sensitivity:** an artifact's `sensitivity` is the strictest of what it cites ([library.md](./library.md)).
- **Selections from private objects carry no quote** ([selection-sharing.md](./selection-sharing.md)); the agent re-reads through the tools so the routing applies.
- **Secrets.** The Meta API key and Baseten token are stored with `orbit plugin secret set office <name>`, never in config or the Library. Knowledge pages never hold secrets; `refs` name files, links, contacts, accounts and artifacts only.
- **Contact details never reach a model.** Model inputs and task attachments carry a page's model view, in which each `contact` ref keeps its label and loses its value; page bodies may not contain email addresses or phone numbers ([knowledge-templates.md](./knowledge-templates.md#contact-details)).
- **Other people's words.** Pasted chats contain third parties' messages. They stay local, and the demo uses synthetic content only.

The plugin's network grant is `any` (the only setting that reaches remote
hosts), but the code calls only `api.meta.ai` (hosted), a Baseten host
(`inference.baseten.co` or `*.api.baseten.co`), or loopback
(`localhost`, `127.0.0.1`, `::1`) for a configured local open-weights
server. An open-weights endpoint is the Baseten or loopback subset; the
hosted Meta host is not one. Any other host is refused before a socket
opens. That restriction is a code rule, reviewed, not a sandbox setting.

## Long recordings

The Models port's `TranscribeRequest.duration_seconds` carries duration from
the caller's media metadata, rounding fractional seconds up. The adapter does
not probe audio. `ModelConfig.audio_splitter` configures the ffmpeg executable,
split threshold and maximum chunk length (both lengths default to 600 seconds).
Transcription accepts WAV and MP3 only. `audio/wav`, `audio/wave` and
`audio/x-wav` are sent as `wav`; `audio/mpeg` and `audio/mp3` are sent as `mp3`.
The model schema takes no other `input_audio` format, so every other MIME type
(M4A/MP4, AAC, FLAC, Ogg, WebM) is refused before staging, splitting or any model
call, at every length. The adapter does not transcode: sources in other
containers are converted to WAV or MP3 before transcription.

At or below the threshold, transcription sends the original bytes directly and
runs no subprocess. Above it, the adapter stages the recording privately and
uses the supervised splitter, which keeps the WAV or MP3 container.

Every chunk passes through the same sensitivity routing with the original
language hint. Private chunks reach only the configured open-weights endpoint.
The request asks for a JSON object `{"text","segments"}`, one segment per
speaker turn (`speaker`, `start_ms`, `end_ms`, `text`, milliseconds from the
start of that chunk). When the reply is that object, those turns are kept
and each chunk's timestamps are shifted by the chunk's start so they stay
offsets into the original recording. A reply that is not that JSON is kept
as transcript text with no turns. Chunk texts are joined with newlines in
recording order. Any split or model failure fails the transcription instead
of returning a partial success. Temporary audio is cleaned up on success
and failure.
