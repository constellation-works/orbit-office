---
type: design
summary: "Spec: delivery — hackathon compliance, the locked 3-minute meeting-to-PRs demo script and the remaining build plan"
last_updated: 2026-11-02
last_validated: 2026-11-02
---

# Spec: Delivery

How the entry stays compliant, what the demo shows, and in what order it
is built.

Source: spec §14–§16.

## Compliance

- **Repository.** `constellation-works/orbit-office` is public, created after the Oct 26 12:00 ET kickoff, and its first commit is new work. Orbit is listed as a dependency with its tested commit. No Orbit code is in the repo.
- **The video** opens with: *"Orbit is my existing open-source agent runtime. This week I built the office plugin, the orchestrator skill, the Library and the app."*
- **Demo data is synthetic only:** the Lantern engineering-sync meeting, the Lantern demo repo, chats, receipts and people are all invented. Weeks of seeded history, including the September ledger decision, are labelled *simulated* on screen. No polaris, almanac, private repos or constellation state appears anywhere.
- **Meta models only.** Every model the project calls at runtime is a Meta model ([README](../../../../README.md#meta-models-at-runtime)).
- **Ownership.** No outside contributions to entry code during the event. Muse Code may be used.
- **Eligibility.** The entry is in the Indie Developer track (theme "Build for Your World"); confirm any open eligibility question in Discord `#questions`.
- The Rust workspace was started from the constellation Rust CLI template; that provenance is stated in the repo README.

## Demo script (3 minutes)

Locked 2026-11-02 ([ORB-21334]). It replaces the KT-onboarding script;
the KT video path, SAM 3.1 and the app's chain view are cut from the demo.

| Time | Beat |
|---|---|
| 0:00–0:15 | **Opening line** (compliance, above), then the problem: decisions made in meetings never turn into work. |
| 0:15–0:50 | **A meeting goes in.** The synthetic Lantern engineering sync ([demo/synthetic/meeting/](../../../../demo/synthetic/meeting/)) is uploaded. Muse Voice Transcribe transcribes it; Muse Spark extracts decisions and to-dos, split between you and agents, each cited to a `#t=` timestamp. |
| 0:50–1:10 | **Contradiction.** The meeting decides "rotate ingest keys monthly"; the seeded September ledger decision (labelled simulated) says weekly. It goes under Needs you, and agents leave rotation alone until it is resolved. |
| 1:10–2:10 | **Overnight time-lapse.** The agent to-dos become Orbit tasks in the synthetic Lantern demo repo; Muse Code crews (`muse-spark`, `muse-wonder`) implement them, Muse reviews, and the PRs merge. |
| 2:10–2:45 | **Morning digest:** Decided, Shipped (with PR links) and Needs you. Resolve the contradiction in one click. |
| 2:45–3:00 | **Close:** every model is Meta's, shown with the where-it-runs table from the [README](../../../../README.md#meta-models-at-runtime). |

The meeting's answer key ([expected.json](../../../../demo/synthetic/meeting/expected.json))
is what the extraction beat must show:

- **Decisions:** keep fifty orders per page on the dashboard; rotate ingest
  keys monthly, starting in December (the contradiction); replay always keeps
  the original order key; the Acme pilot stays read-only until the review.
- **Your to-dos (3):** tell the access desk about the new rotation schedule;
  book the Acme pilot review for next Thursday; ask Nori to review read
  access for the two new packing-station operators.
- **Agent to-dos (4), each sized for one overnight crew run:** add a retry
  with backoff to the spool uploader; fix the dashboard pagination
  off-by-one and test every page; add a `--dry-run` flag to `lantern
  replay`; make the intake desk reject an empty order key with a clear error.

Every model call in the demo is a Meta model; see
[models-and-privacy.md](./models-and-privacy.md).

## Build plan

Oct 27 – Nov 1 built the plugin, the Library, the app and the synthetic
pack; the [slice tasks](#slice-tasks) below record what landed. The rest
of the week, against the locked script (orchestration record [ORB-21334]):

| Day | Build | Demo |
|---|---|---|
| Mon 11-02 | Meeting extraction with `#t=` citations, the you/agent split and the ledger contradiction check ([ORB-21336]); the morning digest's Decided / Shipped / Needs you ([ORB-21337]); Meta-only model routing with Muse Voice Transcribe ([ORB-21338]) | Synthetic meeting, September ledger seed, this script and the README model table ([ORB-21339]); create the synthetic Lantern demo repo sized to the four agent to-dos |
| Tue 11-03 | Fixes from the first full run-through | Full run-through on synthetic data; file the agent to-dos as Orbit tasks in the Lantern repo and let the Muse Code crews run overnight, recorded for the time-lapse |
| Wed 11-04 | Freeze; README pass | Record and edit the video; submit before 21:00 ET; post on X |

Checks that gate the run-through: the meeting extraction cites a `#t=`
timestamp for every decision and to-do in the answer key; the monthly
rotation decision is reported against `dec-lantern-1`; and no runtime call
goes to a non-Meta model.

### Slice tasks

These are the tasks that built the plugin and app the demo runs on.
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

- `office.capture` and `office.source-show` — [ORB-18714] done
- Knowledge show and write — [ORB-18717] done
- Search and attach — [ORB-18719] done
- Agent to-do activity and orchestrator skills — [ORB-18720] done
- cloudcli — [ORB-18800] done

Pending or not verified as landed in this checkout:

- Delete modules still planned at submission — [ORB-18739]

## Task References

- [ORB-18708] — wired office-core ports through dispatch and composition.
- [ORB-18709] — built the Library store adapter.
- [ORB-18710] — built `office.init`, `office.reindex` and Library health.
- [ORB-18712] — built the Meta and Baseten model adapters with privacy routing.
- [ORB-18713] — built the Orbit callback adapter.
- [ORB-18714] — built `office.capture` and `office.source-show`.
- [ORB-18715] — built `office.transcribe` and `office.extract`.
- [ORB-18716] — built the KT routine and the kt skill.
- [ORB-18717] — built `office.knowledge-show` and `office.knowledge-write`.
- [ORB-18718] — built `office.artifact-show` and `office.artifact-promote`.
- [ORB-18719] — built `office.search` and `office.attach`.
- [ORB-18720] — built the agent to-do activity and the orchestrator skills.
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
- [ORB-18800] — built the read-only cloudcli plugin.
- [ORB-18809] — built `office.video-plan` and `office.frames`.
- [ORB-21334] — orchestrates the hackathon demo and locked this script.
- [ORB-21336] — meeting extraction for the demo.
- [ORB-21337] — built the morning digest for the demo.
- [ORB-21338] — Meta-only runtime models.
- [ORB-21339] — built the synthetic meeting, September ledger seed, this script and the README model table.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
