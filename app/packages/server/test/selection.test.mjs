import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test } from 'node:test';
import { createSelectionFeature } from '../dist/features/selection/index.js';

test('selection HTTP API peeks, shares private references without quotes, and clears', async t => {
  const calls = []; let tray = [];
  const feature = createSelectionFeature({ async callTool(name, input) {
    calls.push({ name, input });
    if (name === 'office.selection-get') return { selections: tray };
    if (input.clear) tray = []; else tray.push(input);
    return { ok: true, output: {} };
  } });
  const server = createServer((req, res) => { const route = feature.routes.find(r => r.method === req.method && r.path === req.url); if (route) void route.handle(req, res); else res.writeHead(404).end(); });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  const url = `http://127.0.0.1:${server.address().port}/api/selection`;
  const post = body => fetch(url, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  const count = async () => (await (await fetch(url)).json()).count;
  assert.equal(await count(), 0);
  assert.equal((await post({ object: 'src-20261027-notes', anchor: '#L1-2', quote: 'secret', view: 'source', sensitivity: 'private' })).status, 200);
  assert.deepEqual(calls.at(-1), { name: 'office.selection-put', input: { object: 'src-20261027-notes', anchor: '#L1-2', view: 'source', sensitivity: 'private' } });
  assert.equal(await count(), 1); assert.equal(await count(), 1);
  tray = []; // An agent consumed the tray between polls.
  assert.equal(await count(), 0);
  assert.equal((await post({ clear: true })).status, 200);
  assert.deepEqual(calls.at(-1), { name: 'office.selection-put', input: { clear: true } });
  assert.equal((await post({ clear: true, extra: 1 })).status, 400);
  assert.ok(calls.filter(call => call.name === 'office.selection-get').every(call => call.input.peek === true));
});
