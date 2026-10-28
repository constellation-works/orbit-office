---
type: design
summary: "Spec: delivery — hackathon compliance, the 3-minute demo script and the day-by-day build plan"
last_updated: 2026-10-27
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
3. **0:30 Day one in a new role:** drop the onboarding folder and a 50-minute KT video. Library setup shows the batch preview; press Process. Pages arrive grouped by subject, one per topic rather than one per document. **The KT video** (marked private at the preview, so every call goes to Muse Spark Open on the private route, labelled with where it runs): The cost preview appears; press Process. The result: a 6-minute chaptered index with tangents folded and one gotcha lifted out, a key-rotation runbook and three system pages proposed, and two questions for the presenter in your to-dos, every line citing `#t=…` or `#frame=…`. Ask "how do I rotate the ingest keys?" and the answer links to 00:31:12.
4. **1:10 Paste a Slack thread into Muse Code.** It is saved as a source, and the orchestrator links it to the client page and answers with citations. Then highlight a line in the KT transcript and say "make this a to-do for me". The task appears, citing that timestamp.
5. **1:35 "Request access to the ingest AD group for me."** The agent follows the `performable` runbook the onboarding folder produced and drafts the request. Needs you: approve it (copied, not sent: the trust gate).
6. **1:55 A proposal appears.** "You've prepared a QBR deck by hand three times. Want a routine?" It shows the evidence, you accept, and it appears in Routines.
7. **2:20 Insights:** the conversion appears under "what got automated", with hours returned.
8. **2:35 Generate the quarterly review.** Every line is cited. Close on the Library and routines: you can see everything, and it is yours.

## Build plan (Oct 27 – Nov 4)

| Day | Plugin | App / orchestrator | Demo |
|---|---|---|---|
| Tue 10-27 | Repo, manifest, `init`, `capture`, Library layout, index, `reindex` | Muse Code wired to Orbit MCP with the office skill (day-1 checks: the skill is listed, and whether a prompt hook exists); app skeleton | Synthetic data plan |
| Wed 10-28 | `transcribe`, `video-plan`, `frames`, `extract` (KT profile), the KT routine | Today and To-dos screens; upload with cost preview | Generate the synthetic KT video |
| Thu 10-29 | `knowledge.*`, `attach`, the agent to-do activity, `artifact-promote`; document capture, `extract` digest profile, `batch-plan`, `consolidate`, `bootstrap-status` | Source view, Library screen with Library setup | Synthetic onboarding folder; seed 6 weeks of history |
| Fri 10-30 | `signals-scan`, proposals create and accept | Needs you, Routines screens | |
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

### Slice tasks

The calendar above is the order. These are the tasks that implement it.
"Done" means the task is done and its merged commit is in this checkout.

Landed:

- App server, Orbit client, static serving, dispatch pass — [ORB-18725] done
- Web UI kit — [ORB-18726] done
- Today screen — [ORB-18727] done
- To-dos screen — [ORB-18728] done
- PWA shell and the settings feature — [ORB-18735] done
- Install script — [ORB-18736] done
- App CI job — [ORB-18738] done
- Port wiring through dispatch — [ORB-18708] done
- Library store — [ORB-18709] done
- `office.init`, `office.reindex`, Library health — [ORB-18710] done
- About templates — [ORB-18760] done; `office.init` installs them — [ORB-18762] done
- Model adapters and privacy routing — [ORB-18712] done
- Orbit callback adapter — [ORB-18713] done
- Artifact show and promote — [ORB-18718] done
- Synthetic demo data — [ORB-18737] done
- Gateway install and agent CLI deny rules — [ORB-18799] done
- `office.signals-scan` — [ORB-18721] done
- Pinned uv and Python for agents — [ORB-18779] done
- Gateway GET, connections, redaction, audit — [ORB-18796] done
- Gateway drafted writes — [ORB-18797] done

- `office.transcribe` and `office.extract` — [ORB-18715] done
- `office.video-plan` and `office.frames` — [ORB-18809] done
- KT routine and the kt skill — [ORB-18716] done
- Proposal create and accept — [ORB-18722] done
- Source view, upload and paste capture — [ORB-18729] done
- Library screen — [ORB-18730] done
- Routines screen — [ORB-18731] done
- Approvals in the app — [ORB-18733] done
- Selection tools — [ORB-18723] done; selection tray — [ORB-18734] done
- Metrics and review — [ORB-18724] done; Insights screen — [ORB-18732] done
- Session brief — [ORB-18746] done
- Daily digest — [ORB-18747] done
- Script runner — [ORB-18781] done

Pending or not verified as landed in this checkout:

- `office.capture` and `office.source-show` — [ORB-18714]
- Knowledge show and write — [ORB-18717]
- Search and attach — [ORB-18719]
- Agent to-do activity and orchestrator skills — [ORB-18720]
- cloudcli — [ORB-18800]
- Delete modules still planned at submission — [ORB-18739]

## Task References

- [ORB-18708] — wired office-core ports through dispatch and composition.
- [ORB-18709] — built the Library store adapter.
- [ORB-18710] — built `office.init`, `office.reindex` and Library health.
- [ORB-18712] — built the Meta and Baseten model adapters with privacy routing.
- [ORB-18713] — built the Orbit callback adapter.
- [ORB-18714] — builds `office.capture` and `office.source-show`.
- [ORB-18715] — built `office.transcribe` and `office.extract`.
- [ORB-18716] — built the KT routine and the kt skill.
- [ORB-18717] — builds `office.knowledge-show` and `office.knowledge-write`.
- [ORB-18718] — built `office.artifact-show` and `office.artifact-promote`.
- [ORB-18719] — builds `office.search` and `office.attach`.
- [ORB-18720] — builds the agent to-do activity and the orchestrator skills.
- [ORB-18721] — built `office.signals-scan`.
- [ORB-18722] — built proposal create and accept, with the signals routine.
- [ORB-18723] — built `office.selection-get` and `office.selection-put`.
- [ORB-18724] — built metrics, review inputs and the review.
- [ORB-18725] — built the app server, the Orbit client, static serving and the dispatch pass.
- [ORB-18726] — built the web UI kit, API client and base styles.
- [ORB-18727] — built the Today screen.
- [ORB-18728] — built the To-dos screen.
- [ORB-18729] — built the source view, upload and paste capture.
- [ORB-18730] — built the Library screen.
- [ORB-18731] — built the Routines screen.
- [ORB-18732] — built the Insights screen.
- [ORB-18733] — built in-app approval of to-dos and proposals.
- [ORB-18734] — built the selection tray, selection feature and prompt hook.
- [ORB-18735] — built the PWA shell and the settings feature.
- [ORB-18736] — built the install script.
- [ORB-18737] — built synthetic demo data seeded through the tools.
- [ORB-18738] — added the app CI job.
- [ORB-18739] — deletes modules still planned at submission.
- [ORB-18746] — built `office.session-brief`.
- [ORB-18747] — built `office.digest` and the daily digest routine.
- [ORB-18760] — built the knowledge template model and the bundled About templates.
- [ORB-18762] — made `office.init` install the bundled templates.
- [ORB-18779] — pinned uv and Python 3.12 for agents in the install script.
- [ORB-18781] — built `office.script-run`.
- [ORB-18796] — built the gateway plugin's connections, GET, redaction and audit.
- [ORB-18797] — built drafted API writes, approved and sent once.
- [ORB-18799] — installed the gateway plugins and the agent CLI deny rules.
- [ORB-18800] — builds the read-only cloudcli plugin.
- [ORB-18809] — built `office.video-plan` and `office.frames`.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
