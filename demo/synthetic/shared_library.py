#!/usr/bin/env python3
"""Two synthetic workspaces share one bare repository through office.library-sync.

Alice's Library shares its Lantern KT pages and the KT transcript. A new
joiner's empty Library connects, syncs, and answers a question from Alice's
pages, with citations that open Alice's transcript at the second. Everything
is invented and stays in the new --root.
"""
import argparse
import json
from pathlib import Path

from common import REPO, child_env, run, write_json

TRANSCRIPT = """SIMULATED Lantern KT walkthrough. Invented system and people.
[00:00:05] Mira Vale: Lantern routes paper-kite orders through one durable spool.
[00:01:10] Mira Vale: When the spool stalls, pause intake before anything else.
[00:01:40] Mira Vale: Then replay the spool from the last committed offset.
[00:02:15] Nori Chen: Readers get Lantern-Readers, which is read-only.
"""

DISPATCH = """---
id: dom-lantern-dispatch
title: Lantern dispatch (SIMULATED)
summary: How the invented Lantern system routes paper-kite orders.
cites: ["{src}#t=00:00:05", "{src}#t=00:02:15"]
refs:
  - {{label: Owner, contact: mira.vale@acme.example}}
---
## Overview
Lantern routes every paper-kite order through one durable spool [{src}#t=00:00:05].
Read access is the Lantern-Readers group, which is read-only [{src}#t=00:02:15].
"""

RECOVERY = """---
id: rb-lantern-spool-recovery
title: Recover a stalled Lantern spool (SIMULATED)
summary: Pause intake, then replay from the last committed offset.
cites: ["{src}#t=00:01:10", "{src}#t=00:01:40"]
---
## Steps
1. Pause intake before anything else [{src}#t=00:01:10].
2. Replay the spool from the last committed offset [{src}#t=00:01:40].
"""

NOTES = """---
id: dom-alice-scratch
title: Alice's scratch notes (SIMULATED)
---
## Overview
Personal notes that are never marked shareable.
"""


class Workspace:
    def __init__(self, root, name, binary):
        self.root = root / name
        self.root.mkdir(mode=0o700)
        self.env = child_env(self.root)
        # Office's git children read only HOME's configuration.
        (self.root / 'home/.gitconfig').write_text(
            f'[user]\n\tname = {name.title()} (SIMULATED)\n\temail = {name}@acme.example\n')
        self.binary = binary
        self.context = {'workspace_root': str(self.root), 'plugin_state': str(self.root / 'office-state'),
                        'agent': 'synthetic-demo', 'model': 'simulated', 'secrets': {}}
        self.calls = []

    def call(self, tool, payload=None):
        response = json.loads(run([self.binary, 'orbit-tool'], env=self.env, cwd=self.root, timeout=180,
                                  input=json.dumps({'schema_version': 1, 'tool': tool,
                                                    'input': payload or {}, 'context': self.context})))
        if not response.get('ok'):
            error = response.get('error', {})
            raise RuntimeError(f"{tool}: {error.get('code')}: {error.get('message')}")
        self.calls.append({'tool': tool, 'input': payload or {}, 'output': response['output']})
        write_json(self.root / 'calls.json', self.calls)
        return response['output']

    def connect(self, remote, member):
        self.call('office.init')
        self.call('office.config-set', {'key': 'shared', 'value': {
            'remote': str(remote), 'branch': 'main', 'member': member}})


def check(condition, message):
    if not condition:
        raise SystemExit(f'shared library demo check failed: {message}')


def demo(root, binary):
    root = Path(root).resolve()
    # Refuse even an empty existing directory: no merge into a real Library.
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    (root / '.synthetic-demo').write_text('SIMULATED only\n')
    binary = Path(binary).resolve(strict=True)
    run(['git', 'init', '--quiet', '--bare', root / 'team.git'], env=child_env(root), cwd=root)

    alice = Workspace(root, 'alice', binary)
    alice.connect(root / 'team.git', 'alice')
    source = alice.call('office.capture', {
        'kind': 'meeting', 'title': 'SIMULATED Lantern KT walkthrough', 'text': TRANSCRIPT,
        'sensitivity': 'normal', 'origin': 'SIMULATED Acme Paper Kites demo', 'tags': ['simulated']})['id']
    for page in [DISPATCH, RECOVERY]:
        alice.call('office.knowledge-write', {'markdown': page.format(src=source)})
    alice.call('office.knowledge-write', {'markdown': NOTES})
    # Marking is the approval: the evidence first, then the pages citing it.
    for document in [source, 'dom-lantern-dispatch', 'rb-lantern-spool-recovery']:
        alice.call('office.share-set', {'id': document, 'shareable': True})
    preview = alice.call('office.library-sync', {'dry_run': True})
    first = alice.call('office.library-sync')
    check(first['pushed'] == preview['pushed'] and len(first['pushed']) == 3, f"alice pushed {first['pushed']}")
    check(not any('dom-alice-scratch' in path for path in first['pushed']), 'unshared page left the machine')
    again = alice.call('office.library-sync')
    check(again['pushed'] == [] and again['commit'] == first['commit'], 'a second sync pushed again')
    shared = run(['git', '--git-dir', root / 'team.git', 'show', 'main:knowledge/domain/dom-lantern-dispatch.md'],
                 env=child_env(root), cwd=root)
    check('mira.vale@acme.example' not in shared, 'a contact ref left the machine')

    joiner = Workspace(root, 'joiner', binary)
    joiner.connect(root / 'team.git', 'joiner')
    pulled = joiner.call('office.library-sync')
    check(pulled['skipped'] == [] and pulled['pushed'] == [], f'joiner sync {pulled}')
    question = 'What do I do when the Lantern spool stalls?'
    hits = joiner.call('office.search', {'query': 'spool stalls', 'layer': 'shared'})['hits']
    check(hits and hits[0]['shared_by'] == 'alice', f'no shared answer: {hits}')
    page = joiner.call('office.knowledge-show', {'id': 'rb-lantern-spool-recovery'})['page']
    cite = f'alice/{source}'
    check(page['layer'] == 'shared' and any(c.startswith(cite) for c in page['cites']), f'page {page}')
    transcript = joiner.call('office.source-show', {'id': cite})
    check(transcript['shared_by'] == 'alice' and '[00:01:10]' in transcript['body'], 'transcript not shared')
    manifest = {'simulated': True, 'question': question, 'answer_page': page['id'],
                'citations': page['cites'], 'commit': first['commit'],
                'alice_pushed': first['pushed'], 'joiner_pulled': pulled['pulled']}
    write_json(root / 'shared-demo.json', manifest)
    return manifest


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True, help='New directory; existing paths are refused')
    parser.add_argument('--office', type=Path, default=REPO / 'target/debug/office')
    args = parser.parse_args()
    result = demo(args.root, args.office)
    print(json.dumps({'simulated': True, 'root': str(args.root), 'answer_page': result['answer_page'],
                      'citations': result['citations']}))
