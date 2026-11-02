---
type: design
summary: "Spec: models and privacy routing — which model does what, and the private route: Meta's open-weights Muse Spark Open on an endpoint the user chooses"
last_validated: 2026-11-02
---

# Spec: Models and privacy routing

Only inference leaves the user's machine. Which model it goes to depends
on what the input contains: the hosted Meta Model API by default, or the
**private route**, Meta's open-weights Muse Spark Open on an endpoint the
user chooses ([below](#the-private-route)).

Source: spec §9.

Every model the plugin calls at runtime is a Meta model. Hosted calls go to
the Meta Model API at `https://api.meta.ai/v1`; `private` material goes only to
Muse Spark Open on the endpoint the user configured.

| Use | Model (id) | Where it runs | Notes |
|---|---|---|---|
| Orchestrator | Muse Code (default) running Muse Spark or Muse Wonder | The user's Muse Code client, against Meta's hosted models | Any MCP-capable agent CLI works. The orchestrator plus agents loop is the candidate for the agentic bonus prize |
| Video and meeting transcription | Muse Voice Transcribe (`muse-voice-transcribe-1.0`) | Meta Model API, `POST /v1/asr/transcribe` | In chunks of at most 10 minutes, decoded locally to 16 kHz mono WAV by ffmpeg (M4A, MP4 and other containers included); our own offsets are applied to returned turns; timestamps exist only where the model returns turns; skipped when a `.vtt` exists |
| Bootstrap digest, one per source | Muse Spark (`muse-spark-1.3`) | Meta Model API, `POST /v1/chat/completions` | The cheap pass ([library-bootstrap.md](./library-bootstrap.md)) |
| Bootstrap consolidation, one per subject | Muse Wonder (the `careful` tier, `models.careful`, default `muse-wonder`) | Meta Model API, `POST /v1/chat/completions` | Merges claims across sources and flags conflicts. The API lists `muse-wonder-1.0`; the default slug `muse-wonder` is not yet verified live ([below](#live-verification-2026-11-02)) |
| KT keyframe reading | Muse Spark (`muse-spark-1.3`) with image input | Meta Model API, `POST /v1/chat/completions` | One call per deduplicated keyframe, capped per hour ([kt-videos.md](./kt-videos.md)) |
| KT chapters, knowledge candidates, questions | Muse Spark (`muse-spark-1.3`) | Meta Model API, `POST /v1/chat/completions` | One call over the merged timeline, or 15-minute windows and a merge |
| Summaries, decisions, to-do extraction | Muse Spark (`muse-spark-1.3`) | Meta Model API, `POST /v1/chat/completions` | Structured output validated against a JSON Schema |
| Daily digest | Muse Spark (`muse-spark-1.3`), one call a day | Meta Model API, `POST /v1/chat/completions` | Through the same routing; private items go to open weights or are listed by reference ([sessions.md](./sessions.md#daily-digest)) |
| Receipt and bill field extraction | Muse Spark (`muse-spark-1.3`) with image input | Meta Model API, `POST /v1/chat/completions` | Fallback: local OCR, then Muse Spark on the text |
| Anything `private`, every use above | Muse Spark Open (`open_weights.model`, for example `meta-models/Muse-Spark-Open-816B-A42B`) | The endpoint the user chose ([below](#the-private-route)), OpenAI-compatible `POST <endpoint>/chat/completions` | One model for every tier and modality: text, image and audio in, text out. Audio goes as a `wav` or `mp3` `input_audio` part. Candidate for the open-weights bonus prize |
| Implementing agents | Orbit's existing crews, including its Muse crews (Muse Code) | Orbit hosts, against Meta's hosted models | Pre-existing: not counted as new Meta-tooling work |

## Routing rules

- **Sensitivity.** Set per source (default by kind, for example `email` → `private`) or by the user. Any model call whose input includes a `private` source goes to the open-weight model. The routing decision is logged in the source's `derived_by` and in the task.
- **Model hosts.** Hosted calls go only to `api.meta.ai`. Private calls go only to the one open-weights endpoint the user configured ([below](#the-private-route)): its exact host, over HTTPS unless it is loopback. Any other host is refused when Library settings load, and again before a socket opens.
- **Redirects.** The model HTTP client does not follow redirects. Each request therefore stays on the host checked by the model endpoint allowlist before the socket opens.
- **Artifacts inherit sensitivity:** an artifact's `sensitivity` is the strictest of what it cites ([library.md](./library.md)).
- **Selections from private objects carry no quote** ([selection-sharing.md](./selection-sharing.md)); the agent re-reads through the tools so the routing applies.
- **Secrets.** The Meta API key and the private route's token (`open_weights_token`, bound to the endpoint origin it was saved for; [below](#the-private-routes-token)) are stored with `orbit plugin secret set office <name>`, never in config or the Library. Knowledge pages never hold secrets; `refs` name files, links, contacts, accounts and artifacts only.
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

### The private route's token

A token is bound to the endpoint **origin** (scheme, host and port) it was
saved for, and is sent to no other origin:

- **Stored as one record.** The secret's value is the JSON record
  `{"origin": "https://muse.internal.acme.example", "token": "<token>"}`,
  written with one `orbit plugin secret set office open_weights_token` call
  on stdin. The token and its origin change together or not at all. The
  binding lives with the credential, not in Library config:
  `office.config-set` cannot write a secret, so changing the endpoint
  (from Settings or directly) cannot move a token to a new origin.
- **Checked on every call.** Each model call, `office.private-route-check`
  included, reads the endpoint and the record once and sends the token as
  `Authorization: Bearer` only when the endpoint's origin equals the bound
  origin; otherwise the request carries no `Authorization` header. A
  changed endpoint, a failed token write, an interleaved save or a
  concurrent call can only leave a call unauthenticated, never pair a token
  with an origin it was not saved for.
- **Reuse.** Leaving the token empty in Settings keeps the saved token for
  an unchanged origin (another path on the same origin counts as
  unchanged). Saving an endpoint on another origin with the token empty
  makes it an unauthenticated endpoint: the old token stays saved for its
  origin and is not sent. *Remove the saved token* (`clear_token`) runs
  `orbit plugin secret rm office open_weights_token`.
- **Unbound values are never sent.** A value that is not that record (for
  example a bare token typed at the installer's prompt or into
  `orbit plugin secret set`) is not tied to an origin, so no call sends it.
  Enter the token in Settings, or store the record yourself with
  `orbit plugin secret set office open_weights_token` (the prompt does not
  echo).
- **Status.** `office.status` reports the token as `set` or `unset` under
  `secrets`, and `open_weights_token_binding` as `this_endpoint`,
  `other_endpoint`, `unbound` or `unset`; never the token or its origin.
  Settings words the token field from it.

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
  to the hosted route.
- **Read tools withhold private bodies.** The MCP-visible read tools
  (`office.source-show`, `office.search`, `office.knowledge-show`,
  `office.artifact-show`) return metadata only for a `private` document —
  id, kind, title, path, sensitivity, provenance and anchors as
  identifiers — marked `content_withheld: true`. The source body and span,
  its summary sidecar and chapter/frame/gotcha text, the search snippet,
  the page body and the artifact body never reach any caller of these
  tools, because the orchestrator's hosted model would otherwise receive
  private text on every read ([decision](../4_decisions.md#private-content-is-withheld-from-mcp-visible-read-tools)).
  This is the same rule selections and `office.attach` already apply.
  Working on private content goes through the model tools, which route
  it to the private endpoint; no private-route summary is substituted.
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
parsed JSON reply locally on both the hosted and open-weights routes. The hosted
request sends the schema with `strict: false`: the Meta Model API's strict mode
refuses any schema whose `required` list omits a property (the meeting and
digest schemas have optional fields), so the schema guides generation and local
validation enforces it. Non-JSON
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
Transcription accepts WAV and MP3. `audio/wav`, `audio/wave` and `audio/x-wav`
are WAV; `audio/mpeg` and `audio/mp3` are MP3. Every other MIME type (M4A/MP4,
AAC, FLAC, Ogg, WebM) is refused by the Models port before staging, splitting
or any model call, at every length. `office.transcribe` never hits that
refusal: it decodes each window of a meeting or video original locally with
ffmpeg (`Models::media_audio`: 16 kHz, mono, 16-bit PCM WAV) before
transcribing, so meetings recorded as M4A or MP4 are transcribed like WAV.

At or below the threshold, transcription sends the original bytes directly and
runs no subprocess. Above it, the adapter stages the recording privately and
uses the supervised splitter, which keeps the WAV or MP3 container.

**Hosted request.** Each hosted chunk is one `POST /v1/asr/transcribe` to Muse
Voice Transcribe: a multipart body with a `request` part (JSON
`{"model":"muse-voice-transcribe-1.0","mode":"DIARIZATION","audioEncoding":"WAV"}`,
plus `"languageBias":[<language>]` when the caller gave a language hint) and an
`audio` part holding the WAV. An MP3 chunk is decoded to WAV locally first. The
reply's `transcript` is the chunk text; each `turns` entry with `startMs` and a
non-empty `transcript` is a speaker turn (`speaker` when diarization labels
one), ending at its `endMs`, else the next turn's start, else the end of the
audio. The one-shot endpoint takes at most 10 minutes or 32 MB per request,
which the 600-second chunks stay under.

**Private request.** On the private route each chunk is one Chat Completions
call with the audio as an `input_audio` part in its own `wav` or `mp3` format,
asking for a JSON object `{"text","segments"}`.

Every chunk passes through the same sensitivity routing with the original
language hint. Private chunks reach only the configured open-weights endpoint.
The private request asks for one segment per speaker turn (`speaker`,
`start_ms`, `end_ms`, `text`, milliseconds from the start of that chunk); a
private reply that is not that JSON is kept as transcript text with no turns.
On both routes, the reported turns are kept and each chunk's timestamps are
shifted by the chunk's start so they stay offsets into the original recording. Chunk texts are joined with newlines in
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

## Live verification (2026-11-02)

Run against `api.meta.ai` with the office plugin's `meta_api_key`, on a scratch
Library holding synthetic content only (ORB-21338):

| Check | Request | Result |
|---|---|---|
| Model catalogue | `GET /v1/models` | 200 in under 1 s. Lists `muse-voice-transcribe-1.0`, `muse-spark-1.3`, `muse-spark-1.2`, `muse-spark-1.1`, `muse-wonder-1.0`, `muse-wonder-full-1.0`, `muse-image-1.0`, `sam-3.1` and `-contributor` variants |
| Meeting-profile extraction, strict schema | `POST /v1/chat/completions`, `muse-spark-1.3`, `json_schema` `strict: true` | 400 `invalid_request_error`: strict mode needs every property in `required` (the meeting schema's optional `volatile`). Fixed by sending `strict: false` |
| Meeting-profile extraction, non-strict | Same, `strict: false` | Passes request validation; then 403 for the key ([below](#the-api-key-must-be-pay-as-you-go)) |
| Transcription on Chat Completions | `muse-spark-1.3` with an `input_audio` part | 403 for the key |
| Muse Voice Transcribe on Chat Completions | `muse-voice-transcribe-1.0` with an `input_audio` part | 404 `model_not_found`: the model is not served there |
| OpenAI-style transcription | `POST /v1/audio/transcriptions` | 404: no such route |

Muse Code's own `muse voice transcribe` (1.4.14) does not use this API. It
streams 16 kHz PCM over a WebSocket speech service with a credential minted from
the Muse Code login, so its shape is not usable from the plugin.

### The API key must be pay-as-you-go

Every inference request with the key stored on 2026-11-01 was refused with HTTP
403 `client_attestation_denied`: the key is billed to a Muse Code subscription,
which covers requests from the Muse Code client only. Hosted extraction,
transcription and the digest need a pay-as-you-go Meta Model API key, stored
with `orbit plugin secret set office meta_api_key`. Until one is set, a live
transcription result (model id, latency) and schema-valid meeting JSON remain
unverified; the request shapes above are what was confirmed.

