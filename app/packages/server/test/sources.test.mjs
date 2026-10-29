import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as sources from '../dist/features/sources/index.js';

/** The repo's gitignored scratch dir; a fresh CI checkout has no .orbit/tmp yet. */
async function scratchDir() {
  const dir = fileURLToPath(new URL('../../../../.orbit/tmp/', import.meta.url));
  await mkdir(dir, { recursive: true });
  return dir;
}

async function fixture(t, callTool, options = {}) {
  const root = await mkdtemp(join(await scratchDir(), 'source-http-'));
  t.after(() => rm(root, { recursive: true }));
  const feature = sources.createSourcesFeature({ client: { callTool }, workspaceRoot: root, ...options });
  const server = createServer((req, res) => {
    const route = feature.routes.find(r => r.method === req.method && r.path === req.url.split('?')[0]);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  await new Promise(done => server.listen(0, '127.0.0.1', done));
  t.after(() => new Promise(done => server.close(done)));
  return { root, url: `http://127.0.0.1:${server.address().port}`, async request(path, body, headers = {}) {
    const response = await fetch(`http://127.0.0.1:${server.address().port}${path}`, body === undefined ? undefined : {
      method: 'POST', body: typeof body === 'string' || Buffer.isBuffer(body) ? body : JSON.stringify(body),
      headers: { 'content-type': Buffer.isBuffer(body) ? 'application/octet-stream' : 'application/json', ...headers },
    });
    return { status: response.status, body: await response.json() };
  } };
}

test('upload stages fixture bytes, captures exactly once and removes its private staging directory', async t => {
  const calls = [];
  const f = await fixture(t, async (name, input) => {
    calls.push({ name, input });
    assert.equal(name, 'office.capture');
    assert.equal(await readFile(input.upload_path, 'utf8'), 'fixture file');
    return { id: 'src-20261027-notes', kind: 'doc', title: 'Notes' };
  });
  const result = await f.request('/api/sources/upload?name=notes.txt&phase=docs', Buffer.from('fixture file'));
  assert.equal(result.status, 200);
  assert.equal(result.body.id, 'src-20261027-notes');
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0].input.tags, ['bootstrap:docs']);
  assert.deepEqual(await readdir(f.root), []);
});

const SOURCE = {
  id: 'src-20261027-notes', kind: 'doc', title: 'Notes', text: 'one\ntwo', sensitivity: 'normal', media_type: null,
  derived_by: { extraction: { route: 'hosted', model: 'fixture' } }, cited_by: [], chapters: [], frames: [], gotchas: [], task_citers_available: false, cited_by_truncated: false,
};
const SHA = 'a'.repeat(64);
const videoPlan = { digest: SHA, id: SOURCE.id, sensitivity: 'normal', duration_ms: 120000, counts: { audio: 1, frames: 3, extract: 1 }, estimate: null };

test('paste, source reads, privacy and citation reads go through the tools and project display data', async t => {
  const calls = [];
  const f = await fixture(t, async (name, input) => {
    calls.push({ name, input });
    if (name === 'office.capture') return { ...SOURCE };
    if (name === 'office.source-show') return { ok: true, output: { ...SOURCE, sensitivity: 'private', derived_by: { frames: { route: 'private:company', model: 'muse' } } } };
    if (name === 'office.status') return { open_weights: { label: 'Company server' } };
    if (name === 'office.source-set-sensitivity') return { id: SOURCE.id, sensitivity: 'private' };
    if (name === 'office.knowledge-show') return { page: { title: 'Runbook', body: 'Restart carefully.' } };
    throw Error('unexpected');
  });
  assert.equal((await f.request('/api/sources/paste', { title: 'Notes', text: 'one\ntwo', private: true })).body.id, SOURCE.id);
  assert.deepEqual(calls[0], { name: 'office.capture', input: { title: 'Notes', text: 'one\ntwo', kind: 'doc', captured_by: 'paste', sensitivity: 'private' } });
  const read = await f.request(`/api/sources?id=${SOURCE.id}`);
  assert.equal(read.status, 200); assert.deepEqual(read.body.route_labels, ['Private: Company server']);
  assert.equal(read.body.text, 'one\ntwo'); assert.equal(read.body.path, undefined);
  assert.equal((await f.request('/api/sources/private', { id: SOURCE.id })).status, 200);
  assert.deepEqual(calls.at(-1), { name: 'office.source-set-sensitivity', input: { id: SOURCE.id, sensitivity: 'private' } });
  assert.deepEqual((await f.request('/api/sources/citation?id=rb-restart&kind=runbook')).body, { title: 'Runbook', text: 'Restart carefully.' });
});

test('a withheld private source reports content_withheld with empty text', async t => {
  const f = await fixture(t, async name => {
    if (name === 'office.source-show') return { ok: true, output: { ...SOURCE, text: '', sensitivity: 'private', content_withheld: true, derived_by: {} } };
    if (name === 'office.status') return { open_weights: null };
    throw Error('unexpected tool');
  });
  const response = await f.request(`/api/sources?id=${SOURCE.id}`);
  assert.equal(response.status, 200);
  assert.equal(response.body.content_withheld, true);
  assert.equal(response.body.text, '');
});

test('a private hosted-provider source names the third party in its route badge', async t => {
  const f = await fixture(t, async name => {
    if (name === 'office.source-show') return { ...SOURCE, sensitivity: 'private', derived_by: {
      transcription: { route: 'private:provider', model: 'fixture' },
    } };
    if (name === 'office.status') return { open_weights: { kind: 'provider', label: 'Selected provider' } };
    throw Error('unexpected tool');
  });
  const response = await f.request(`/api/sources?id=${SOURCE.id}`);
  assert.equal(response.status, 200);
  assert.deepEqual(response.body.route_labels, ['Private: Selected provider (third party)']);
});

test('video and phase previews only plan; Process confirms the exact displayed digest and privacy', async t => {
  const calls = [];
  const f = await fixture(t, async (name, input) => {
    calls.push({ name, input });
    return { plan: name === 'office.video-plan' ? { ...videoPlan, sensitivity: input.sensitivity ?? 'normal' } : {
      digest: SHA, phase: 'docs', private: input.sensitivity === 'private', sources: [SOURCE.id], videos: [], counts: { digest: 1, audio: 0, frames: 0, extract: 0, careful: 2 }, estimate: { amount: 0.42, currency: 'USD' },
    } };
  });
  const preview = await f.request('/api/sources/plan', { id: SOURCE.id, private: false });
  assert.deepEqual(preview.body, { digest: SHA, source: SOURCE.id, private: false, minutes: 2, frames: 3, calls: 5, sources: 1, estimate: null });
  assert.deepEqual(calls, [{ name: 'office.video-plan', input: { id: SOURCE.id } }]);
  await f.request('/api/sources/plan', { id: SOURCE.id, private: true });
  await f.request('/api/sources/plan', { id: SOURCE.id, private: true, confirm: true, digest: SHA });
  assert.deepEqual(calls.at(-1), { name: 'office.video-plan', input: { id: SOURCE.id, sensitivity: 'private', confirm: true, digest: SHA } });
  assert.equal((await f.request('/api/sources/plan', { phase: 'docs', private: true })).body.calls, 3);
  await f.request('/api/sources/plan', { phase: 'docs', private: true, confirm: true, digest: SHA });
  assert.deepEqual(calls.at(-1), { name: 'office.batch-plan', input: { phase: 'docs', sensitivity: 'private', confirm: true, digest: SHA } });
});

test('unknown keys, unsafe filenames, missing digests and oversized uploads are refused before tool calls', async t => {
  const calls = []; const f = await fixture(t, async (...args) => { calls.push(args); return SOURCE; }, { maxUploadBytes: 4 });
  for (const [path, body] of [
    ['/api/sources/paste', { title: 'x', text: 'hello', private: false, extra: true }],
    ['/api/sources/private', { id: SOURCE.id, sensitivity: 'normal' }],
    ['/api/sources/plan', { id: SOURCE.id, private: false, confirm: true }],
    ['/api/sources/plan', { id: SOURCE.id, private: false, digest: SHA }],
    ['/api/sources/plan', { id: SOURCE.id, phase: 'docs', private: false }],
    ['/api/sources/upload?name=../notes.txt', Buffer.from('ok')],
    ['/api/sources/upload?name=script.exe', Buffer.from('ok')],
    ['/api/sources/upload?name=notes.txt&extra=yes', Buffer.from('ok')],
  ]) assert.equal((await f.request(path, body)).status, 400, path);
  assert.equal((await f.request('/api/sources/upload?name=notes.txt', Buffer.from('large'))).status, 413);
  assert.deepEqual(calls, []); assert.deepEqual(await readdir(f.root), []);
});

test('a refused capture cleans staging and is not retried; malformed source output fails visibly', async t => {
  const calls = []; const f = await fixture(t, async (name) => { calls.push(name); return { ok: false, error: { code: 'private_endpoint_unconfigured' } }; });
  assert.equal((await f.request('/api/sources/upload?name=notes.txt', Buffer.from('text'))).status, 409);
  assert.deepEqual(calls, ['office.capture']); assert.deepEqual(await readdir(f.root), []);
  const bad = await fixture(t, async () => ({ ...SOURCE, chapters: null }));
  assert.equal((await bad.request(`/api/sources?id=${SOURCE.id}`)).status, 502);
});


test('media serves complete bytes and bounded byte ranges for actual player seeking', async t => {
  const calls = []; const f = await fixture(t, async (name, input) => {
    calls.push({ name, input });
    return { media: { media_type: "video/webm", content_base64: Buffer.from("0123456789").toString("base64"), too_large: false } };
  });
  const path = `${f.url}/api/sources/media?id=${SOURCE.id}`;
  for (const [range, status, expected, contentRange] of [
    [undefined, 200, "0123456789", null], ["bytes=2-5", 206, "2345", "bytes 2-5/10"],
    ["bytes=5-", 206, "56789", "bytes 5-9/10"], ["bytes=-3", 206, "789", "bytes 7-9/10"],
    ["bytes=99-", 416, "", "bytes */10"], ["bytes=0-1,3-4", 416, "", "bytes */10"], ["bytes=-0", 416, "", "bytes */10"],
  ]) {
    const response = await fetch(path, { headers: range ? { Range: range } : {} });
    assert.equal(response.status, status); assert.equal(response.headers.get("content-range"), contentRange);
    assert.equal(await response.text(), expected);
  }
  assert.ok(calls.every(c => c.name === "office.source-show" && c.input.media === "original"));
});
