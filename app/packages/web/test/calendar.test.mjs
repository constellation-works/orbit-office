import assert from 'node:assert/strict';
import { test } from 'node:test';

import { screen } from '../dist/screens/calendar/index.js';
import {
  dayRuns, hourRange, meetingState, meetingsWithoutNotes, placeDay, stateWords, taskSlot, taskState, taskWords, weekStart,
} from '../dist/screens/calendar/model.js';
import { upcoming } from '../dist/screens/today/next-meeting.js';
import { navigableScreens } from '../dist/router.js';

// Local times, so the reading holds in any time zone the suite runs in.
const at = (day, hour, minute = 0) => new Date(2026, 9, day, hour, minute);
const now = at(28, 11, 30);
function meeting(title, start, end, extra = {}) {
  return { key: `${title}:${start.toISOString()}`, title, start: start.toISOString(), end: end.toISOString(), all_day: false, status: 'confirmed',
    location: '', online: false, organizer: '', attendees: [], people: [], agenda: '', private: false, prep: null, capture: null, ...extra };
}

test('Calendar is a navigable screen whose weeks start on Monday', () => {
  assert.ok(navigableScreens().includes(screen));
  assert.equal(navigableScreens().map(s => s.id).slice(0, 3).join(), 'today,calendar,todos');
  assert.deepEqual(weekStart(now), at(26, 0));
  assert.deepEqual(weekStart(at(32, 9)), at(26, 0), 'Sunday 1 November ends the week of Monday 26 October');
  assert.deepEqual(weekStart(at(33, 9)), at(33, 0), 'Monday 2 November starts the next week');
  assert.deepEqual(weekStart(at(26, 0)), at(26, 0));
});

test('meeting states are words for every case the spec names', () => {
  const lead = 120;
  const words = m => stateWords(m, now, lead).join(' · ');
  assert.equal(words(meeting('QBR', at(28, 14), at(28, 15), { prep: 'art-1' })), 'Prep ready');
  assert.equal(words(meeting('QBR', at(28, 14), at(28, 15))), 'Prep at 12:00');
  assert.equal(words(meeting('Late', at(28, 14, 10), at(28, 15))), 'Prep at 12:15', 'the prep routine runs on the quarter hour');
  assert.equal(words(meeting('Soon', at(28, 12), at(28, 13))), 'Prep on its way');
  assert.equal(words(meeting('Past', at(28, 9), at(28, 10))), 'Add notes');
  assert.equal(words(meeting('Past', at(28, 9), at(28, 10), { capture: { source: 'src-a', summarized: false } })), 'Notes added');
  assert.equal(words(meeting('Past', at(28, 9), at(28, 10), { capture: { source: 'src-a', summarized: true } })), 'Summary saved');
  assert.equal(words(meeting('Gone', at(29, 9), at(29, 10), { status: 'cancelled' })), 'Cancelled');
  assert.equal(words(meeting('Mine', at(29, 9), at(29, 10), { private: true, prep: 'art-2' })), 'Prep ready · Private');
  assert.equal(words(meeting('Maybe', at(29, 9), at(29, 10), { status: 'tentative', prep: 'art-3' })), 'Prep ready · Tentative');
  assert.equal(meetingState(meeting('Offsite', at(30, 0), at(31, 0), { all_day: true }), now, lead), null);
});

test('the rail counts today’s past meetings without notes, and Today finds the next one', () => {
  const meetings = [
    meeting('Yesterday', at(27, 9), at(27, 10)),
    meeting('Morning', at(28, 9), at(28, 10)),
    meeting('Noted', at(28, 10), at(28, 11), { capture: { source: 'src-a', summarized: false } }),
    meeting('Cancelled', at(28, 8), at(28, 9), { status: 'cancelled' }),
    meeting('Afternoon', at(28, 14), at(28, 15)),
    meeting('Lunch', at(28, 12), at(28, 13), { status: 'cancelled' }),
  ];
  assert.equal(meetingsWithoutNotes(meetings, now), 1);
  assert.equal(upcoming(meetings, now).title, 'Afternoon');
  assert.equal(upcoming(meetings, at(28, 15, 30)), null, 'nothing left today hides the card');
  assert.equal(upcoming([meeting('Running', at(28, 11), at(28, 12))], now).title, 'Running');
});

test('hours always show 08:00–18:00, widen to the meetings, and overlaps sit side by side', () => {
  const days = [at(26, 0), at(27, 0)];
  assert.deepEqual(hourRange([], days), { first: 8, last: 18 });
  assert.deepEqual(hourRange([meeting('Early', at(26, 7, 30), at(26, 8)), meeting('Late', at(27, 18), at(27, 19, 15))], days), { first: 7, last: 20 });
  const placed = placeDay([meeting('A', at(29, 13), at(29, 14)), meeting('B', at(29, 13, 30), at(29, 14, 30)), meeting('C', at(29, 15), at(29, 16))], at(29, 0));
  assert.deepEqual(placed.map(p => [p.item.title, p.lane, p.lanes]), [['A', 0, 2], ['B', 1, 2], ['C', 0, 1]]);
});

function task(title, notBefore, state, extra = {}) {
  return { id: `T-${title}`, title, not_before: notBefore.toISOString(), owner: 'agent', state, started_at: null, ...extra };
}

test('scheduled to-dos read their state in words, a late start by its time', () => {
  const words = item => taskWords(item).join(' · ');
  assert.equal(words(task('Numbers', at(30, 9), 'scheduled')), 'Scheduled · For an agent');
  assert.equal(words(task('Renewal', at(29, 10), 'not_approved')), 'Not approved · For an agent');
  assert.equal(words(task('Venue', at(29, 15), 'scheduled', { owner: 'you' })), 'Scheduled · For you');
  assert.equal(words(task('Board', at(28, 11), 'running', { started_at: at(28, 11, 1).toISOString() })), 'Running · For an agent', 'a minute after the time is on time');
  assert.equal(words(task('Pricing', at(28, 9), 'running', { started_at: at(28, 9, 42).toISOString() })), 'Started 09:42 · For an agent');
  assert.equal(words(task('Expenses', at(27, 8), 'done', { started_at: at(27, 8, 0).toISOString() })), 'Done · For an agent');
  assert.equal(words(task('Late done', at(27, 8), 'done', { started_at: at(27, 9, 15).toISOString() })), 'Done · Started 09:15 · For an agent');
  assert.equal(words(task('Review', at(27, 8), 'review')), 'Ready for review · For an agent');
  assert.equal(taskState(task('Renewal', at(29, 10), 'not_approved')).tone, 'attention');
});

test('a scheduled to-do sits at its time beside the meetings, widening the hours when it must', () => {
  const early = taskSlot(task('Early', at(28, 6, 30), 'scheduled'));
  assert.equal(Date.parse(early.end) - Date.parse(early.start), 30 * 60_000);
  assert.deepEqual(hourRange([early], [at(28, 0)]), { first: 6, last: 18 });
  const placed = placeDay([meeting('Design', at(28, 9), at(28, 9, 45)), taskSlot(task('Pricing', at(28, 9), 'running'))], at(28, 0));
  assert.deepEqual(placed.map(p => ['task' in p.item ? p.item.task.title : p.item.title, p.lane, p.lanes]), [['Design', 0, 2], ['Pricing', 1, 2]]);
});

test('routine runs come from the shared schedule reader: marks, bands for frequent ones, none for events', () => {
  const day = at(28, 0);
  const digest = dayRuns({ kind: 'routine', name: 'office-daily-digest', schedule: { cron: '5 0 * * *' }, enabled: true, last_fired_at: null }, day);
  assert.deepEqual(digest, { kind: 'marks', times: [at(28, 0, 5)] });
  assert.deepEqual(dayRuns({ kind: 'auto_task', name: 'r', schedule: { cron: '0 10 * * 1' }, enabled: true, last_fired_at: null }, day), { kind: 'marks', times: [] });
  assert.deepEqual(dayRuns({ kind: 'routine', name: 'p', schedule: { cron: '*/15 * * * *' }, enabled: false, last_fired_at: null }, day), { kind: 'band', words: 'Every 15 minutes' });
  assert.equal(dayRuns({ kind: 'routine', name: 's', schedule: { every_minutes: 15 }, enabled: true, last_fired_at: null }, day).kind, 'band');
  const interval = dayRuns({ kind: 'auto_task', name: 'k', schedule: { every_minutes: 720 }, enabled: false, last_fired_at: at(27, 6).toISOString() }, day);
  assert.deepEqual(interval, { kind: 'marks', times: [at(28, 6), at(28, 18)] });
  assert.equal(dayRuns({ kind: 'auto_task', name: 'e', schedule: { deliveries_landed: { threshold: 3 } }, enabled: true, last_fired_at: null }, day), null);
});
