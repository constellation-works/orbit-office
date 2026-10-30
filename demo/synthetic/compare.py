#!/usr/bin/env python3
"""Compare complete seeded Library content, including originals and revisions.

Normalize source identities, dates and Git commit identities. Approval/plan
fingerprints derived from those normalized inputs are represented by their
field names; their full underlying records remain in the comparison. Binary
originals and frames are compared by their unmodified SHA-256. No page body,
source text, citation, count, path or link is dropped.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

from common import child_env, run

FINGERPRINTS = {'digest', 'inputs_digest', 'evidence_digest', 'settings_digest',
                'plan_digest', 'payload_sha256', 'base_sha256', 'transcription_sha256'}


def snapshot(root):
    root = Path(root).resolve()
    library = root / 'library'
    manifest = json.loads((root / 'seed-manifest.json').read_text())
    calls = json.loads((root / 'calls.json').read_text())
    replacements = {source: f'src-synthetic-{i:04d}' for i, (key, source) in
                    enumerate(sorted(manifest['sources'].items()))}
    env = child_env(root)
    commits = run(['git', '-C', library, 'rev-list', '--reverse', 'HEAD'], env=env).splitlines()
    replacements.update({commit: f'commit-{i:04d}' for i, commit in enumerate(commits)})

    def fingerprints(value):
        if isinstance(value, dict):
            for key, item in value.items():
                if key in FINGERPRINTS and isinstance(item, str) and re.fullmatch('[a-f0-9]{64}', item):
                    replacements.setdefault(item, 'normalized-fingerprint-' + key)
                fingerprints(item)
        elif isinstance(value, list):
            for item in value:
                fingerprints(item)
    fingerprints(calls)

    def text(value):
        for original, replacement in sorted(replacements.items(), key=lambda item: -len(item[0])):
            value = value.replace(original, replacement)
        value = value.replace(str(root), '<seed-root>')
        value = re.sub(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})', '<timestamp>', value)
        value = re.sub(r'\b\d{4}-\d{2}-\d{2}\b', '<date>', value)
        value = re.sub(r'sources/([a-z]+)/\d{4}/\d{2}/', r'sources/\1/', value)
        return re.sub(r'sources/\d{4}/\d{2}/', 'sources/', value)

    files = {}
    for file in sorted(library.rglob('*')):
        relative = file.relative_to(library)
        if '.git' in relative.parts or not file.is_file():
            continue
        normalized_path = text(relative.as_posix())
        if normalized_path in files:
            raise AssertionError('normalization collapsed distinct paths: ' + relative.as_posix())
        data = file.read_bytes()
        if file.suffix in {'.md', '.json', '.yaml'} or file.name == '.gitignore':
            files[normalized_path] = {'text': text(data.decode('utf-8'))}
        else:
            files[normalized_path] = {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}
    # Preserve commit ordering and messages as evidence of identical tool writes.
    history = run(['git', '-C', library, 'log', '--reverse', '--format=%s'], env=env).splitlines()
    return {'files': files, 'history': [text(line) for line in history]}


def compare(left, right):
    a, b = snapshot(left), snapshot(right)
    if a != b:
        changed = sorted(key for key in set(a['files']) | set(b['files']) if a['files'].get(key) != b['files'].get(key))
        raise AssertionError('Libraries differ: ' + ', '.join(changed[:12]) +
                             ('; commit sequence differs' if a['history'] != b['history'] else ''))
    return {'equal': True, 'files': len(a['files']), 'commits': len(a['history'])}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('left', type=Path)
    parser.add_argument('right', type=Path)
    args = parser.parse_args()
    print(json.dumps(compare(args.left, args.right)))
