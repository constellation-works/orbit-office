#!/usr/bin/env python3
"""Render the 15-minute, two-narrator Lantern KT recording on macOS.

No recording or real terminal is captured. Every slide, terminal command and
voice line is generated. Determinism is for the recorded toolchain and voice
assets on the same host (say voices can change between macOS releases).
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import textwrap
import wave

from common import HERE, child_env, run, write_json
from native import prepare

# Two 30-second turns per scene. The gotcha is intentionally inside a tangent.
SCENES = [
    ("Welcome to Lantern", "slide", "core",
     "Welcome. I am Mira Vale, the fictional owner of Lantern. Jules and I are explaining an invented order dispatch system. Everything you see and hear is simulated. Our customer makes paper kites. We will follow an order from the intake desk to the packing station, then recover a stuck order safely.",
     "I am Jules Reed. I run the fictional Acme Paper Kites pilot. I will ask the questions a new teammate might ask. Today we will pause before replaying anything, preserve the checkpoint, and keep read access separate from replay permission. The screen commands are rendered text, never executed against a service."),
    ("Three moving parts", "slide", "core",
     "Lantern has an intake desk, a durable spool, and a dispatcher. The desk gives each order an original order key. The spool keeps that key and the order body. The dispatcher asks a packing station to prepare a kite. The dashboard is only a view, so rebuilding it cannot recover missing spool records.",
     "So if the dashboard shows nothing, we should check the spool before announcing data loss. Correct. We also compare the checkpoint to the last acknowledged order. A stale view can look alarming even while the durable record is safe. Write down which view you inspected and its refresh time before asking for help."),
    ("First read-only inspection", "terminal", "core",
     "This scene illustrates lantern status followed by lantern spool inspect. Neither command changes an order. The result shows checkpoint forty two and one unacknowledged order. A new reader can inspect that checkpoint, but cannot use the replay command. The distinction matters because replay has effects at the packing station even when the dashboard is quiet.",
     "I would put the order key and checkpoint into my investigation notes. I would also record that the dispatcher is still running. We should not race it by starting a manual recovery. Ask the owner for a recovery window first. Read access is useful for diagnosis, but it is never permission to send another order."),
    ("Tangent: the kite festival", "slide", "tangent",
     "A short tangent. The imaginary company once used green kites for its demo festival. The packing team argued about ribbon colors for an entire afternoon. This story explains our silly names, but it is not operating guidance. Please do not turn the ribbon debate into a runbook requirement or a customer commitment.",
     "My favorite fictional kite had a tiny lantern painted on it. We named the staging stations after winds, which made the first training session confusing. Those names are not service names. When writing permanent notes, keep the festival story in the source recording and keep the operational pages about spool safety and access."),
    ("Pause before replay", "terminal", "core",
     "Return to the recovery procedure. Request the recovery window, then pause the dispatcher. Inspect the status again and wait for the paused state before continuing. Save the spool checkpoint and the original order key. A pause request that has not been acknowledged is not a pause, so do not proceed from a pending result.",
     "The rendered terminal shows lantern dispatcher pause and then lantern status. We are looking for paused equals true. I would attach that observation to the draft recovery plan. If a teammate cannot confirm it, we stop and ask Mira. This is a repeatable checkpoint rather than a reason to guess that the dispatcher is probably idle."),
    ("Deduplicate by original key", "terminal", "core",
     "Replay always carries the original order key. The station deduplicates on that key. Generating a fresh key makes the same order look new and can pack a second kite. The replay command in this slide names order demo forty two. It does not mint another identity and it does not edit the original body.",
     "This is the heart of the procedure. If I cannot find the original key, I do not invent one to get past validation. I ask the owner to trace intake. Rebuilding a dashboard identifier is different from recovering an order identity. Our draft must explain what we found and which key the station will receive."),
    ("Checkpoint and view", "slide", "core",
     "The checkpoint is a durable recovery marker. Save it before rebuilding the dashboard view. The dashboard may be regenerated from the spool after the checkpoint is safe. A rebuild can change what the reader sees, but it cannot replace a missing acknowledgement. Keep these separate in your investigation so the next person can reproduce your conclusion.",
     "I will ask whether the view was rebuilt before the checkpoint was saved. That order belongs in the runbook. If it was reversed, the source notes should record the uncertainty. A tidy dashboard is not evidence of successful delivery. We need the station acknowledgement for the original order key before calling the recovery complete."),
    ("Read access request", "slide", "core",
     "The onboarding process requests the Lantern Readers AD group. Supply your synthetic staff handle, the project name, a read-only justification and approver Nori Chen. The access desk form is a reserved example address. In the demo an agent prepares a draft with those fields. The user reviews it and submits it themselves.",
     "The request must say why read-only access is enough. It does not ask for replay permission by implication. The draft should cite the access process page, name the requested group and link the form. An access request awaiting review is not approved access. We keep that distinction visible in the demo's Needs you queue."),
    ("Tangent: rehearsal and a gotcha", "terminal", "tangent",
     "Another tangent about our imaginary rehearsal. Jules brought a cardboard packing station to the training room. It was supposed to make the demo funny, but its status lamp was wired backward. Nobody should learn hardware repair from this anecdote. It stays in the recording as background while the useful procedure stays short.",
     "One actual gotcha came up during that rehearsal. Do not rotate the replay key after a timeout. The first attempt may already have reached the station. Reuse the original key and inspect the acknowledgement before another attempt. Although I mentioned this inside a tangent, preserve that warning as a cited gotcha. The cardboard lamp story can be omitted."),
    ("Timeout is uncertainty", "terminal", "core",
     "A timeout says we did not receive a reply in time. It does not prove the station rejected the order. Inspect the station acknowledgement and the durable spool before another attempt. Do not automatically retry with a new key. The original key is the link that lets the station recognize a duplicate safely.",
     "In my handoff I would write outcome unknown until the acknowledgement is found. I would include the original key, saved checkpoint and paused status. That makes the next person's investigation concrete. A missing response cannot be converted into success by changing the dashboard. We either find the acknowledgement or ask the owner to resolve it."),
    ("Resume with evidence", "terminal", "core",
     "After the original order is acknowledged, verify the saved checkpoint and the queue depth. Then ask for permission to resume the dispatcher within the agreed recovery window. Observe the running state and one fresh acknowledgement. Record the evidence in the recovery notes. Resume is a separate step, not a side effect of rebuilding the view.",
     "The terminal scene illustrates the final status output, but does not send a real command. We should see the recovered key acknowledged and the queue moving. If either observation is missing, the draft report lists it as unresolved. A readable receipt should tell a new teammate exactly what was observed and when."),
    ("Who owns what", "slide", "core",
     "Mira owns spool recovery, Jules owns the pilot relationship, and Nori reviews access requests. Those are invented work roles. The onboarding interview records them as facts, then proposes About pages using the bundled person, client and project templates. The user accepts each page with citations instead of trusting an uncited generated biography.",
     "Our durable pages should describe the system and responsibilities. This week's queue size belongs in a dated source, since it will change. The distinction keeps a six-week demo believable without teaching the agent to treat an old status as current. When in doubt, link the source and keep the About page concise."),
    ("What belongs in the Library", "slide", "core",
     "Keep sources complete and pages selective. The orientation deck, process page and policy remain searchable. Consolidation groups evidence by subject rather than copying one page per file. The access runbook has inputs, cited steps and a clear done-when statement. It is performable by an agent only up to a reviewable draft.",
     "The fictional PTO policy is a summary with a date and a pointer to its authoritative synthetic source. It must not become a claim about a real employer. All weeks in this pack are labelled simulated. The cutoff in Library settings tells Insights exactly which history was seeded, so later real activity should not inherit the demo label."),
    ("Review exercise", "terminal", "core",
     "Imagine the station timed out after accepting order demo forty two. What should you do next? Pause within the recovery window, save the checkpoint, inspect the acknowledgement and preserve the original key. Do not create a fresh order just to make the status screen turn green. Draft the next action and ask for review where the process requires it.",
     "Now imagine you only have Lantern Readers access. You can collect the status and explain the problem. You cannot replay the order yourself. Ask Mira for the recovery procedure and authority. A useful agent hands over a complete draft with evidence and honest limits. That is more useful than a confident claim without a receipt."),
    ("Wrap up and questions", "slide", "core",
     "We have followed the invented system from intake to acknowledgement. The durable spool is the source of truth. Replay keeps the original key. Save the checkpoint before rebuilding the view. Ask for the recovery window, pause before replay, and verify before resuming. The gotcha inside the rehearsal tangent is worth keeping with its timestamp.",
     "Thank you for joining the simulated Lantern KT. Ask Nori about read-only access and ask Mira about unresolved recovery questions. The demo leaves outward actions as drafts. Its private route should display the endpoint destination and third-party label, with Built with Muse Spark Open visible. A loopback rehearsal is labelled simulated and is never claimed as a live provider run."),
]


def generate(output, voices=("Samantha", "Daniel")):
    output = Path(output).resolve()
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    env = child_env(output)
    tools = prepare(output, env)
    font = Path('/System/Library/Fonts/Menlo.ttc')
    if platform.system() != 'Darwin' or not font.is_file():
        raise RuntimeError('video narration requires macOS say and Menlo')
    turns = []
    with wave.open(str(output / 'narration.wav'), 'wb') as joined:
        joined.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
        for number, (title, kind, label, first, second) in enumerate(SCENES):
            for speaker, speech in enumerate([first, second]):
                index = number * 2 + speaker
                stem = output / f'turn-{index:02d}'
                stem.with_suffix('.txt').write_text(speech)
                run(['say', '-v', voices[speaker], '-r', '165', '-f', stem.with_suffix('.txt'), '-o', stem.with_suffix('.aiff')], env=env)
                run(['ffmpeg', '-v', 'error', '-nostdin', '-i', stem.with_suffix('.aiff'), '-ar', '16000', '-ac', '1', '-c:a', 'pcm_s16le', stem.with_suffix('.wav')], env=env)
                with wave.open(str(stem.with_suffix('.wav')), 'rb') as source:
                    samples = source.readframes(source.getnframes())
                limit = 30 * 16000 * 2
                if len(samples) > limit:
                    raise RuntimeError(f'narration turn {index} exceeds 30 seconds; select a faster voice')
                joined.writeframes(samples + bytes(limit - len(samples)))
                turns.append({'start': index * 30, 'end': (index + 1) * 30,
                              'speaker': ['Mira', 'Jules'][speaker], 'text': speech, 'label': label})
            terminal = (f'$ lantern status\ncheckpoint: 42  paused: true\n$ lantern spool inspect demo-42\noriginal_key: demo-42\nacknowledged: true' if kind == 'terminal' else
                        '\n'.join(textwrap.wrap(first, 65)[:6]))
            text = f'SIMULATED / LANTERN KT / {label.upper()}\n\n{title}\n\n{terminal}\n\nMira Vale + Jules Reed / invented system'
            frame_text = output / f'frame-{number:02d}.txt'
            frame_text.write_text(text)
            run([tools / 'synthetic-native', 'frame', frame_text, output / f'frame-{number:02d}.png',
                 'light' if number % 2 else 'dark'], env=env)
    concat = ''.join(f"file 'frame-{i:02d}.png'\nduration 60\n" for i in range(len(SCENES))) + "file 'frame-14.png'\n"
    (output / 'frames.txt').write_text(concat)
    run(['ffmpeg', '-v', 'error', '-nostdin', '-f', 'concat', '-safe', '0', '-i', output / 'frames.txt',
         '-i', output / 'narration.wav', '-vf', 'fps=1', '-t', '900', '-c:v', 'libx264', '-preset', 'veryfast',
         '-threads', '1', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-b:a', '64k', '-fflags', '+bitexact',
         '-flags:v', '+bitexact', '-flags:a', '+bitexact', '-map_metadata', '-1', '-movflags', '+faststart',
         output / 'encoded.mp4'], env=env, timeout=300)
    # FFmpeg's concurrent input scheduling can change MP4 chunk interleaving
    # despite identical audio/video packets. Remux the completed streams with
    # unlimited interleave waiting for a byte-deterministic container.
    run(['ffmpeg', '-v', 'error', '-nostdin', '-i', output / 'encoded.mp4', '-c', 'copy',
         '-map_metadata', '-1', '-fflags', '+bitexact', '-max_interleave_delta', '0',
         '-movflags', '+faststart', output / 'lantern-kt.mp4'], env=env, timeout=120)
    sha = hashlib.sha256((output / 'lantern-kt.mp4').read_bytes()).hexdigest()
    write_json(output / 'script.json', turns)
    manifest = {'simulated': True, 'duration_seconds': 900, 'sha256': sha,
                'voices': voices, 'macos': platform.mac_ver()[0],
                'ffmpeg': run(['ffmpeg', '-version'], env=env).splitlines()[0],
                'font_sha256': hashlib.sha256(font.read_bytes()).hexdigest(),
                'native_helper_sha256': hashlib.sha256((HERE / 'native.swift').read_bytes()).hexdigest(),
                'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    write_json(output / 'manifest.json', manifest)
    return manifest


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(generate(args.output)))
