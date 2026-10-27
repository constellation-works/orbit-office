#!/usr/bin/env python3
"""Seed a fresh, isolated Library through Office and Gateway exec tools.

The default is an explicitly simulated inference fixture. --provider-config
selects a real hosted endpoint for the private KT route, never a discovered
host credential. All task callbacks stay in the synthetic root.
"""
import argparse
from contextlib import ExitStack
from datetime import date, timedelta
import hashlib
import json
from pathlib import Path
import shutil
import sys

from common import FAKE_TOKEN, HERE, REPO, child_env, run, write_json
from mock_crm import serving
from model_fixture import ModelHandler
from native import prepare


class Demo:
    def __init__(self, root, binary):
        self.root = Path(root).resolve()
        # Refuse even an empty existing directory: no merge into a real Library.
        self.root.mkdir(mode=0o700, parents=True, exist_ok=False)
        (self.root / '.synthetic-demo').write_text('SIMULATED only\n')
        self.binary = Path(binary).resolve(strict=True)
        write_json(self.root / 'runtime.json', {'office': str(self.binary)})
        self.env = child_env(self.root)
        if not shutil.which('pdftotext', path=self.env['PATH']):
            tools = prepare(self.root, self.env)
            self.env['PATH'] = str(tools) + ':' + self.env['PATH']
        self.context = {'workspace_root': str(self.root), 'plugin_state': str(self.root / 'office-state'),
                        'agent': 'synthetic-demo', 'model': 'simulated', 'secrets': {}}
        self.calls = []
        self.sources = {}
        self.gateway_secrets = {}

    def call(self, tool, payload=None):
        context = dict(self.context)
        command = [self.binary, 'orbit-tool']
        env = dict(self.env)
        if tool.startswith('gateway.'):
            command = [sys.executable, HERE / 'gateway_entry.py']
            context['plugin_state'] = str(self.root / 'gateway-state')
            context['secrets'] = self.gateway_secrets
            env['ORBIT_PLUGIN_STATE'] = context['plugin_state']
        response = json.loads(run(command, input=json.dumps({'schema_version': 1, 'tool': tool,
                                  'input': payload or {}, 'context': context}), env=env, cwd=self.root, timeout=180))
        if not response.get('ok'):
            error = response.get('error', {})
            raise RuntimeError(f"{tool}: {error.get('code')}: {error.get('message')}")
        output = response['output']
        self.calls.append({'tool': tool, 'input': payload or {}, 'output': output})
        write_json(self.root / 'calls.json', self.calls)
        return output

    def capture(self, key, **payload):
        payload.setdefault('sensitivity', 'normal')
        payload.setdefault('origin', 'SIMULATED Acme Paper Kites demo')
        payload.setdefault('tags', ['simulated'])
        output = self.call('office.capture', payload)
        self.sources[key] = output['id']
        return output['id']

    def accept(self, proposal):
        return self.call('office.proposal-accept', {'id': proposal['id'], 'sha256': proposal['payload_sha256']})

    def proposal(self, page, evidence):
        proposal = self.call('office.proposal-create', {
            'title': 'SIMULATED About: ' + page['title'],
            'description': 'SIMULATED onboarding interview; review the cited synthetic answers.',
            'evidence': evidence, 'payload': {'kind': 'knowledge_add', 'page': page}, 'tags': ['simulated']})
        self.accept(proposal)

    def bootstrap(self, phase, sources):
        preview = self.call('office.batch-plan', {'phase': phase})
        self.call('office.batch-plan', {'phase': phase, 'confirm': True, 'digest': preview['plan']['digest']})
        for source in sources:
            self.call('office.extract', {'id': source, 'profile': 'digest'})
        result = self.call('office.consolidate', {'batch_id': phase})
        if result['failed'] or result['deferred']:
            raise RuntimeError('bootstrap left failed/deferred subjects: ' + json.dumps(result))
        db = json.loads((self.root / 'demo-orbit.json').read_text())
        for proposed in result['proposed']:
            task = db['tasks'][proposed['proposal']]
            payload = task['artifacts']['proposal.json']
            self.accept({'id': task['id'], 'payload_sha256': hashlib.sha256(payload.encode()).hexdigest()})
        return result


def seed(root, binary, video=None, provider_config=None):
    demo = Demo(root, binary)
    with ExitStack() as stack:
        crm = stack.enter_context(serving(port=4329))
        model = stack.enter_context(serving(ModelHandler, port=4330))
        model.audio_count = 0
        model.request_count = 0
        demo.call('office.init')
        demo.call('office.config-set', {'key': 'demo.simulated_until', 'value': '2026-10-25'})
        route = {'endpoint': f'http://127.0.0.1:{model.server_port}/v1',
                 'model': 'meta-models/Muse-Spark-Open-816B-A42B', 'kind': 'provider',
                 'label': 'Acme Inference (SIMULATED loopback)'}
        demo.call('office.config-set', {'key': 'open_weights', 'value': route})
        demo.call('office.private-route-check')
        # Bootstrap inference is private even in rehearsal: never send it to Meta.
        facts = demo.capture('interview', kind='conversation', title='SIMULATED onboarding interview',
            occurred_at='2026-09-14T09:00:00Z', entries=[
                {'kind': 'fact', 'text': 'SIMULATED: Jules Reed is the pilot lead at Acme Paper Kites.'},
                {'kind': 'fact', 'text': 'SIMULATED: Mira Vale owns Lantern spool recovery; Nori Chen reviews access.'},
                {'kind': 'fact', 'text': 'SIMULATED: Lantern routes paper-kite orders using a durable spool and original order keys.'},
                {'kind': 'fact', 'text': 'SIMULATED: Acme Paper Kites is an invented client running a Lantern pilot.'},
                {'kind': 'fact', 'text': 'SIMULATED: Jules prefers concise drafts with citations and manual submission.'}])
        cite = facts + '#L1'
        pages = [
            {'id': 'dom-user', 'title': 'About Jules Reed (simulated)', 'template': 'user',
             'sections': {'Name': f'SIMULATED: Jules Reed. ({cite})', 'Role': f'Pilot lead for Acme Paper Kites. ({cite})',
                          'Team': 'Works with [[dom-person-mira-vale]].', 'Duties': 'Owns [[dom-project-lantern]] for [[dom-client-acme-paper-kites]].',
                          'Working preferences': f'SIMULATED: concise cited drafts; manual submission. ({cite})'}},
            {'id': 'dom-person-mira-vale', 'title': 'Mira Vale (simulated)', 'template': 'person',
             'sections': {'Role': f'SIMULATED: Lantern spool owner. ({facts}#L2)', 'Works with': 'Works with Jules Reed (simulated).'}},
            {'id': 'dom-client-acme-paper-kites', 'title': 'Acme Paper Kites (simulated)', 'template': 'client',
             'sections': {'Overview': f'SIMULATED: invented customer running the Lantern pilot. ({facts}#L4)',
                          'People': 'Jules Reed (simulated) and [[dom-person-mira-vale]].'}},
            {'id': 'dom-project-lantern', 'title': 'Lantern (simulated)', 'template': 'project',
             'sections': {'Status': f'SIMULATED: pilot. ({facts}#L4)',
                          'Background': f'SIMULATED: paper-kite dispatch via durable spool and original keys. ({facts}#L3)',
                          'Stakeholders': 'Jules Reed (simulated), [[dom-person-mira-vale]], [[dom-client-acme-paper-kites]].'}},
        ]
        # Build dependencies first: every page link must already resolve.
        for page in pages[1:] + pages[:1]:
            demo.proposal(page, [cite])
        milestones = ['baseline and access', 'spool inspection', 'checkpoint rehearsal',
                      'timeout recovery', 'packing pilot', 'handoff and follow-up']
        start = date(2026, 9, 14)
        for day in range(42):
            current = start + timedelta(days=day)
            if current.weekday() >= 5:
                continue
            week = day // 7
            label = f'{current} / {milestones[week]}'
            demo.capture('meeting-' + str(current), kind='meeting', title='SIMULATED standup ' + label,
                         occurred_at=f'{current}T09:00:00Z', participants=['Jules Reed', 'Mira Vale'],
                         text=f'SIMULATED standup {label}\nMira: The spool checkpoint is saved for rehearsal {day + 1}.\nJules: I will prepare a cited handoff draft for review.\nDecision: Keep replay keys stable. No production action was taken.')
            demo.capture('chat-' + str(current), kind='chat', title='SIMULATED #lantern ' + label,
                         occurred_at=f'{current}T13:00:00Z', participants=['Jules Reed', 'Nori Chen'],
                         text=f'SIMULATED chat {label}\nJules: Does the Lantern-Readers request include replay?\nNori: No. Read-only access is enough for inspection. Review request {day + 1} before submission.')
            demo.capture('receipt-' + str(current), kind='receipt', title='SIMULATED receipt ' + str(current),
                         occurred_at=f'{current}T16:00:00Z',
                         text=f'SIMULATED receipt / invented Kite Cafe\nDate: {current}\nTraining refreshments: USD {12 + day % 7}.00\nDemo receipt number: KITE-{day + 1:03d}\nNo payment or real purchase occurred.')
            from orbit_fixture import callback
            work = callback('orbit.task.add', {'title': 'SIMULATED handoff ' + label, 'type': 'chore',
                  'description': 'Synthetic completed-work receipt; no real work or savings claimed.',
                  'created_at': f'{current}T09:00:00Z',
                  'tags': ['simulated', 'owner:agent' if day % 3 else 'owner:you']}, demo.root)['task']['id']
            for state in ['in-progress', 'review', 'done']:
                callback('orbit.task.update', {'id': work, 'status': state, 'at': f'{current}T17:00:00Z'}, demo.root)
        demo.capture('weekly-close', kind='note', title='SIMULATED final weekly recap',
            occurred_at='2026-10-25T17:00:00Z',
            text='SIMULATED: the six-week Lantern rehearsal ended today. The access request and CRM update remain drafts for review. No real work, access grant or payment occurred.')
        # Inputs are staged in the workspace, not copied into the Library.
        inputs = demo.root / 'inputs'
        inputs.mkdir(mode=0o700)
        onboarding = {}
        for name, phase in [('confluence.html', 'docs'), ('orientation.pptx', 'docs'),
                            ('pto-policy.pdf', 'process'), ('request-ad-group.md', 'process')]:
            shutil.copyfile(HERE / 'onboarding' / name, inputs / name)
            onboarding[name] = demo.capture(name, title='SIMULATED onboarding ' + name,
                upload_path=str(inputs / name), sensitivity='private', occurred_at='2026-09-14T10:00:00Z',
                tags=['simulated', 'bootstrap:' + phase])
        docs = demo.bootstrap('docs', [onboarding['confluence.html'], onboarding['orientation.pptx']])
        process = demo.bootstrap('process', [onboarding['pto-policy.pdf'], onboarding['request-ad-group.md']])
        # The runbook's template promises drafts. Materialize those explicit rules
        # through the public write API; bootstrap proposals retain their evidence.
        runbook_id = next(p['page'] for p in process['proposed'] if p['page'].startswith('rb-'))
        demo.call('office.knowledge-show', {'id': runbook_id})
        markdown = (demo.root / 'library' / 'knowledge' / 'runbooks' / (runbook_id + '.md')).read_text()
        if 'performable: agent' not in markdown:
            reviewed = markdown
            markdown = markdown.replace('---\n', '---\nperformable: agent\nsubmission:\n  system: Acme Access Desk (simulated)\n  via: draft\n', 1)
            update = demo.call('office.proposal-create', {'title': 'SIMULATED: make access runbook performable as a draft',
                'description': 'Record the user-reviewed draft boundary from the synthetic access process.',
                'evidence': [onboarding['request-ad-group.md'] + '#L1'], 'tags': ['simulated'],
                'payload': {'kind': 'knowledge_update', 'markdown': markdown,
                            'base_sha256': hashlib.sha256(reviewed.encode()).hexdigest()}})
            demo.accept(update)
        # Draft is a captured source: provenance survives and nothing is submitted.
        request = demo.capture('access-request', kind='note', title='SIMULATED draft: Lantern-Readers access',
            occurred_at='2026-10-23T12:00:00Z',
            text=f'SIMULATED DRAFT - waiting for your review\nStaff: jules-demo\nGroup: Lantern-Readers\nProject: Lantern\nApprover: Nori Chen\nJustification: inspect the fictional pilot spool read-only.\nForm: https://access.acme.example/request\nRunbook: {runbook_id}\nSource: {onboarding["request-ad-group.md"]}\nNot submitted; no access was granted.')
        # Draft the review item via the same callback fixture used by the tools.
        from orbit_fixture import callback
        draft_task = callback('orbit.task.add', {'title': 'SIMULATED: review the Lantern-Readers request', 'type': 'chore',
                  'description': f'Draft {request}; runbook {runbook_id}. Review and submit the form yourself.',
                  'tags': ['owner:you', 'simulated', 'source:' + request]}, demo.root)['task']['id']
        draft_file = demo.root / 'access-request.md'
        draft_file.write_text(demo.call('office.source-show', {'id': request})['body'])
        callback('orbit.task.artifact.put', {'id': draft_task, 'source_path': str(draft_file), 'path': 'access-request.md'}, demo.root)
        connection = demo.call('gateway.connection-put', {'name': 'acme-crm',
                'base_url': f'http://127.0.0.1:{crm.server_port}/v1', 'auth': {'scheme': 'bearer'},
                'methods': ['GET'], 'drafts': ['PATCH']})
        slot = connection['connection']['slot']
        # The connection's documented slot name is returned; never use a real key.
        demo.gateway_secrets[slot] = FAKE_TOKEN
        get = demo.call('gateway.http-get', {'connection': 'acme-crm', 'path': 'accounts/acme'})
        if FAKE_TOKEN in json.dumps(get) or '[secret]' not in json.dumps(get):
            raise RuntimeError('gateway did not redact the echoed fake token')
        draft = demo.call('gateway.http-draft', {'connection': 'acme-crm', 'method': 'PATCH',
            'path': 'accounts/acme', 'body': {'stage': 'review', 'note': 'SIMULATED pilot update; waiting for you'}})
        if draft['state'] != 'pending' or crm.patch_count != 0:
            raise RuntimeError('CRM write must remain a pending draft')
        callback('orbit.task.update', {'id': draft['task_id'],
            'title': 'SIMULATED: approve Acme CRM PATCH'}, demo.root)
        video_evidence = None
        video_source = None
        if provider_config and not video:
            raise ValueError('--provider-config requires --video')
        if video:
            if provider_config:
                config = json.loads(Path(provider_config).read_text())
                if config['kind'] != 'provider' or not config['endpoint'].startswith('https://'):
                    raise ValueError('live KT evidence needs an explicit HTTPS hosted provider')
                token_path = config.pop('token_file', None)
                if token_path:
                    demo.context['secrets']['open_weights_token'] = Path(token_path).read_text().strip()
                demo.call('office.config-set', {'key': 'open_weights', 'value': config})
                demo.call('office.private-route-check')
            target = inputs / 'lantern-kt.mp4'
            shutil.copyfile(video, target)
            source = demo.capture('kt-video', kind='video', title='SIMULATED Lantern KT', upload_path=str(target),
                sensitivity='private', occurred_at='2026-10-23T14:00:00Z')
            preview = demo.call('office.video-plan', {'id': source})
            demo.call('office.video-plan', {'id': source, 'confirm': True, 'digest': preview['plan']['digest']})
            demo.call('office.transcribe', {'id': source})
            demo.call('office.frames', {'id': source})
            task = callback('orbit.task.add', {'title': 'SIMULATED KT processing', 'tags': ['simulated']}, demo.root)['task']['id']
            video_evidence = demo.call('office.extract', {'id': source, 'profile': 'kt', 'task': task})
            video_source = demo.call('office.source-show', {'id': source})
        status = demo.call('office.status')
        bootstrap = demo.call('office.bootstrap-status')
        metrics = demo.call('office.metrics', {'since': '2026-09-14', 'until': '2026-11-01'})
        manifest = {'simulated': True, 'sources': demo.sources, 'last_seeded_day': '2026-10-25',
                    'about_pages': [p['id'] for p in pages], 'bootstrap': {'docs': docs, 'process': process},
                    'access_runbook': runbook_id, 'access_draft': request, 'crm_get': get, 'crm_draft': draft,
                    'crm_patch_requests': crm.patch_count, 'status': status, 'setup': bootstrap,
                    'video': video_evidence, 'video_source': video_source,
                    'hosted_provider_verified': bool(video and provider_config),
                    'model_fixture_requests': model.request_count, 'metrics': metrics}
        write_json(demo.root / 'seed-manifest.json', manifest)
        return manifest


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True, help='New directory; existing paths are refused')
    parser.add_argument('--office', type=Path, default=REPO / 'target/debug/office')
    parser.add_argument('--video', type=Path)
    parser.add_argument('--provider-config', type=Path)
    args = parser.parse_args()
    result = seed(args.root, args.office, args.video, args.provider_config)
    print(json.dumps({'simulated': True, 'sources': len(result['sources']), 'root': str(args.root),
                      'hosted_provider_verified': result['hosted_provider_verified']}))
