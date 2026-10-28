import assert from 'node:assert/strict';
import { createServer, request as httpRequest } from 'node:http';
import { mkdtemp, readdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { test } from 'node:test';

import { createCalendarFeature } from '../dist/features/calendar/index.js';
import { createHandler } from '../dist/http/router.js';
import { fakeCalendarClient, prepArtifact, scratchDir, weekEnd, weekStart } from './calendar-fixture.mjs';

async function serve(t, overrides = {}, options = {}) {
  const root = await mkdtemp(join(await scratchDir(), 'calendar-http-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const fake = fakeCalendarClient(overrides);
  const feature = createCalendarFeature({ client: fake.client, workspaceRoot: root, ...options });
  const server = createServer((req, res) => {
    const route = feature.routes.find(r => r.method === req.method && r.path === req.url.split('?')[0]);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  await new Promise(done => server.listen(0, '127.0.0.1', done));
  t.after(() => new Promise(done => server.close(done)));
  const url = `http://127.0.0.1:${server.address().port}`;
  return { ...fake, root, url, async get(path) {
    const response = await fetch(`${url}${path}`);
    return { status: response.status, body: await response.json() };
  }, async post(path, body, headers = {}) {
    const binary = Buffer.isBuffer(body);
    const response = await fetch(`${url}${path}`, { method: 'POST', body: binary ? body : JSON.stringify(body), headers: { 'content-type': binary ? 'application/octet-stream' : 'application/json', ...headers } });
    return { status: response.status, body: await response.json() };
  } };
}
const week = `/api/calendar?from=${encodeURIComponent(weekStart)}&until=${encodeURIComponent(weekEnd)}`;

test('a populated range lists meetings with linked attendees and the shipped routines only', async t => {
  const f = await serve(t);
  const { status, body } = await f.get(week);
  assert.equal(status, 200);
  assert.equal(body.synced_at, '2026-10-28T10:15:00Z');
  assert.equal(body.sync_failed, false);
  assert.equal(body.prep_lead_minutes, 120);
  assert.equal(body.meetings.length, 9);
  assert.deepEqual(f.calls.find(c => c.name === 'office.calendar-list').input, { from: weekStart, until: weekEnd, include_cancelled: true });
  const qbr = body.meetings.find(m => m.title === 'Acme QBR');
  assert.equal(qbr.prep, prepArtifact);
  assert.deepEqual(qbr.attendees, [
    { name: 'Jane Park', response: 'accepted', person: { id: 'dom-jane-park', title: 'Jane Park' } },
    { name: 'Sam Lee', response: 'tentative', person: { id: 'dom-sam-lee', title: 'Sam Lee' } },
    { name: 'Robin Ortiz', response: 'needsAction', person: null },
  ]);
  assert.deepEqual(qbr.people, [{ id: 'dom-acme-finance', title: 'Acme finance desk' }]);
  assert.equal(qbr.organizer, 'Jane Park');
  assert.equal(body.meetings.find(m => m.title === 'One-to-one with Sam').private, true);
  assert.equal(body.meetings.find(m => m.title === 'Vendor call').status, 'cancelled');
  assert.deepEqual(body.meetings.find(m => m.title === 'Weekly sync').capture, { source: 'src-20261026-weekly-sync', summarized: true });
  // Sorted by start, whatever order the snapshot holds.
  assert.deepEqual(body.meetings.map(m => m.start), [...body.meetings.map(m => m.start)].sort((a, b) => Date.parse(a) - Date.parse(b)));
  assert.deepEqual(body.routines.map(r => [r.kind, r.name, r.enabled]).sort(), [
    ['auto_task', 'office-knowledge-maintenance', false], ['auto_task', 'office-review', true],
    ['routine', 'office-daily-digest', true], ['routine', 'office-meeting-prep', false],
  ]);
  assert.equal(body.routines.find(r => r.name === 'office-knowledge-maintenance').last_fired_at, '2026-10-25T12:00:00Z');
  assert.equal(body.routines_unavailable, false);
  assert.ok(f.calls.some(c => c.name === 'orbit.auto_task.list'));
});

test('scheduled to-dos in the range come from orbit.task.list with the not_before range, with their state and start', async t => {
  const f = await serve(t);
  const { status, body } = await f.get(week);
  assert.equal(status, 200);
  const read = f.calls.find(c => c.name === 'orbit.task.list' && c.input.not_before_from !== undefined);
  assert.deepEqual(read.input, { not_before_from: weekStart, not_before_until: weekEnd, fields: ['id', 'title', 'status', 'tags', 'not_before', 'history'], limit: 200 });
  assert.deepEqual(body.tasks.map(task => [task.title, task.state, task.owner, task.started_at]), [
    ['File the expense report', 'done', 'agent', '2026-10-27T08:00:20Z'],
    ['Refresh the pricing sheet', 'running', 'agent', '2026-10-28T09:42:10Z'],
    ['Prepare the board pack', 'running', 'agent', '2026-10-28T11:00:40Z'],
    ['Draft the renewal summary', 'not_approved', 'agent', null],
    ['Call the venue', 'scheduled', 'you', null],
    ['Send the weekly numbers', 'scheduled', 'agent', null],
  ], 'sorted by time; rejected tasks, tasks that are not to-dos and next week are left out');
  assert.equal(body.tasks.find(task => task.title === 'Call the venue').not_before, '2026-10-29T15:00:00Z');
  assert.equal(body.scheduled_unavailable, false);
});

test('a to-do scheduled at the very end of the range belongs to the next one', async t => {
  const f = await serve(t, { tasks: [{ id: 'EDGE-1', title: 'Monday first thing', status: 'backlog', tags: ['owner:agent'], not_before: weekEnd, history: [] }] });
  const { body } = await f.get(week);
  assert.deepEqual(body.tasks, []);
  assert.equal(body.scheduled_unavailable, false);
});

test('an Orbit that cannot read the not_before range keeps the meetings and says the scheduled to-dos are missing', async t => {
  const f = await serve(t, { scheduledDenied: true });
  const { status, body } = await f.get(week);
  assert.equal(status, 200);
  assert.equal(body.meetings.length, 9);
  assert.deepEqual(body.tasks, []);
  assert.equal(body.scheduled_unavailable, true);
});

test('a malformed scheduled row is dropped and reported, never guessed at', async t => {
  const f = await serve(t, { tasks: [
    { id: 'BAD-1', title: 'No time', status: 'backlog', tags: ['owner:agent'], not_before: '2026-10-29T08:00:00Z', history: [] },
    { id: 'OK-1', title: 'Fine', status: 'backlog', tags: ['owner:you'], not_before: '2026-10-29T09:00:00Z', history: [] },
  ] });
  // The first row comes back from the range read with a time no one can read.
  const original = f.client.callTool;
  f.client.callTool = async (name, input) => {
    const value = await original(name, input);
    if (name === 'orbit.task.list' && input.not_before_from !== undefined) value.tasks[0].not_before = 'Friday';
    return value;
  };
  const { body } = await f.get(week);
  assert.deepEqual(body.tasks.map(task => task.title), ['Fine']);
  assert.equal(body.scheduled_unavailable, true);
});

test('a never-synced snapshot reads synced_at null, distinct from a synced empty week', async t => {
  const never = await serve(t, { synced_at: null, events: [] });
  const first = await never.get(week);
  assert.equal(first.status, 200);
  assert.equal(first.body.synced_at, null);
  assert.deepEqual(first.body.meetings, []);
  const empty = await serve(t, { events: [] });
  const second = await empty.get(week);
  assert.equal(second.body.synced_at, '2026-10-28T10:15:00Z');
  assert.deepEqual(second.body.meetings, []);
});

test('a failed latest sync keeps the last good snapshot and says the sync failed', async t => {
  const f = await serve(t, { last_error: 'gateway_unreachable' });
  const { status, body } = await f.get(week);
  assert.equal(status, 200);
  assert.equal(body.sync_failed, true);
  assert.equal(body.meetings.length, 9);
  assert.ok(!JSON.stringify(body).includes('gateway_unreachable'), 'the safe code stays on the server');
});

test('a routine permission refusal still shows the meetings and the auto-tasks', async t => {
  const f = await serve(t, { routinesDenied: true });
  const { status, body } = await f.get(week);
  assert.equal(status, 200);
  assert.equal(body.meetings.length, 9);
  assert.deepEqual(body.routines.map(r => r.name).sort(), ['office-knowledge-maintenance', 'office-review']);
  assert.equal(body.routines_unavailable, true);
});

test('ranges are validated before any tool runs', async t => {
  const f = await serve(t);
  for (const path of ['/api/calendar', `/api/calendar?from=${weekStart}`, `/api/calendar?from=yesterday&until=${weekEnd}`,
    `/api/calendar?from=${weekEnd}&until=${weekStart}`, `/api/calendar?from=2026-10-01T00:00:00Z&until=2026-12-01T00:00:00Z`,
    `${week}&include_cancelled=false`]) {
    const result = await f.get(path);
    assert.equal(result.status, 400, path);
    assert.equal(result.body.error, 'invalid_input');
  }
  assert.equal(f.calls.filter(c => c.name === 'office.calendar-list').length, 0);
});

test('sync calls office.calendar-sync once and reports a refusal as a fixed code', async t => {
  const f = await serve(t, { synced_at: null });
  const ok = await f.post('/api/calendar/sync', {});
  assert.equal(ok.status, 200);
  assert.deepEqual(Object.keys(ok.body).sort(), ['events', 'skipped', 'synced_at']);
  assert.deepEqual(f.calls.filter(c => c.name === 'office.calendar-sync').map(c => c.input), [{}]);
  f.state.syncError = 'calendar_unconfigured';
  assert.deepEqual(await f.post('/api/calendar/sync', {}), { status: 409, body: { error: 'calendar_unconfigured' } });
  f.state.syncError = 'gateway_unreachable';
  assert.deepEqual(await f.post('/api/calendar/sync', {}), { status: 502, body: { error: 'sync_failed' } });
  assert.equal((await f.post('/api/calendar/sync', { ics_file: 'demo/synthetic/calendar.ics' })).status, 400);
});

test('capture of pasted notes links the meeting by event key', async t => {
  const f = await serve(t);
  const key = 'design:2026-10-28T09:00:00Z';
  const result = await f.post('/api/calendar/capture', { event_key: key, text: 'Decided to ship the beta.' });
  assert.equal(result.status, 200);
  assert.deepEqual(result.body, { id: 'src-20261028-meeting-1', kind: 'meeting', title: 'Design review' });
  assert.deepEqual(f.captures[0].input, { event_key: key, text: 'Decided to ship the beta.', captured_by: 'paste' });
  const listed = await f.get(week);
  assert.deepEqual(listed.body.meetings.find(m => m.key === key).capture, { source: 'src-20261028-meeting-1', summarized: false });
});

test('capture of an uploaded transcript stages it owner-only and removes it after', async t => {
  const f = await serve(t);
  const key = 'acme-prep:2026-10-27T10:00:00Z';
  const result = await f.post(`/api/calendar/capture?event_key=${encodeURIComponent(key)}&name=call.vtt`, Buffer.from('WEBVTT\n\n00:00.000 --> 00:01.000\nHello'));
  assert.equal(result.status, 200);
  assert.equal(f.captures[0].text, 'WEBVTT\n\n00:00.000 --> 00:01.000\nHello');
  assert.equal(f.captures[0].input.event_key, key);
  assert.equal(f.captures[0].input.captured_by, 'upload');
  assert.match(f.captures[0].input.upload_path, /\.office-upload-[^/]+\/call\.vtt$/);
  assert.deepEqual(await readdir(f.root), []);
  for (const name of ['run.exe', '.hidden.txt', 'a/b.txt']) {
    const refused = await f.post(`/api/calendar/capture?event_key=${encodeURIComponent(key)}&name=${encodeURIComponent(name)}`, Buffer.from('x'));
    assert.equal(refused.status, 400, name);
    assert.equal(refused.body.error, 'unsupported_file');
  }
  assert.equal(f.captures.length, 1);
});

test('capture with an unknown event is refused as unknown_event and creates nothing', async t => {
  const f = await serve(t);
  const pasted = await f.post('/api/calendar/capture', { event_key: 'gone:2026-10-01T09:00:00Z', text: 'Notes' });
  assert.deepEqual(pasted, { status: 404, body: { error: 'unknown_event' } });
  const uploaded = await f.post('/api/calendar/capture?event_key=gone&name=notes.md', Buffer.from('# Notes'));
  assert.deepEqual(uploaded, { status: 404, body: { error: 'unknown_event' } });
  assert.deepEqual(await readdir(f.root), []);
  assert.equal(f.state.sources.size, 1, 'only the seeded source');
  assert.equal(f.captures.length, 0);
  for (const body of [{ text: 'Notes' }, { event_key: 'x', text: ' ' }, { event_key: 'x', text: 'a', title: 'b' }]) {
    assert.equal((await f.post('/api/calendar/capture', body)).status, 400);
  }
});

test('a captured meeting reads its summary and to-dos without ids', async t => {
  const f = await serve(t);
  const key = 'design:2026-10-28T09:00:00Z';
  const { body: source } = await f.post('/api/calendar/capture', { event_key: key, text: 'Notes' });
  const pending = await f.get(`/api/calendar/meeting?source=${source.id}`);
  assert.deepEqual(pending, { status: 200, body: { summary: null, todos: [], more: false } });
  f.state.sources.get(source.id).summary = 'We agreed to ship the beta.';
  f.state.tasks.push({ id: 'T-1', title: 'Send the beta notes', status: 'backlog', tags: ['owner:you', `source:${source.id}`] });
  const done = await f.get(`/api/calendar/meeting?source=${source.id}`);
  assert.deepEqual(done.body, { summary: 'We agreed to ship the beta.', todos: [{ title: 'Send the beta notes', owner: 'you', done: false }], more: false });
  assert.equal((await f.get('/api/calendar/meeting?source=../etc')).status, 400);
});

test('calendar routes sit behind the loopback guard and healthz lists the feature', async t => {
  const server = createServer(createHandler());
  await new Promise(done => server.listen(0, '127.0.0.1', done));
  t.after(() => new Promise(done => server.close(done)));
  const port = server.address().port;
  const send = (method, path, headers) => new Promise((resolve, reject) => {
    const req = httpRequest({ host: '127.0.0.1', port, method, path, headers }, res => {
      let data = ''; res.on('data', chunk => { data += chunk; }); res.on('end', () => resolve({ status: res.statusCode, body: data }));
    });
    req.on('error', reject); req.end(method === 'POST' ? '{}' : undefined);
  });
  for (const [method, path] of [['GET', week], ['POST', '/api/calendar/sync'], ['POST', '/api/calendar/capture'], ['GET', '/api/calendar/meeting?source=src-1']]) {
    const evil = await send(method, path, { host: 'evil.example.com', 'content-type': 'application/json' });
    assert.equal(evil.status, 403, `${method} ${path} with a foreign Host`);
    assert.equal(JSON.parse(evil.body).error, 'forbidden');
    const crossSite = await send(method, path, { host: `127.0.0.1:${port}`, origin: 'https://evil.example.com', 'content-type': 'application/json' });
    assert.equal(crossSite.status, 403, `${method} ${path} from a foreign Origin`);
  }
  // A state-changing request with no Origin is refused too.
  assert.equal((await send('POST', '/api/calendar/sync', { host: `127.0.0.1:${port}`, 'content-type': 'application/json' })).status, 403);
  const health = JSON.parse((await send('GET', '/healthz', { host: `127.0.0.1:${port}` })).body);
  assert.ok(health.features.includes('calendar'));
});
