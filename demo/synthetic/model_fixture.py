"""Deterministic inference-edge fixture. It returns authored synthetic answers.

It exercises the real private HTTP adapter, not model quality or a hosted
provider. Live provider evidence must be gathered separately with seed.py's
explicit provider configuration.
"""
import json
import re

from mock_crm import Handler
from video import SCENES


def digest(text):
    process = 'Request Lantern-Readers AD group' in text
    policy = '# Page 1' in text and not process
    subject = 'Request Lantern-Readers AD group' if process else ('PTO allowance' if policy else 'Lantern dispatch system')
    subject += ' (simulated)'
    template = 'runbook' if process else 'topic'
    lines = re.findall(r'(#L\d+) ([^\n]*)', text)
    needle = '20 days' if policy else ('Confirm with Nori' if process else 'spool')
    anchor = next((anchor for anchor, line in lines if needle in line), '#L1')
    claim = ('Read access never grants replay permission.' if not policy else
             'SIMULATED: annual PTO allowance is 20 days; arrange coverage five working days ahead.')
    procedures = []
    if process:
        procedures = [{'title': subject, 'inputs': ['Synthetic staff handle', 'Lantern-Readers', 'Nori Chen approval'],
                       'steps': [{'text': 'Confirm read-only scope with Nori Chen.', 'cites': [anchor]},
                                 {'text': 'Prepare the access form with group and justification; wait for the user to submit.', 'cites': [anchor]}],
                       'done_when': 'A complete draft and form link are ready for user review; no request has been sent.'}]
    return {'summary': 'SIMULATED onboarding evidence for ' + subject, 'kind_hint': 'process' if process else 'orientation',
            'as_of': '2026-09-01', 'subjects': [{'name': subject, 'template': template, 'aliases': []}],
            'claims': [{'subject': subject, 'text': claim, 'cites': [anchor], 'volatile': False}],
            'procedures': procedures, 'terms': [], 'refs': []}


def careful(text):
    # The adapter prepends instructions/schema, followed by the subject JSON.
    data = json.loads(text[text.index('{"claims":'):])
    sections = re.findall(r'^## (.+)$', data['template'], re.M)
    evidence = '\n'.join(f"- {c['text']} ({', '.join(c['cites'])})" for c in data['claims'])
    result = []
    for section in sections:
        content = 'SIMULATED. ' + evidence
        if data['procedures']:
            procedure = data['procedures'][0]
            if section == 'Inputs':
                content = '\n'.join('- ' + v for v in procedure['inputs'])
            elif section == 'Steps':
                content = '\n'.join(f"{i}. {v['text']} ({', '.join(v['cites'])})" for i, v in enumerate(procedure['steps'], 1))
            elif section == 'Done when':
                content = procedure['done_when']
        result.append(f'## {section}\n\n{content}')
    return {'body': '\n\n'.join(result), 'conflicts': []}


def kt():
    return {'summary': 'SIMULATED Lantern KT: preserve the spool and original replay key.',
            'chapters': [{'title': title, 'label': label, 'start': i * 60, 'end': (i + 1) * 60}
                         for i, (title, _, label, _, _) in enumerate(SCENES)],
            'gotchas': [{'text': 'Never rotate the original replay key after a timeout.', 'cites': ['#t=00:08:30']}],
            'knowledge_candidates': [{'template': 'topic', 'target_page': 'Lantern dispatch system',
                                      'claims': [{'text': 'SIMULATED: the durable spool is the source of truth.',
                                                  'cites': ['#t=00:01:00'], 'volatile': False}]}],
            'runbook_candidates': [], 'glossary': [],
            'questions': [{'text': 'SIMULATED: confirm who authorizes a Lantern recovery window.', 'cites': ['#t=00:04:00']}]}


class ModelHandler(Handler):
    def do_POST(self):
        if not self.valid_origin() or self.path != '/v1/chat/completions':
            return self.answer(403, {'error': 'loopback authority and endpoint required'})
        length = int(self.headers.get('Content-Length', '0'))
        if length > 40 * 1024 * 1024:
            return self.answer(413, {'error': 'bounded synthetic input required'})
        data = json.loads(self.rfile.read(length))
        content = data['messages'][0]['content']
        schema = data.get('response_format', {}).get('json_schema', {}).get('schema', {})
        props = schema.get('properties', {})
        if 'segments' in props and len(json.dumps(content)) < 100_000:
            answer = {'text': 'SIMULATED capability probe.', 'segments': []}
        elif 'segments' in props:
            offset = self.server.audio_count * 600
            self.server.audio_count += 1
            segments = []
            for i, (_, _, _, first, second) in enumerate(SCENES):
                for speaker, speech in enumerate([first, second]):
                    start = i * 60 + speaker * 30
                    if offset <= start < offset + 600:
                        segments.append({'speaker': ['Mira', 'Jules'][speaker], 'start_ms': (start - offset) * 1000,
                                         'end_ms': (start - offset + 28) * 1000, 'text': speech})
            answer = {'text': '\n'.join(v['text'] for v in segments), 'segments': segments}
        elif 'subjects' in props:
            answer = digest(content)
        elif 'conflicts' in props:
            answer = careful(content)
        elif 'chapters' in props:
            answer = kt()
        else:
            answer = 'SIMULATED slide or terminal: Lantern uses the durable spool and original order key.'
        self.server.request_count += 1
        self.answer(200, {'model': 'meta-models/Muse-Spark-Open-816B-A42B',
                          'choices': [{'message': {'content': json.dumps(answer) if isinstance(answer, dict) else answer}}]})
