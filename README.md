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
  <a href="#models-at-runtime"><img src="https://img.shields.io/badge/models-configurable-informational" alt="Models: configurable" /></a>
  <a href="https://github.com/constellation-works/orbit"><img src="https://img.shields.io/badge/built%20on-Orbit-6E9FFF" alt="Built on Orbit" /></a>
</p>

<p align="center">
  <img src="docs/assets/office-demo.gif" alt="Animated tour of the real Orbit Office app on a synthetic workspace, captured November 2, 2026: a meeting goes in on the Source page and comes back as a timestamped transcript; Muse Spark pulls out four decisions, each linked to the second it was said, and splits the to-dos into yours and the agents'; a contradiction between rotating ingest keys monthly and the earlier weekly decision waits under Needs you with both sources cited; Muse Code crews pick up the agent to-dos as Orbit tasks and four pull requests merge on the lantern repository with tests green; the Today morning digest reads 4 decided, 4 shipped, 4 need you, every line cited; and Settings, Models shows the model behind every job and where it runs." width="880" />
</p>

Orbit Office turns your meetings into work that gets done. A local web app and an
[Orbit](https://github.com/constellation-works/orbit) plugin transcribe a recording,
pull out the decisions and to-dos, and hand the agents' share to coding crews that open
pull requests overnight. Every decision and to-do cites the moment of the meeting it came from.

- **Local-first.** Your Library is plain files and git in your own workspace; the app runs on `localhost`.
- **Configurable model routing.** Hosted calls go to the OpenAI-compatible endpoint and models you configure; the defaults are Meta's Model API and the Muse models ([table below](#models-at-runtime)).
- **Private stays private.** Material marked `private` goes only to the open-weights model on an endpoint you choose, never to the hosted endpoint.

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

**You need:** macOS or Linux (Windows through WSL2), Node 24+, `cargo` (until a release
ships prebuilt plugin bundles), an authenticated agent CLI (Claude Code, Codex or Muse
Code), and an API key for the hosted model endpoint (by default a pay-as-you-go Meta
Model API key).

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

## Models at runtime

Model routing is configurable in the Library settings (`.office/config.yaml`, or
`office.config-set`). `normal` material goes to one hosted, OpenAI-compatible endpoint,
`models.endpoint`; `private` material goes only to the private route, `open_weights`.
Each job's model is a setting too. Unset keys keep the defaults below: Meta's Model API
at `https://api.meta.ai/v1` and the Muse models. Every endpoint must use HTTPS (plain
HTTP only on loopback) and carry no credentials; the hosted key is the `meta_api_key`
plugin secret whatever the provider. Routing and the private route:
[models-and-privacy.md](docs/design/office/specs/models-and-privacy.md).

| Job | Setting (default) | Where it runs |
|---|---|---|
| Meeting and video transcription, in chunks of up to 10 minutes, with timestamped speaker turns | `models.transcribe` (`muse-voice-transcribe-1.0`) | The hosted endpoint: `POST /v1/asr/transcribe` on Meta's Model API, Chat Completions `input_audio` on any other |
| Meeting summaries, decisions and to-dos; the daily digest; document digests, KT chapters, keyframe and receipt reading | `models.standard` (`muse-spark-1.3`) | The hosted endpoint, `POST /chat/completions` |
| The `careful` tier: consolidating a Library bootstrap batch by subject | `models.careful` (`muse-wonder`) | The hosted endpoint, `POST /chat/completions` |
| Every use above whose input includes a `private` source: text, image and audio | `open_weights.model` (for example `meta-models/Muse-Spark-Open-816B-A42B`) | The private route: the one OpenAI-compatible endpoint you configure (your company's deployment, a hosted provider or your own hardware), never Meta's hosted API |
| The orchestrator session, and the agents Orbit dispatches to implement agent to-dos and review their PRs | Your agent CLI and Orbit crews | Your machine, started by Orbit, against whatever models that CLI is set up for |

"Private" means not sent to the hosted endpoint, only to the endpoint you chose; it does
not mean the material never leaves your machine.

## Provenance

- **Orbit is a dependency.** [Orbit](https://github.com/constellation-works/orbit)
  is an existing open-source agent runtime (MIT), installed at a tested release and never
  vendored, forked or added as a submodule. No Orbit code is in this repository.
- **The Rust workspace** started from the constellation
  [Rust CLI template](https://github.com/constellation-works/rust-cli-template).
- **Demo data is synthetic.** The Lantern meeting, the Lantern demo repo, chats, receipts
  and people in [demo/synthetic/](demo/synthetic/) are invented, and seeded history is
  labelled *simulated*.

The demo script: [delivery.md](docs/design/office/specs/delivery.md).

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
