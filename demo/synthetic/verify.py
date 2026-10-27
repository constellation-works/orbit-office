#!/usr/bin/env python3
"""Behavior checks through the seed, real exec backends and comparison entry point."""
import argparse
import hashlib
import json
from pathlib import Path

from common import REPO, write_json
from compare import compare
from seed import seed


def examine(root, manifest, video):
    library = root / 'library'
    assert len(manifest['sources']) == (97 if video else 96), 'six weeks plus onboarding and draft sources'
    calls = json.loads((root / 'calls.json').read_text())
    db = json.loads((root / 'demo-orbit.json').read_text())
    accepted = [c for c in calls if c['tool'] == 'office.proposal-accept']
    for identity in manifest['about_pages']:
        receipts = [c for c in accepted if c['output']['change'].get('page') == identity]
        assert len(receipts) == 1, identity + ' must be accepted once'
        payload = json.loads(db['tasks'][receipts[0]['input']['id']]['artifacts']['proposal.json'])
        assert payload['kind'] == 'knowledge_add'
        assert 'template:' in payload['markdown']
        assert manifest['sources']['interview'] in payload['markdown']
    assert manifest['crm_patch_requests'] == 0
    assert manifest['crm_draft']['state'] == 'pending'
    assert manifest['crm_draft']['request']['method'] == 'PATCH'
    assert db['tasks'][manifest['crm_draft']['task_id']]['status'] == 'proposed'
    assert '[secret]' in json.dumps(manifest['crm_get'])
    runbook = (library / 'knowledge/runbooks' / (manifest['access_runbook'] + '.md')).read_text()
    assert 'performable: agent' in runbook and 'via: draft' in runbook
    assert len(manifest['bootstrap']['docs']['proposed']) == 1
    assert len(manifest['bootstrap']['process']['proposed']) == 1
    assert manifest['metrics']['simulated_until'] == '2026-10-25'
    weeks = manifest['metrics']['weeks']
    assert len([w for w in weeks if w['since'] <= '2026-10-25']) == 6
    assert any(w['since'] > '2026-10-25' for w in weeks), 'an unseeded week must remain unlabelled'
    if video:
        result = manifest['video']
        assert result['derived_by']['route'] == 'private:provider'
        assert len(result['result']['chapters']) == 15
        assert result['result']['gotchas'][0]['cites'] == ['#t=00:08:30']
        assert result['dropped']['dangling'] == 0
        frame_calls = next(c['output']['frames_read'] for c in calls if c['tool'] == 'office.frames')
        assert frame_calls > 0, 'video must exercise real scene/frame processing'
    return {'criteria_1_3_4_6_8': 'passed', 'private_fixture_route': 'passed' if video else 'not_run',
            'hosted_provider': 'not_run', 'accepted_proposals': len(accepted)}


def verify(output, binary, video=None):
    output.mkdir(parents=True, exist_ok=False, mode=0o700)
    left, right = output / 'left', output / 'right'
    seed(left, binary, video)
    seed(right, binary, video)
    return verify_existing(output, binary, video)


def verify_existing(output, binary, video=None):
    left, right = output / 'left', output / 'right'
    first = json.loads((left / 'seed-manifest.json').read_text())
    second = json.loads((right / 'seed-manifest.json').read_text())
    first_checks = examine(left, first, video)
    examine(right, second, video)
    comparison = compare(left, right)
    assert comparison['files'] >= len(first['sources']), 'normalization must preserve every source'
    # A comparator that ignores source text or links would hide demo drift.
    page = right / 'library/knowledge/domain/dom-user.md'
    original = page.read_bytes()
    for mutation in [b'\nSIMULATED changed responsibility.\n', b'\n[[dom-project-different]]\n']:
        page.write_bytes(original + mutation)
        try:
            try:
                compare(left, right)
            except AssertionError:
                pass
            else:
                raise AssertionError('comparison missed a semantic mutation')
        finally:
            page.write_bytes(original)
    comparison = compare(left, right)
    # Fresh means fresh: even a fully populated synthetic Library is refused.
    try:
        seed(left, binary, video)
    except FileExistsError:
        pass
    else:
        raise AssertionError('seed overwrote an existing Library')
    report = {'checks': first_checks, 'comparison': comparison, 'mutation_checks': 2,
              'video_sha256': hashlib.sha256(video.read_bytes()).hexdigest() if video else None}
    write_json(output / 'verification.json', report)
    return report


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--office', type=Path, default=REPO / 'target/debug/office')
    parser.add_argument('--video', type=Path)
    parser.add_argument('--existing', action='store_true', help='Recheck the two previously seeded roots without reseeding')
    args = parser.parse_args()
    operation = verify_existing if args.existing else verify
    print(json.dumps(operation(args.output.resolve(), args.office.resolve(), args.video.resolve() if args.video else None)))
