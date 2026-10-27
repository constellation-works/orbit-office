import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { test } from 'node:test';
import { sourceServer, sourceId, sourceData, digest } from '../../server/test/source-fixture.mjs';
import { videoBytes } from '../../server/test/source-media.mjs';

async function browserFixture(t) {
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true }); t.after(() => browser.close());
  const page = await browser.newPage({ serviceWorkers: 'block' }); const errors = [];
  page.on('pageerror', error => errors.push(error.message)); t.after(() => assert.deepEqual(errors, []));
  const server = await sourceServer(t); return { ...server, page };
}
async function shot(page, name) {
  const dir = process.env.SOURCE_SCREENSHOTS_DIR; if (!dir) return;
  await mkdir(dir, { recursive: true });
  for (const width of [1440, 375]) {
    await page.setViewportSize({ width, height: 1000 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${name} at ${width}`);
    await page.screenshot({ path: resolve(dir, `${name}-light-${width}.png`), fullPage: true });
  }
}

test('source links select the built view, highlight and scroll spans, and seek the actual video for all time anchors', { timeout: 60000 }, async t => {
  const { page, url, sources } = await browserFixture(t);
  for (const [anchor, time, selector] of [
    ['t=00:00:01', 1, '.source-line[data-highlight=true]'],
    ['frame=00:00:01', 1, '.source-frame[data-highlight=true]'],
    ['chapter=2', 2, '.source-chapter[data-highlight=true]'],
  ]) {
    await page.goto(`${url}/?source=${sourceId}#${anchor}`);
    await page.waitForFunction(time => { const v = document.querySelector('video'); return v?.readyState >= 1 && Math.abs(v.currentTime - time) < 0.1; }, time);
    assert.equal(await page.locator(selector).count(), 1);
    assert.equal(await page.getByText('Hosted', { exact: true }).count(), 1);
    assert.equal(await page.getByText('Gotcha: Never restart on Friday.').isVisible(), true);
    if (anchor.startsWith('chapter')) assert.equal(await page.locator('.source-chapter details').evaluate(node => node.open), true);
    if (anchor.startsWith('t=')) { assert.equal(await page.locator('.source-chapter details').evaluate(node => node.open), false); await shot(page, 'source-video'); }
  }
  await page.goto(`${url}/?source=${sourceId}#L3-5`);
  await page.locator('.source-line[data-highlight=true]').first().waitFor(); assert.equal(await page.locator('.source-line[data-highlight=true]').count(), 3);
  assert.ok(await page.locator('.source-line[data-highlight=true]').first().evaluate(node => { const box = node.getBoundingClientRect(); return box.top < innerHeight && box.bottom > 0; }));
  await page.getByRole('link', { name: 'Restart the ingest worker', exact: true }).click(); await page.getByText('Check the queue before restarting.').waitFor();
  await page.getByRole('link', { name: 'Check queue capacity', exact: true }).click(); await page.getByText('Confirm the queue is empty.').waitFor();
  await page.getByRole('button', { name: 'Mark private', exact: true }).click(); await page.getByRole('button', { name: 'Marked private', exact: true }).waitFor();
  sources.set(sourceId, { ...sourceData(), sensitivity: 'private', derived_by: { frames: { route: 'private:company', model: 'fixture' } } });
  await page.reload(); await page.getByText('Private: Company server', { exact: true }).waitFor(); await shot(page, 'source-private');
  sources.set(sourceId, { ...sourceData(), kind: 'image', media_type: 'image/png', title: 'Queue diagram', text: 'The queue feeds the workers.', chapters: [], frames: [], gotchas: [] });
  await page.reload(); await page.getByRole('img', { name: 'Queue diagram' }).waitFor(); await page.waitForFunction(() => document.querySelector('.source-original img')?.naturalWidth > 0); await shot(page, 'source-image');
  sources.set(sourceId, { ...sourceData(), kind: 'meeting', media_type: 'audio/wav', title: 'Handover audio', chapters: [], frames: [], gotchas: [] });
  await page.goto(`${url}/?source=${sourceId}#t=00:00:01`);
  await page.waitForFunction(() => document.querySelector('audio')?.currentTime === 1); await shot(page, 'source-audio');
});

test('fixture file uploads and paste each capture once and open the new source', { timeout: 60000 }, async t => {
  const { page, url, captures } = await browserFixture(t);
  await page.goto(`${url}/#source`); await page.getByRole('heading', { name: 'Add a source' }).waitFor(); await shot(page, 'source-capture');
  await page.getByLabel('Choose files', { exact: true }).setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('First line\nSecond line') });
  await page.getByRole('heading', { name: 'notes.txt', exact: true }).waitFor(); assert.equal(captures.length, 1);
  assert.equal(captures[0].text, 'First line\nSecond line'); await shot(page, 'source-text');
  await page.getByText('Add another source', { exact: true }).click();
  await page.getByLabel('Source title', { exact: true }).fill('Planning notes'); await page.getByLabel('Pasted text', { exact: true }).fill('Bring the checklist.');
  await page.getByRole('button', { name: 'Save pasted text' }).click(); await page.getByRole('heading', { name: 'Planning notes', exact: true }).waitFor();
  assert.equal(captures.length, 2); assert.equal(captures[1].input.captured_by, 'paste');
});

test('a dropped video only captures and plans until Process; private replanning replaces the approved digest', { timeout: 60000 }, async t => {
  const { page, url, calls } = await browserFixture(t);
  await page.goto(`${url}/#source`); await page.getByRole('heading', { name: 'Add a source' }).waitFor();
  await page.locator('.source-drop').evaluate((node, data) => {
    const transfer = new DataTransfer(); transfer.items.add(new File([new Uint8Array(data)], 'handover.webm', { type: 'video/webm' }));
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, dataTransfer: transfer }));
  }, Array.from(videoBytes));
  await page.getByRole('heading', { name: 'Video preview', exact: true }).waitFor();
  assert.deepEqual(calls.map(c => c.name), ['office.capture', 'office.video-plan']);
  await page.getByText('1 source · 3 minutes · 3 frames · 5 model calls').waitFor(); await page.getByText('Rates unset', { exact: true }).waitFor(); await shot(page, 'source-video-preview');
  await page.getByRole('region', { name: 'Processing preview' }).getByRole('checkbox').check();
  await page.getByRole('button', { name: 'Process', exact: true }).waitFor();
  await page.waitForFunction(() => document.querySelector('.source-plan input')?.disabled === true);
  assert.equal(calls.at(-1).input.sensitivity, 'private'); assert.equal(calls.some(c => c.input.confirm === true), false);
  await page.getByRole('button', { name: 'Process', exact: true }).click();
  await page.getByRole('heading', { name: 'handover.webm', exact: true }).waitFor();
  const confirmed = calls.find(c => c.input.confirm === true);
  assert.equal(confirmed.name, 'office.video-plan'); assert.equal(confirmed.input.digest, 'd'.repeat(64)); assert.equal(confirmed.input.sensitivity, 'private');
});

test('folder drop drains nested entries, captures supported files once with the selected phase, then confirms its private batch', { timeout: 60000 }, async t => {
  const { page, url, calls, captures } = await browserFixture(t);
  await page.goto(`${url}/#source`); await page.getByRole('heading', { name: 'Add a source' }).waitFor();
  await page.getByLabel('Folder phase').selectOption('team');
  await page.locator('.source-drop').evaluate(node => {
    const file = (name, text) => ({ isFile: true, isDirectory: false, file(ok) { ok(new File([text], name)); } });
    const directory = entries => ({ isFile: false, isDirectory: true, createReader() { let i = 0; return { readEntries(ok) { ok(i < entries.length ? [entries[i++]] : []); } }; } });
    const entry = directory([file('one.txt', 'One'), directory([file('two.md', 'Two'), file('ignore.exe', 'Not supported')])]);
    const transfer = { items: [{ webkitGetAsEntry: () => entry }], files: [] };
    const event = new Event('drop', { bubbles: true, cancelable: true }); Object.defineProperty(event, 'dataTransfer', { value: transfer }); node.dispatchEvent(event);
  });
  await page.getByRole('heading', { name: 'Team preview', exact: true }).waitFor();
  assert.deepEqual(captures.map(c => c.title), ['one.txt', 'two.md']); for (const c of captures) assert.deepEqual(c.input.tags, ['bootstrap:team']);
  assert.deepEqual(calls.map(c => c.name), ['office.capture', 'office.capture', 'office.batch-plan']);
  await page.getByText('Estimated cost: 0.25 USD', { exact: true }).waitFor(); await shot(page, 'source-batch-preview');
  await page.getByRole('region', { name: 'Processing preview' }).getByRole('checkbox').check();
  await page.getByRole('button', { name: 'Process', exact: true }).click(); await page.getByRole('button', { name: 'Confirmed', exact: true }).waitFor();
  assert.deepEqual(calls.at(-1), { name: 'office.batch-plan', input: { phase: 'team', sensitivity: 'private', confirm: true, digest: 'e'.repeat(64) } });
});
