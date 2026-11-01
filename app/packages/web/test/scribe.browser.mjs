/**
 * The Scribe screen in a real browser, on the production scribe feature and
 * the two-session fixture Library (server/test/scribe-fixture.mjs). Run with
 * the verified host browser kit after `npm --prefix app run check`; set
 * SCRIBE_SCREENSHOTS_DIR to a directory under `.orbit/tmp/` to capture each
 * view at 320px and 1440px.
 */
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { test } from "node:test";

import { NOW, office, scribeServer } from "../../server/test/scribe-fixture.mjs";

const WIDTHS = [320, 1440];

async function browserFixture(t) {
  assert.ok(process.env.PLAYWRIGHT_MODULE, "source the verified ORBIT_BROWSER_KIT_ENV first");
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true }); t.after(() => browser.close());
  const context = await browser.newContext({ timezoneId: "UTC", serviceWorkers: "block", viewport: { width: 1440, height: 1000 } });
  const page = await context.newPage(); const errors = [];
  page.on("pageerror", (error) => errors.push(error.message)); t.after(() => assert.deepEqual(errors, []));
  page.setDefaultTimeout(5000);
  await page.clock.install({ time: NOW });
  return { ...(await scribeServer(t)), page };
}
async function noOverflow(page, name) {
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${name}: no horizontal scroll at ${page.viewportSize().width}px`);
}
async function shot(page, name) {
  const dir = process.env.SCRIBE_SCREENSHOTS_DIR; if (!dir) return;
  await mkdir(dir, { recursive: true });
  await page.screenshot({ path: resolve(dir, `${name}-dark-${page.viewportSize().width}.png`), fullPage: true });
}
const session = (page, title) => page.getByRole("region", { name: title });
const tab = (page, name) => page.getByRole("tab", { name, exact: true });
const results = (page) => page.getByRole("tabpanel");

test("the feed shows each line with its tags, quote, time and file:line, the most recent session first", { timeout: 60000 }, async (t) => {
  const { page, url } = await browserFixture(t);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`${url}/?width=${page.viewportSize().width}#scribe`);
    await page.getByRole("heading", { name: "Scribe", exact: true, level: 1 }).waitFor();
    const recent = session(page, "Scribe screen");
    await recent.getByText("Most recent").waitFor();
    await recent.getByText("orbit-office · 4 lines").waitFor();
    const blocks = page.locator(".scribe-session h3");
    assert.deepEqual(await blocks.allInnerTexts(), ["Scribe screen", "Plan the scribe v1 rollout keep it model free"]);
    await session(page, "Plan the scribe v1 rollout keep it model free").getByText("constellation · 4 lines").waitFor();
    // Newest first inside a session, each with its anchors.
    const quotes = await recent.locator(".scribe-quote").allInnerTexts();
    assert.deepEqual(quotes, ["Landed the scribe-ask tool as #287.", "ORB-20780 is green and merged.", "poll every ten seconds", "CI for the screen is still running."]);
    for (const row of office.feed.results) {
      const item = page.locator(`[data-citation="${row.citation}"]`);
      await item.getByText(row.file_line, { exact: true }).waitFor();
      await item.getByText(`#${row.tag}`, { exact: true }).waitFor();
      assert.equal(await item.locator("time").getAttribute("datetime"), row.time);
      await item.getByText(`claude --resume ${row.session_id}`, { exact: true }).waitFor();
    }
    await noOverflow(page, "feed"); await shot(page, "scribe-feed");
  }
});

test("Pending, Preferences, Landed and Why show the lines office.scribe-query returns", { timeout: 60000 }, async (t) => {
  const { page, url } = await browserFixture(t);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`${url}/?width=${page.viewportSize().width}#scribe`);
    await session(page, "Scribe screen").waitFor();
    for (const [name, view] of [["Pending", "pending"], ["Preferences", "pref"], ["Landed", "landed"], ["Why", "why"]]) {
      await tab(page, name).click();
      assert.equal(await tab(page, name).getAttribute("aria-selected"), "true");
      await results(page).locator(".scribe-quote").first().waitFor();
      assert.deepEqual(await results(page).locator(".scribe-quote").allInnerTexts(), office[view].results.map((row) => row.quote), name);
      if (view === "pref") await results(page).getByText("Stale · review by 2026-10-31").waitFor();
      if (view === "landed") await page.getByText(`Today, ${office.landed.today} in ${office.landed.time_zone}.`).waitFor();
      if (view === "why") {
        await results(page).locator(`[data-citation="${office.why.results[0].citation}"]`).getByText("no recorded reason").waitFor();
        await results(page).getByText("a socket is overkill for one window").waitFor();
      }
      await noOverflow(page, name); await shot(page, `scribe-${view}`);
    }
  }
});

test("a note is recorded as one by: user line and shows in the feed after the next poll", { timeout: 60000 }, async (t) => {
  const { page, url, calls } = await browserFixture(t);
  await page.setViewportSize({ width: 320, height: 1000 });
  await page.goto(`${url}/?width=${page.viewportSize().width}#scribe`);
  await session(page, "Scribe screen").waitFor();
  const form = page.getByRole("form", { name: "Add a note" });
  assert.equal(await form.getByLabel("Session").inputValue(), office.after_note.results[0].session_id, "the most recent session is chosen");
  await form.getByLabel("Note, recorded verbatim").fill("#todo call the venue at 3pm");
  await form.getByText("Tag: #todo, from the text").waitFor();
  await noOverflow(page, "note form"); await shot(page, "scribe-note");
  await form.getByRole("button", { name: "Add note" }).click();
  await form.getByText(`Recorded at ${office.note.path}:25.`, { exact: false }).waitFor();
  const appends = calls.filter((call) => call.name === "office.scribe-append");
  assert.deepEqual(appends.map((call) => call.input.entries), [[{ tag: "todo", by: "user", text: "call the venue at 3pm" }]]);
  const recent = session(page, "Scribe screen");
  assert.equal(await recent.getByText("call the venue at 3pm").count(), 0, "not shown before the poll");
  await page.clock.runFor(10_000);
  const note = recent.locator(".scribe-entry").first();
  await note.getByText("call the venue at 3pm").waitFor();
  await note.getByText("#todo", { exact: true }).waitFor();
  await note.locator("dd", { hasText: "user" }).waitFor();
  await noOverflow(page, "feed with the note"); await shot(page, "scribe-feed-noted");
});

test("the ask box answers with SCR file:line, drops a mismatched quote, and says not captured with the range", { timeout: 60000 }, async (t) => {
  const { page, url } = await browserFixture(t);
  const decision = office.why.results.find((row) => row.why !== null);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`${url}/?width=${page.viewportSize().width}#scribe`);
    const form = page.getByRole("form", { name: "Ask the scribe" });
    await form.getByLabel("Question").fill("How often does the screen poll?");
    await form.getByRole("button", { name: "Ask" }).click();
    const claim = form.locator(".scribe-claim");
    await claim.getByText("The screen polls every ten seconds.").waitFor();
    await claim.getByText(decision.file_line, { exact: true }).waitFor();
    assert.deepEqual(await claim.locator(".scribe-quote").allInnerTexts(), ["poll every ten seconds"]);
    assert.equal(await form.getByText("a websocket for every window").count(), 0, "the mismatched quote is not shown");
    await form.getByText("2 quotes were dropped: not found on the cited line.", { exact: false }).waitFor();
    await noOverflow(page, "answer"); await shot(page, "scribe-ask-answered");
    await form.getByLabel("Question").fill("What did we order for lunch?");
    await form.getByRole("button", { name: "Ask" }).click();
    await form.getByText("Not captured. Searched 2026-10-31 to 2026-11-01 across 2 files; topics recall, scribe.").waitFor();
    await noOverflow(page, "not captured"); await shot(page, "scribe-ask-not-captured");
  }
});
