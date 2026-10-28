/**
 * Scheduling from the To-dos screen in a real browser, on the production
 * to-do feature and the fixture Orbit client of the Calendar suite
 * (server/test/calendar-fixture.mjs). Run with the verified host browser kit
 * after `npm --prefix app run check`; set CALENDAR_SCREENSHOTS_DIR to a
 * directory under `.orbit/tmp/` to capture each view at 1440px and 375px.
 */
import assert from 'node:assert/strict';
import { mkdir } from 'node:fs/promises';
import { resolve } from 'node:path';
import { test } from 'node:test';

import { calendarServer, now } from '../../server/test/calendar-fixture.mjs';

async function browserFixture(t) {
  assert.ok(process.env.PLAYWRIGHT_MODULE, 'source the verified ORBIT_BROWSER_KIT_ENV first');
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true }); t.after(() => browser.close());
  const context = await browser.newContext({ timezoneId: 'UTC', serviceWorkers: 'block', viewport: { width: 1440, height: 1000 } });
  const page = await context.newPage(); const errors = [];
  page.on('pageerror', error => errors.push(error.message)); t.after(() => assert.deepEqual(errors, []));
  page.setDefaultTimeout(5000);
  await page.clock.setFixedTime(now);
  return { ...(await calendarServer(t)), page };
}
async function noOverflow(page, name) {
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${name}: no horizontal overflow at ${page.viewportSize().width}px`);
}
async function shot(page, name) {
  const dir = process.env.CALENDAR_SCREENSHOTS_DIR; if (!dir) return;
  await mkdir(dir, { recursive: true });
  await page.screenshot({ path: resolve(dir, `${name}-dark-${page.viewportSize().width}.png`), fullPage: true });
}
const todo = (page, title) => page.locator('.todos-column .card', { has: page.getByRole('heading', { name: title, exact: true }) });

test('Schedule… sets a to-do’s time, which then reads in words, and Clear schedule removes it', { timeout: 60000 }, async t => {
  const { page, url, state, calls } = await browserFixture(t);
  const stored = id => state.tasks.find(task => task.id === id).not_before;
  for (const width of [1440, 375]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`${url}/#todos`);
    const venue = todo(page, 'Call the venue');
    await venue.getByText('Scheduled Thu 15:00', { exact: true }).waitFor();
    await venue.getByRole('button', { name: 'Reschedule…' }).waitFor();
    await venue.getByRole('button', { name: 'Clear schedule' }).waitFor();
    // An agent's to-do can be scheduled too; one already running keeps its time and offers nothing.
    await todo(page, 'Send the renewal draft').getByRole('button', { name: 'Schedule…', exact: true }).waitFor();
    const running = todo(page, 'Refresh the pricing sheet');
    await running.getByText('Scheduled Wed 09:00', { exact: true }).waitFor();
    assert.equal(await running.getByRole('button').count(), 0);
    await noOverflow(page, 'to-dos'); await shot(page, 'todos-scheduled');
  }

  await page.setViewportSize({ width: 1440, height: 1000 });
  const draft = todo(page, 'Send the renewal draft');
  await draft.getByRole('button', { name: 'Schedule…', exact: true }).click();
  const picker = draft.getByRole('form', { name: 'Schedule' });
  assert.equal(await picker.getByLabel('Date and time').inputValue(), '2026-10-29T09:00', 'it suggests tomorrow at 09:00');
  assert.match(await picker.innerText(), /In this computer's time zone, UTC\./);
  await shot(page, 'todos-picker');
  await picker.getByLabel('Date and time').fill('2026-10-30T09:00');
  await picker.getByRole('button', { name: 'Save' }).click();
  await draft.getByText('Scheduled Fri 09:00', { exact: true }).waitFor();
  assert.equal(stored('FIXTURE-7'), '2026-10-30T09:00:00Z');
  assert.equal(await draft.getByRole('form').count(), 0);

  await page.setViewportSize({ width: 375, height: 1000 });
  await draft.getByRole('button', { name: 'Reschedule…' }).click();
  await noOverflow(page, 'picker on a phone'); await shot(page, 'todos-picker');
  await draft.getByRole('button', { name: 'Cancel' }).click();
  await draft.getByRole('button', { name: 'Clear schedule' }).click();
  await draft.getByRole('button', { name: 'Schedule…', exact: true }).waitFor();
  assert.equal(await draft.locator('.todos-when').count(), 0);
  assert.equal(stored('FIXTURE-7'), null);
  assert.equal(state.tasks.find(task => task.id === 'FIXTURE-7').status, 'backlog', 'scheduling moves no status');
  assert.deepEqual(calls.filter(c => c.name === 'orbit.task.update').map(c => c.input), [
    { id: 'FIXTURE-7', not_before: '2026-10-30T09:00:00Z' }, { id: 'FIXTURE-7', not_before: '' },
  ]);
  assert.equal((await page.locator('main').innerText()).includes('FIXTURE-'), false, 'no task ids are shown');
});
