<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/office-lockup-on-dark.svg" />
    <img src="docs/assets/office-lockup-on-light.svg" alt="orbit office" width="400" />
  </picture>
</p>

<h3 align="center">Meetings in. Pull requests by morning.</h3>

<p align="center">
  <a href="https://github.com/constellation-works/orbit-office/actions/workflows/ci.yml"><img src="https://github.com/constellation-works/orbit-office/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="License: MIT" /></a>
  <a href="#meta-models-at-runtime"><img src="https://img.shields.io/badge/models-Meta%20only-informational" alt="Models: Meta only" /></a>
  <a href="https://github.com/constellation-works/orbit"><img src="https://img.shields.io/badge/built%20on-Orbit-6E9FFF" alt="Built on Orbit" /></a>
</p>

<!-- Demo GIF slot: uncomment once docs/assets/office-demo.gif exists, and describe what it shows in the alt text.
<p align="center">
  <img src="docs/assets/office-demo.gif" alt="" width="880" />
</p>
-->

Orbit Office turns your meetings into work that gets done. A local web app and an
[Orbit](https://github.com/constellation-works/orbit) plugin transcribe a recording,
pull out the decisions and to-dos, and hand the agents' share to coding crews that open
pull requests overnight. Every decision and to-do cites the moment of the meeting it came from.

- **Local-first.** Your Library is plain files and git in your own workspace; the app runs on `localhost`.
- **Meta models only.** Every model it calls at runtime is a Meta model ([table below](#meta-models-at-runtime)).
- **Private stays private.** Material marked `private` goes only to Muse Spark Open on an endpoint you choose, never to Meta's hosted API.

## How it works

```text
1. A meeting goes in      Upload a recording, a .vtt or pasted notes.
                          Muse Voice Transcribe returns timestamped speaker turns.
2. Decisions and to-dos   Muse Spark extracts a summary, decisions and to-dos, each
                          cited to a #t= turn, and splits them: yours, and agents'.
3. Needs you              Decisions go to a ledger. One that contradicts an earlier
                          decision becomes a question for you, and agent work on
                          that matter waits for your answer.
4. Overnight              Agent code to-dos become Orbit tasks in the repo you chose.
                          Muse Code crews implement and review them as pull requests.
5. Morning digest         Decided, Shipped (with PR links) and Needs you, since the meeting.
```

Agents never pick a side in a contradiction, and outward-facing drafts wait for your
approval. The [meetings skill](plugin/.orbit-plugin/skills/meetings/SKILL.md) is the
agent's side of this loop; the same app also keeps a cited Library, turns hour-long
knowledge-transfer videos into chapters and runs your routines.

## Quick start

**You need:** macOS or Linux (Windows through WSL2), Node 20+, `cargo` (until a release
ships prebuilt plugin bundles), an authenticated agent CLI (Claude Code, Codex or Muse
Code), and a pay-as-you-go Meta Model API key.

```bash
git clone https://github.com/constellation-works/orbit-office && cd orbit-office
./install.sh    # --agent muse to register Muse Code; --help for every option
```

The installer is idempotent: re-running it repairs a partial install. It installs
Orbit (tested `0.39.0`) when missing, a pinned `uv` and Python, `ffmpeg`, the `office`
and `gateway` plugins (asking once for their grants), and your API key, stored as an
Orbit plugin secret. Then it creates the `~/orbit` workspace and Library, registers your
agent CLI and opens the app at `http://localhost:4317`. Details:
[install.md](docs/design/office/specs/install.md).

Open your agent in `~/orbit` and talk to it: the `office-orchestrator` skill runs the
office. To send agent to-dos to a repository, set `meetings.agent_workspace` to that
Orbit workspace.

## Meta models at runtime

Every model the project calls at runtime is a Meta model; no other model provider is
called. Hosted calls go to the Meta Model API at `https://api.meta.ai/v1`. Routing and
the private route: [models-and-privacy.md](docs/design/office/specs/models-and-privacy.md).

| Model | Called for | Where it runs |
|---|---|---|
| Muse Voice Transcribe (`muse-voice-transcribe-1.0`) | Meeting and video transcription, in chunks of up to 10 minutes, with timestamped speaker turns | Meta Model API, `POST /v1/asr/transcribe` |
| Muse Spark (`muse-spark-1.3`) | Meeting summaries, decisions and to-dos; the daily digest; document digests, KT chapters, keyframe and receipt reading | Meta Model API, `POST /v1/chat/completions` |
| Muse Wonder (`muse-wonder`) | The `careful` tier: consolidating a Library bootstrap batch by subject | Meta Model API, `POST /v1/chat/completions` |
| Muse Spark Open (for example `meta-models/Muse-Spark-Open-816B-A42B`) | Every use above whose input includes a `private` source: text, image and audio | The private route: the one OpenAI-compatible endpoint you configure (your company's deployment, a hosted provider or your own hardware), never Meta's hosted API |
| Muse Code crews `muse-spark` and `muse-wonder` | The orchestrator session, and the agents Orbit dispatches to implement agent to-dos and review their PRs | Muse Code on your machine, started by Orbit, against Meta's hosted Muse Spark and Muse Wonder |

"Private" means not sent to Meta's hosted API, only to the endpoint you chose; it does
not mean the material never leaves your machine.

## Built for the Meta hackathon

- **Orbit is a pre-existing dependency.** [Orbit](https://github.com/constellation-works/orbit)
  is my existing open-source agent runtime (MIT), installed at a tested release and never
  vendored, forked or added as a submodule. No Orbit code is in this repository.
- **The rest was built this week.** The `office` plugin, the orchestrator skill, the
  Library and the web app are new work in this repository, which was created after the
  kickoff. The Rust workspace started from the constellation
  [Rust CLI template](https://github.com/constellation-works/rust-cli-template).
- **Demo data is synthetic.** The Lantern meeting, the Lantern demo repo, chats, receipts
  and people in [demo/synthetic/](demo/synthetic/) are invented, and seeded history is
  labelled *simulated*.

The demo script and the full compliance list: [delivery.md](docs/design/office/specs/delivery.md).

## Learn more

- **[Overview](docs/design/office/1_overview.md)**, **[design](docs/design/office/2_design.md)** and **[decisions](docs/design/office/4_decisions.md)**
- **[Specs](docs/design/office/specs/):** meetings and digest ([sessions](docs/design/office/specs/sessions.md)), [decision precedents](docs/design/office/specs/decision-precedents.md), [web app](docs/design/office/specs/web-app.md), [plugin surface](docs/design/office/specs/plugin-surface.md)
- **[ARCHITECTURE.md](ARCHITECTURE.md):** the Rust crates, the plugin tree and the app, and their boundaries
- **[Glossary](docs/design/office/references/glossary.md)** and the [constellation standards](docs/standards/) this repository adopts

## Development

```bash
make ci         # Rust: fmt, structure, standards, clippy, cargo-deny, tests, goldens
make baseline   # every stack's gate: ci, gateway, app typecheck and tests
```

`office orbit-tool` serves one Orbit exec call: a JSON envelope on stdin, one on stdout.

## License

[MIT](LICENSE)
