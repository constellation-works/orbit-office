---
type: design
summary: "Spec: models and privacy routing — which model does what, and the private route: Meta's open-weights Muse Spark Open on an endpoint the user chooses"
last_validated: 2026-10-26
---

# Spec: Models and privacy routing

Only inference leaves the user's machine. Which model it goes to depends
on what the input contains: the hosted Meta Model API by default, or the
**private route**, Meta's open-weights Muse Spark Open on an endpoint the
user chooses ([below](#the-private-route)).

Source: spec §9.

| Use | Model | Notes |
|---|---|---|
| Orchestrator | Muse Code (default) running Muse Spark or Muse Wonder | Any MCP-capable agent CLI works. The orchestrator plus agents loop is the candidate for the agentic bonus prize |
| Video and meeting transcription | Muse Voice Transcribe | In chunks with our own offsets applied to returned turns; timestamps exist only where the model returns turns; skipped when a `.vtt` exists |
| Bootstrap digest, one per source | Muse Spark | The cheap pass ([library-bootstrap.md](./library-bootstrap.md)) |
| Bootstrap consolidation, one per subject | Muse Wonder (the `careful` tier, `models.careful`) | Merges claims across sources and flags conflicts; day-1 check that it is on the API |
| KT keyframe reading | Muse Spark with image input | One call per deduplicated keyframe, capped per hour ([kt-videos.md](./kt-videos.md)) |
| KT chapters, knowledge candidates, questions | Muse Spark | One call over the merged timeline, or 15-minute windows and a merge |
| Summaries, decisions, to-do extraction | Muse Spark | Structured output validated against a JSON Schema |
| Daily digest | Muse Spark, one call a day | Through the same routing; private items go to open weights or are listed by reference ([sessions.md](./sessions.md#daily-digest)) |
| Receipt and bill field extraction | Muse Spark with image input | Confirm the request shape in the API reference on day 1. Fallback: local OCR, then Muse Spark on the text |
| Anything `private`, every use above | Muse Spark Open on the private route ([below](#the-private-route)) | One model for every tier and modality: text, image and audio in, text out. Candidate for the open-weights bonus prize |
| Implementing agents | Orbit's existing crews, including its Muse crews | Pre-existing: not counted as new Meta-tooling work |

## Routing rules

- **Sensitivity.** Set per source (default by kind, for example `email` → `private`) or by the user. Any model call whose input includes a `private` source goes to the open-weight model. The routing decision is logged in the source's `derived_by` and in the task.
- **Model hosts.** Hosted calls go only to `api.meta.ai`. Private calls go only to the one open-weights endpoint the user configured ([below](#the-private-route)): its exact host, over HTTPS unless it is loopback. Any other host is refused when Library settings load, and again before a socket opens.
- **Redirects.** The model HTTP client does not follow redirects. Each request therefore stays on the host checked by the model endpoint allowlist before the socket opens.
- **Artifacts inherit sensitivity:** an artifact's `sensitivity` is the strictest of what it cites ([library.md](./library.md)).
- **Selections from private objects carry no quote** ([selection-sharing.md](./selection-sharing.md)); the agent re-reads through the tools so the routing applies.
- **Secrets.** The Meta API key and the private route's token (`open_weights_token`) are stored with `orbit plugin secret set office <name>`, never in config or the Library. Knowledge pages never hold secrets; `refs` name files, links, contacts, accounts and artifacts only.
- **Contact details never reach a model.** Model inputs and task attachments carry a page's model view, in which each `contact` ref keeps its label and loses its value; page bodies may not contain email addresses or phone numbers ([knowledge-templates.md](./knowledge-templates.md#contact-details)).
- **Other people's words.** Pasted chats contain third parties' messages. They stay local unless the user marks the source shareable, which sends them only to the team's shared library ([shared-library.md](./shared-library.md)). The demo uses synthetic content only.

The plugin's network grant is `any` (the only setting that reaches remote
hosts), but the code calls only `api.meta.ai` (hosted) and the one
configured private endpoint. Its `git` child reaches only the shared
library's configured remote, carrying only documents marked `shareable`
([shared-library.md](./shared-library.md#transport)). Any other host is refused before a socket
opens. That restriction is a code rule, reviewed, not a sandbox setting.

## The private route

Muse Spark Open is Meta's open-weights model: 816B parameters with 42B
active per token, text, image, video and audio in, text out, a 1M-token
context, and BF16 and NVFP4 weights. It needs at least about 700 GB of GPU
memory, so it **does not run on a laptop**. The private route is therefore
an endpoint the user chooses, never "on your computer" by default
([decision](../4_decisions.md#the-private-route-is-muse-spark-open-on-an-endpoint-the-user-chooses)).

### Endpoint choices

Set up once in Settings. Each choice is labelled with where data goes, and
that label is shown wherever the route is used:

| Choice | Label in the app | Where private material goes |
|---|---|---|
| **Your company's deployment** (vLLM or SGLang on its own GPUs, or in its cloud account) | "Private: Acme's server" | Your employer's infrastructure. The strongest option for work material. |
| **A hosted provider** (for example Baseten's model API) | "Private: Baseten (third party)" | That provider, under its terms. Not Meta's hosted API, but not your company either. |
| **Your own hardware** (for example two M5 Ultra Mac Studios with MLX, on loopback or your LAN) | "Private: your hardware" | Machines you own. |

Settings keys (`library/.office/config.yaml`, validated on every read):

```yaml
open_weights:
  endpoint: https://muse.internal.acme.example/v1   # OpenAI-compatible base URL
  model: meta-models/Muse-Spark-Open-816B-A42B       # as the server names it
  kind: company            # company | provider | own_hardware
  label: Acme's server     # shown in the app
  capabilities: [text, image, audio]   # written by the check below, never typed in
  verified_at: 2026-10-27T10:02:00-04:00
```

The endpoint's token, when it needs one, is the declared secret
`open_weights_token`. The token replaces `baseten_token`; for one
release the old installer inputs and the old status field stay accepted
as deprecated aliases with a warning (see the deviation recorded in
[4_decisions.md](../4_decisions.md), STD-01@2 §R10 and §R35).

### Honest wording

- **"Private" means "not sent to Meta's hosted API; sent only to the
  endpoint you chose".** It never means "never leaves your machine". The
  Settings screen, the README and the pitch all say exactly that.
- Every model call records its route in `derived_by` (`hosted` or
  `private:<kind>`), and the source view shows the badge with the endpoint's
  label. For `kind: provider`, the source badge appends “(third party)”
  independently of the user-chosen endpoint label.

### Checking an endpoint: `office.private-route-check`

Mutating, `mcp_scope: none`; the app calls it when the user saves the
endpoint. It sends three tiny requests (a sentence, a 1×1 image and a
1-second silent WAV) and records which ones work as `capabilities`, with
`verified_at`. It reports the endpoint's model name and the time each
request took. Nothing is routed to an endpoint that has not passed the text
check. A failed modality is shown in plain words ("this server doesn't take
audio, so private videos can't be transcribed").

Refusals on the private route: `private_endpoint_unconfigured` (no
`open_weights` block), `private_endpoint_unverified` (no check has passed the
text request), `private_media_unsupported` (the call needs image or audio and
the check did not record it; the message names which) and, from the check
itself, `private_route_changed` (the endpoint or model changed while it ran, so
nothing was recorded). The Models port returns each result with its
`derived_by` (`route` and `model`), which the calling tool writes to the
source's `derived_by`.

### What private changes

- **Sources, batches and videos.** Each source can be marked private in the
  source view, and a whole bootstrap batch or video plan can be marked
  private at its preview. That sets every source in it.
  ([library-bootstrap.md](./library-bootstrap.md#batch-preview-and-confirmation),
  [kt-videos.md](./kt-videos.md#2-plan-and-confirm-officevideo-plan)).
- **Source media reads.** The source view marks an existing source private
  through `office.source-set-sensitivity`, hidden from MCP; private-to-normal
  is refused. `office.source-show`'s optional original/frame payload checks the
  latest sensitivity and settings under a shared writer lock before reading
  bytes. A private source needs a configured endpoint
  (`private_endpoint_unconfigured`) with a recorded verification timestamp and
  text capability (`private_endpoint_unverified`). A frame/image also needs
  `image`, audio needs `audio`, and a video original conservatively needs both;
  a missing modality returns `private_media_unsupported` naming it. Private
  text originals need text only; other original MIME types are refused as
  unsupported. These reads make no model or network call and never fall back
  to the hosted route. Source text display without a media request retains
  its existing behavior.
- **Every tier and modality** goes to the one private model: digest,
  careful consolidation, frame reading and transcription. A private KT video
  is refused at planning only when the endpoint lacks a capability it needs
  (`private_media_unsupported`, naming which).
- **The preview shows the route per call group**: for example "112 calls,
  all on Private: Acme's server".
- **No endpoint configured:** private material is refused, never sent to
  the hosted API. The digest lists private items by reference.

### Attribution

The Muse Model License asks products built with the model to show "Built
with Muse Spark Open" prominently. The app shows it in its footer and in
Settings whenever a private endpoint is configured, and the README says it.
The app learns that from `office.status`: `open_weights` is the public block,
or null when nothing is configured, and the attribution is shown only then.
The commercial terms of the license (an agreement with Meta for a
hosted-model service with over $20M in yearly revenue) do not apply to a
user's own use.

## Structured extraction

`Models::extract` compiles `request.schema` before inference and validates the
parsed JSON reply locally on both the hosted and open-weights routes. Non-JSON
or schema-violating replies return non-retryable `model_invalid_response`.
The schema sent to the endpoint guides generation; local validation enforces
the contract even when the endpoint does not enforce `response_format`.

Schemas must be self-contained: local references such as `#/$defs/item` work,
but validation never retrieves HTTP URLs or files. An invalid schema or an
unresolvable reference returns non-retryable `model_invalid_schema` before
inference. Validation diagnostics contain neither schema nor reply contents.

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
recording order. The transcript also keeps each chunk in that order: a chunk
that reported turns contributes those turns, and a chunk that reported none
contributes its text. The meeting body follows that order, so a text-only
chunk stays between its neighbors instead of being dropped when any other
chunk reported turns. A single reply is one chunk. Any split or model failure
fails the transcription instead of returning a partial success. Temporary
audio is cleaned up on success and failure.

`ExtractRequest::tier` distinguishes `Standard` extraction from `Careful`
synthesis. Existing extraction callers select `Standard`. Hosted careful calls
use the Library setting `models.careful`, default `muse-wonder`; it accepts a
nonempty ASCII model slug up to 128 bytes (letters, digits and `-._/:`). Set it
with `office.config-set`. This setting changes the model, never the endpoint or
credentials. Private calls at both tiers still use only `open_weights.model`
and require its verified text capability. Changed careful settings invalidate
confirmed batch and per-call routing snapshots. The default setting preserves
existing plan fingerprints. Offline tests verify the selected slug; live Muse
Wonder availability and context capacity remain the Day-1 check above.
