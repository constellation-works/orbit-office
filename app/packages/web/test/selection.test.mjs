import assert from 'node:assert/strict';
import { test } from 'node:test';
import { clearSelections, getSelectionCount, putSelection } from '../dist/api/client.js';

test('selection helpers peek through the app and send typed shares and clear requests', async t => {
  const original = globalThis.fetch; const calls = [];
  t.after(() => { globalThis.fetch = original; });
  globalThis.fetch = async (path, options) => {
    calls.push({ path, options });
    return new Response(JSON.stringify(options.method === 'POST' ? { ok: true } : { count: 2 }), { status: 200, headers: { 'content-type': 'application/json' } });
  };
  assert.deepEqual(await getSelectionCount(), { count: 2 });
  const input = { object: 'src-notes', anchor: '#L1', quote: 'words', view: 'source', sensitivity: 'normal' };
  await putSelection(input); await clearSelections();
  assert.equal(calls[0].path, '/api/selection');
  assert.deepEqual(JSON.parse(calls[1].options.body), input);
  assert.deepEqual(JSON.parse(calls[2].options.body), { clear: true });
});
