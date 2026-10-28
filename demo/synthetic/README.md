# Synthetic demo data

Everything in this directory is invented. Acme Paper Kites, Lantern,
Jules Reed, Mira Vale, Nori Chen, the access process, PTO policy, receipts,
token and endpoints are synthetic. No exports from real accounts or other
repositories are used. Every source title and origin says SIMULATED.

The pack contains six weeks (2026-09-14 through 2026-10-25) of meetings,
chats, receipts and work receipts; four About pages; onboarding documents;
a loopback Acme CRM; and a scripted 15-minute KT video. The last seeded
history record is a weekly recap on 2026-10-25. `demo.simulated_until` is set to that date;
Insights labels those six weeks simulated and leaves the following week
unlabelled. The receipts demonstrate the UI, not real productivity claims.

## Run

Run from the repository root on macOS with Python 3.12+, Swift command-line
tools, FFmpeg (including ffprobe and libx264), and the installed Samantha and
Daniel `say` voices. No package installation or global configuration is
performed. If Poppler is missing, a private argv-compatible `pdftotext`
adapter uses PDFKit to extract the actual bundled PDF. Frame rendering uses
AppKit, so an FFmpeg build without drawtext works.

```sh
cargo build --locked -p office
python3 demo/synthetic/video.py --output .orbit/tmp/synthetic-kt
python3 demo/synthetic/seed.py --root .orbit/tmp/synthetic-demo --video .orbit/tmp/synthetic-kt/lantern-kt.mp4
```

Every output root must be new. Existing directories, including previously
seeded Libraries, are refused. Outputs stay in the supplied root; use
`.orbit/tmp/` for runs and evidence. The video, voice files, rendered frames,
Library, task records and generated caches are never committed.

`office.init` creates the Library. The script drives the built Office exec
entry point and the real Gateway handlers with v1 envelopes. It stages
uploads beside the Library and calls `office.capture`; it never copies
records into the Library. All four About pages name bundled templates and
come from accepted `knowledge_add` proposals citing captured interview
facts. Document digests, confirmed batch plans, consolidation proposals and
their acceptances create pages by subject. A separate accepted update makes
the AD-group runbook explicitly `performable: agent` with `submission.via:
draft`. The access draft has a source, cited runbook, review task and task
artifact. Nothing is submitted.

The callback fixture is deliberately isolated: `ORBIT_BIN` points only the
demo children at `orbit_fixture.py`, with a cleared environment and a fresh
home. It is not the installed Orbit CLI, never registers a workspace, and
cannot dispatch work. Synthetic proposal/work/task records live in
`demo-orbit.json`. This makes a rehearsal safe on a shared host and lets the
actual app render its queue without altering host tasks.

## Determinism and behavior checks

```sh
python3 demo/synthetic/verify.py --output .orbit/tmp/synthetic-check --video .orbit/tmp/synthetic-kt/lantern-kt.mp4
python3 demo/synthetic/compare.py .orbit/tmp/synthetic-check/left .orbit/tmp/synthetic-check/right
```

Verification seeds two complete fresh Libraries sequentially and checks
About proposal receipts, citations, bootstrap proposals, the performable
runbook, gateway redaction, the pending PATCH, private KT provenance and the
simulated cutoff. It also mutates page content and links to prove comparison
detects drift, then restores the file and checks equality again. Re-seeding
an existing root must fail.

Comparison covers every Library file outside `.git`, including source
revisions, originals, templates, configuration, pages, proposals and frame
bytes, plus the ordered Git commit messages. It normalizes source IDs,
dates and commit identities. Plan/approval fingerprints derived from those
inputs are represented by their field names; all underlying content and
structure remain compared. It does not discard citations, prose, links,
call counts or binary bytes. It compares seeded content, not byte-identical
Git object databases or rebuildable plugin-state indexes.

`documents.py` deterministically regenerates the committed PPTX and PDF:

```sh
python3 demo/synthetic/documents.py
```

The video generator scripts two alternating narrators and 15 scenes (slides
and rendered terminal sessions), with two tangents and one useful gotcha
inside the second tangent at 08:30. No shell command displayed in a frame is
executed. Audio is padded to 30 seconds per turn and FFmpeg assembles exactly
900 seconds. A final remux fixes MP4 chunk interleaving after encoding;
identical packet streams otherwise can produce different container bytes.
`manifest.json` records SHA-256, macOS version, voices, font and native-helper
hashes and FFmpeg version. Determinism is within that recorded toolchain: changing
macOS voice assets, font rendering or FFmpeg can change bytes. Run the
generator twice and compare manifest SHA-256 values when refreshing evidence.

## Shared library: two workspaces

```sh
cargo build --locked -p office
python3 demo/synthetic/shared_library.py --root .orbit/tmp/shared-demo
```

Needs only Python 3, git and the built binary (Linux or macOS). The new root
holds a bare `team.git` and two workspaces, `alice/` and `joiner/`, each with
its own fresh home whose `.gitconfig` carries a SIMULATED identity; Office's
git children read only that home. Alice captures a time-coded Lantern KT
transcript, writes a topic page and a runbook citing it plus a scratch page
she never shares, marks the transcript and both pages shareable, previews
`office.library-sync` with `dry_run`, syncs, and syncs again (nothing is
pushed). The joiner connects an empty Library, syncs, and answers "What do I
do when the Lantern spool stalls?" from Alice's runbook; its
`alice/src-…#t=00:01:10` citations open Alice's transcript at the second.
Each step is checked; `shared-demo.json` records the answer page, its
citations and both sync results, and `<workspace>/calls.json` every call.
Nothing leaves the root and no Orbit task is created.

## CRM and pending approval

The seed owns and stops its loopback CRM on port 4329 and simulated inference
endpoint on port 4330. It refuses if a port is occupied. They are never
backgrounded past the seed command. The connection is created through
`gateway.connection-put`, restricted to direct GET and drafted PATCH.
`gateway.http-get` echoes the fake token as `[secret]` after gateway
redaction. `gateway.http-draft` files a pending PATCH; the seed asserts zero
PATCH requests. No send tool is called. The public fake token in `common.py`
is not a credential.

To inspect the mock independently in the foreground:

```sh
python3 demo/synthetic/mock_crm.py --port 4329
```

Stop it before running the seed. Its only GET route is
`/v1/accounts/acme`; it requires the fake token. PATCH always refuses.

## Private routing and the live-provider boundary

The default endpoint is an **authored loopback inference fixture**, labelled
SIMULATED. It returns deterministic scripted answers. The actual tools make
the capability probes, private transcription, frame reading and extraction
HTTP calls; their results record `private:provider`. This verifies plumbing,
not model quality or processing by a hosted provider.

The required demo check uses this loopback endpoint configured with
`kind: provider`. Verification checks that the video is private, the
transcription, frame reading and extraction use `private:provider`, and the
actual app displays the third-party label and attribution. This is a
scripted integration check; it does not measure model quality.

The operator's demo-time real-provider check uses an explicitly supplied HTTPS
endpoint and, if required, its token. Create a private JSON configuration
outside the repository with these keys (reserved example host shown):

```json
{
  "endpoint": "https://inference.acme.example/v1",
  "model": "meta-models/Muse-Spark-Open-816B-A42B",
  "kind": "provider",
  "label": "Selected provider",
  "token_file": "/absolute/private/path/to/token"
}
```

```sh
python3 demo/synthetic/seed.py --root .orbit/tmp/synthetic-live --video .orbit/tmp/synthetic-kt/lantern-kt.mp4 --provider-config .orbit/tmp/provider.json
```

Only the synthetic KT video uses that provider; onboarding uses the local
fixture. The token is read into the call context, never input, argv, Library
config or evidence logs. The endpoint must pass `office.private-route-check`
before video planning. The app labels provider routes third party and shows
“Built with Muse Spark Open”. `hosted_provider_verified` in the seed manifest
is false for every fixture run. Never cite a fixture run as hosted evidence.
Live model answers need not be byte-deterministic; the repeatability check
uses the authored fixture.

## Actual app screenshots

Build the app and source the host's verified browser kit, then run:

```sh
make app-deps
npm --prefix app run build
. "$ORBIT_BROWSER_KIT_ENV"
node demo/synthetic/screenshots.mjs .orbit/tmp/synthetic-demo .orbit/tmp/synthetic-screenshots
```

This starts the actual application router and compiled web UI with only the
synthetic CLI fixture on the server's PATH. It serves reads only, captures
Insights, Library, Source, Needs you and To-dos at 1280px and 375px in both color schemes,
and closes/reaps the server and browser before returning. The app currently
supports a fixed dark theme; both browser color preferences are captured.
No host Orbit
records are read. Source route badges and the attribution footer come from
the real seeded configuration. The screenshot manifest identifies the
simulated inference boundary.

The current Source view limits inline originals to 512 KiB. The generated
15-minute original exceeds that limit: it is processed by the tools and its
transcript, chapters, gotcha and keyframes render, but the app cannot play
the full original inline. The screenshot check waits for the player's
settled state and records that limitation rather than claiming playback.


`calendar.ics` is also synthetic: Lantern's weekly checkpoint includes a
cancelled occurrence and a moved one with its original key, plus invented
review/planning meetings. Mira's invented attendee address matches the seeded
person page's contact ref. Import it from a workspace-relative path with
`office.calendar-sync`'s `ics_file`; it never connects a real account. The demo
feed uses fixed October/November 2026 dates and sync's configured window.
