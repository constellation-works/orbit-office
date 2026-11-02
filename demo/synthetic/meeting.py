#!/usr/bin/env python3
"""Render the SIMULATED Lantern engineering-sync meeting on macOS.

The committed script (meeting/lantern-sync.txt) is the single source. Each
"Speaker: line" turn is spoken with the say voice the KT video already uses
for that speaker, and the turns are joined with a fixed gap. The answer key
(meeting/expected.json) is checked against the script: every quote must be
spoken verbatim in exactly one turn, and the rendered key gives each one its
#t= timestamp. Determinism is for the recorded toolchain and voice assets on
the same host (say voices can change between macOS releases).
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import wave

from common import HERE, child_env, run, write_json

SCRIPT = HERE / 'meeting' / 'lantern-sync.txt'
EXPECTED = HERE / 'meeting' / 'expected.json'
VOICES = {'Mira': 'Samantha', 'Jules': 'Daniel'}
RATE = 16000
GAP = int(0.6 * RATE)


def turns(text):
    parsed = []
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip() or line.startswith('#'):
            continue
        speaker, sep, speech = line.partition(': ')
        if not sep or speaker not in VOICES:
            raise ValueError(f'{SCRIPT.name}:{number}: expected "Mira: ..." or "Jules: ..."')
        parsed.append((speaker, speech.strip()))
    return parsed


def stamp(seconds, vtt=False):
    whole = int(seconds)
    clock = f'{whole // 3600:02d}:{whole % 3600 // 60:02d}:{whole % 60:02d}'
    return f'{clock}.{int(round((seconds - whole) * 1000)):03d}' if vtt else clock


def locate(spoken, expected):
    """Pin every answer-key quote to the one turn that speaks it."""
    keyed = {'simulated': True, 'meeting': expected['meeting'], 'decisions': [], 'todos': []}
    for group in ['decisions', 'todos']:
        for item in expected[group]:
            hits = [turn for turn in spoken if item['quote'] in turn['text']]
            if len(hits) != 1:
                raise ValueError(f'answer-key quote must be spoken in exactly one turn: {item["quote"]!r}')
            keyed[group].append({**item, 'speaker': hits[0]['speaker'], 'cite': '#t=' + stamp(hits[0]['start'])})
    return keyed


def generate(output):
    output = Path(output).resolve()
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    if platform.system() != 'Darwin':
        raise RuntimeError('meeting narration requires macOS say')
    env = child_env(output)
    script = turns(SCRIPT.read_text())
    expected = json.loads(EXPECTED.read_text())
    spoken = []
    offset = 0
    with wave.open(str(output / 'lantern-sync.wav'), 'wb') as joined:
        joined.setparams((1, 2, RATE, 0, 'NONE', 'not compressed'))
        for index, (speaker, speech) in enumerate(script):
            stem = output / 'turns' / f'turn-{index:02d}'
            stem.parent.mkdir(mode=0o700, exist_ok=True)
            stem.with_suffix('.txt').write_text(speech)
            run(['say', '-v', VOICES[speaker], '-r', '175', '-f', stem.with_suffix('.txt'),
                 '-o', stem.with_suffix('.aiff')], env=env)
            run(['ffmpeg', '-v', 'error', '-nostdin', '-i', stem.with_suffix('.aiff'), '-ar', str(RATE),
                 '-ac', '1', '-c:a', 'pcm_s16le', '-fflags', '+bitexact', '-flags:a', '+bitexact',
                 stem.with_suffix('.wav')], env=env)
            with wave.open(str(stem.with_suffix('.wav')), 'rb') as source:
                samples = source.readframes(source.getnframes())
            frames = len(samples) // 2
            spoken.append({'index': index, 'speaker': speaker, 'voice': VOICES[speaker], 'text': speech,
                           'start': offset / RATE, 'end': (offset + frames) / RATE})
            joined.writeframes(samples + bytes(GAP * 2))
            offset += frames + GAP
    run(['ffmpeg', '-v', 'error', '-nostdin', '-i', output / 'lantern-sync.wav', '-c:a', 'aac', '-b:a', '64k',
         '-fflags', '+bitexact', '-flags:a', '+bitexact', '-map_metadata', '-1', '-movflags', '+faststart',
         output / 'lantern-sync.m4a'], env=env, timeout=120)
    vtt = ['WEBVTT', '']
    for turn in spoken:
        vtt += [f'{stamp(turn["start"], True)} --> {stamp(turn["end"], True)}',
                f'<v {turn["speaker"]}>{turn["text"]}', '']
    (output / 'lantern-sync.vtt').write_text('\n'.join(vtt))
    write_json(output / 'script.json', spoken)
    write_json(output / 'answer-key.json', locate(spoken, expected))
    digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    manifest = {'simulated': True, 'duration_seconds': round(offset / RATE, 3), 'turns': len(spoken),
                'voices': VOICES, 'macos': platform.mac_ver()[0],
                'ffmpeg': run(['ffmpeg', '-version'], env=env).splitlines()[0],
                'wav_sha256': digest(output / 'lantern-sync.wav'),
                'm4a_sha256': digest(output / 'lantern-sync.m4a'),
                'script_sha256': digest(SCRIPT), 'expected_sha256': digest(EXPECTED),
                'generator_sha256': digest(Path(__file__))}
    write_json(output / 'manifest.json', manifest)
    return manifest


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='New directory; existing paths are refused')
    args = parser.parse_args()
    print(json.dumps(generate(args.output)))
