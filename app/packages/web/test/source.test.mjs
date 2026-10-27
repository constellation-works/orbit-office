import assert from 'node:assert/strict';
import { test } from 'node:test';
import { screenHash, sourceHref } from '../dist/api/source-location.js';
import { anchorTarget, droppedFiles, seconds } from '../dist/screens/source/model.js';
const view = { text: '[00:00:00] Start\n\n[00:01:00] Next', chapters: [{ number: 1, start: 60 }], frames: [{ time: 60 }] };

test('source queries preserve all citation fragments while ordinary navigation still selects its screen', () => {
  for (const hash of ['', '#t=00:01:00', '#L1-3', '#frame=00:01:00', '#chapter=1']) assert.equal(screenHash('?source=src-fixture', hash), '#source');
  assert.equal(screenHash('?source=src-fixture', '#library'), '#library');
  assert.equal(screenHash('', ''), '#today'); assert.equal(screenHash('', '#unknown'), '#unknown');
  assert.equal(sourceHref('src-space x', '#L2'), '?source=src-space%20x#L2');
});
test('line ranges and interior timestamps resolve to spans, while invalid and overflowing anchors refuse', () => {
  assert.deepEqual(anchorTarget(view, '#L1-3').lines, [0, 1, 2]);
  assert.deepEqual(anchorTarget(view, '#t=00:00:35'), { time: 35, lines: [0] });
  assert.deepEqual(anchorTarget(view, '#chapter=1'), { time: 60, lines: [2], chapter: 1 });
  assert.deepEqual(anchorTarget(view, '#frame=00:01:00'), { time: 60, lines: [2], frame: 0 });
  for (const hash of ['#L4', '#L3-1', '#frame=00:00:12', '#chapter=3', '#t=99:99:99', '#t=99999999999999999:00:00']) assert.equal(anchorTarget(view, hash), null);
  assert.equal(seconds('100:00:00'), 360000);
});
test('directory fallback preserves normal dropped files', async () => {
  const file = new File(['one'], 'one.txt'); assert.deepEqual(await droppedFiles({ items: [], files: [file] }), { folder: false, files: [file] });
});

test('a timestamp highlights the whole speaker turn, including continuation lines', () => {
  assert.deepEqual(anchorTarget({ ...view, text: '[00:00:00] Start\nContinuation\n\n[00:01:00] Next' }, '#t=00:00:35'), { time: 35, lines: [0, 1] });
});
