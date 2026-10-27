---
type: design
summary: "Spec: delivery — hackathon compliance, the 3-minute demo script and the day-by-day build plan"
last_validated: 2026-10-26
---

# Spec: Delivery

How the entry stays compliant, what the demo shows, and in what order it
is built.

Source: spec §14–§16.

## Compliance

- **Repository.** `constellation-works/orbit-office` is public, created after the Oct 26 12:00 ET kickoff, and its first commit is new work. Orbit is listed as a dependency with its tested commit. No Orbit code is in the repo.
- **The video** opens with: *"Orbit is my existing open-source agent runtime. This week I built the office plugin, the orchestrator skill, the Library and the app."*
- **Demo data is synthetic only:** a KT video about a made-up system, chats, receipts, people. Weeks of seeded history are labelled *simulated* on screen. No polaris, almanac, private repos or constellation state appears anywhere.
- **Ownership.** No outside contributions to entry code during the event. Muse Code may be used.
- **Eligibility.** Confirm the startup-track eligibility question in Discord `#questions`.
- The Rust workspace was started from the constellation Rust CLI template; that provenance is stated in the repo README.

## Demo script (3 minutes)

1. **0:00 Opening line** (compliance), then the problem in one sentence: cloud agents run your work where you can't see or keep it.
2. **0:15 Today screen:** seeded weeks of work. The automation ratio has climbed from 10% to 55% (labelled simulated).
3. **0:30 Drop a 50-minute KT video.** The cost preview appears; press Process. The result: a 6-minute chaptered index with tangents folded and one gotcha lifted out, a key-rotation runbook and three system pages proposed, and two questions for the presenter in your to-dos, every line citing `#t=…` or `#frame=…`. Ask "how do I rotate the ingest keys?" and the answer links to 00:31:12.
4. **1:10 Paste a Slack thread into Muse Code.** It is saved as a source, and the orchestrator links it to the client page and answers with citations. Then highlight a line in the KT transcript and say "make this a to-do for me". The task appears, citing that timestamp.
5. **1:35 An agent drafts the key-rotation change request** using the runbook the video just produced. Needs you: approve it (copied, not sent: the trust gate).
6. **1:55 A proposal appears.** "You've prepared a QBR deck by hand three times. Want a routine?" It shows the evidence, you accept, and it appears in Routines.
7. **2:20 Insights:** the conversion appears under "what got automated", with hours returned.
8. **2:35 Generate the quarterly review.** Every line is cited. Close on the Library and routines: you can see everything, and it is yours.

## Build plan (Oct 27 – Nov 4)

| Day | Plugin | App / orchestrator | Demo |
|---|---|---|---|
| Tue 10-27 | Repo, manifest, `init`, `capture`, Library layout, index, `reindex` | Muse Code wired to Orbit MCP with the office skill (day-1 checks: the skill is listed, and whether a prompt hook exists); app skeleton | Synthetic data plan |
| Wed 10-28 | `transcribe`, `video.plan`, `frames`, `extract` (KT profile), the KT routine | Today and To-dos screens; upload with cost preview | Generate the synthetic KT video |
| Thu 10-29 | `knowledge.*`, `attach`, the agent to-do activity, `artifact.promote` | Source view, Library screen | Seed 6 weeks of history |
| Fri 10-30 | `signals.scan`, proposals create and accept | Needs you, Routines screens | |
| Sat 10-31 | `metrics`, `review`, `selection.*` | Insights, Review screens; selection tray and hook | First full run-through |
| Sun 11-01 | Open-weight routing; receipts pack if on track | Polish; installable PWA; the paste capture moment | |
| Mon 11-02 | Hardening; [install script](./install.md) | Polish | Record the video |
| Tue 11-03 | Buffer | Buffer | Edit the video; README |
| Wed 11-04 | — | — | Submit before 21:00 ET; post on X for the viral bonus |

Day-1 checks that gate later work: the interactive Muse Code session
lists `office-orchestrator`; whether Muse Code has a prompt-submit hook;
the Muse Spark multimodal request shape (fallback: local OCR, then
Muse Spark on the text); and the KT checks in
[kt-videos.md](./kt-videos.md#day-1-checks).
