/**
 * The Scribe screen in a real browser, on the production scribe feature
 * and the fixture outputs (server/test/scribe-fixture.mjs). Run with the
 * verified host browser kit after `npm --prefix app run check`; set
 * SCRIBE_SCREENSHOTS_DIR to a directory under `.orbit/tmp/` to capture
 * each view at 320px and 1440px, the open SCR and the Tags refusal
 * included, for the task's screenshot evidence.
 */
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { resolve } from "node:path";
import { test } from "node:test";

import { NOW, agreement, cards, decisions, office, preferences, scribeServer } from "../../server/test/scribe-fixture.mjs";

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
const region = (page, name) => page.getByRole("region", { name, exact: true });
const FEED = "Feed · one card per project";
const DECISIONS = "Decisions · newest first";
const cardOf = (page, id) => region(page, FEED).locator(`[data-id="${id}"]`);

test("the feed shows exactly one card per project plus the global card, and opening one walks only the previous chain", { timeout: 60000 }, async (t) => {
  const { page, url, calls } = await browserFixture(t);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`${url}/?width=${page.viewportSize().width}#scribe`);
    await page.getByRole("heading", { name: "Scribe", exact: true, level: 1 }).waitFor();
    assert.deepEqual(await page.locator(".panel-head h2").evaluateAll((items) => items.map((item) => item.textContent)), [DECISIONS, FEED, "Preferences", "Agreement", "Tags"]);
    await page.getByText("Couldn’t read open contradictions. We’ll try again shortly.").waitFor();
    const ids = await region(page, FEED).locator("[data-id]").evaluateAll((items) => items.map((item) => item.dataset.id));
    assert.deepEqual(ids, cards.feed.cards.map((card) => card.id), "one card per project plus global; the older orbit-office SCR is absent");
    const latest = cardOf(page, "SCR-20261031-0900-aaaa02");
    await latest.getByText("orbit-office", { exact: true }).waitFor();
    await latest.getByText("(scribe) Rework the Scribe tab into the one-card feed.").waitFor();
    await latest.getByText("ORB-21120 — finish the Tags editor. owner: agent.").waitFor();
    await latest.getByText("Next", { exact: true }).waitFor();
    await latest.getByText("Outcome and Blockers", { exact: true }).waitFor();
    await latest.getByText("2 decisions · 1 preference").waitFor();
    await cardOf(page, "SCR-20261029-0900-cccc01").getByText("Global", { exact: true }).waitFor();
    await cardOf(page, "SCR-20261028-0900-dddd01").getByText("Private session: content withheld.").waitFor();
    await noOverflow(page, "feed"); await shot(page, "scribe-feed");

    // Opening shows the whole SCR; Previous reaches the older one, and stops there.
    await latest.getByRole("button", { name: "Open Ship the scribe feed" }).click();
    const detail = cardOf(page, "SCR-20261031-0900-aaaa02");
    await detail.getByRole("heading", { name: "Ship the scribe feed" }).waitFor();
    await detail.getByText("SCR-20261031-0900-aaaa02#T2").waitFor();
    await detail.getByText("#decision ship v2 first because: the feed is unusable").waitFor();
    await shot(page, "scribe-scr-open");
    await detail.getByRole("button", { name: /Previous/ }).click();
    const older = cardOf(page, "SCR-20261030-0900-aaaa01");
    await older.getByRole("heading", { name: "Design the scribe feed" }).waitFor();
    assert.equal(await older.getByRole("button", { name: /Previous/ }).count(), 0, "the chain's end has no Previous");
    assert.ok(calls.some((call) => call.name === "office.session-brief" && call.input.id === "SCR-20261030-0900-aaaa01"));
    await shot(page, "scribe-scr-previous");
    await older.getByRole("button", { name: "Back to the feed" }).click();
    await cardOf(page, "SCR-20261029-0900-cccc01").waitFor();
    await noOverflow(page, "feed again");
  }
});

test("Preferences and Decisions show the office.scribe-query panels", { timeout: 60000 }, async (t) => {
  const { page, url } = await browserFixture(t);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`${url}/?width=${page.viewportSize().width}#scribe`);
    await cardOf(page, "SCR-20261031-0900-aaaa02").waitFor();
    for (const row of preferences.preferences) {
      const item = region(page, "Preferences").locator(`[data-citation="${row.citation}"]`);
      await item.getByText(row.statement).waitFor();
    }
    await region(page, "Preferences").getByText("Stale · review by 2026-10-31").waitFor();
    await noOverflow(page, "preferences"); await shot(page, "scribe-preferences");

    const ledger = region(page, DECISIONS);
    await ledger.locator('[data-id="dec-orbit-office-8"]').waitFor();
    const ids = await ledger.locator("[data-id]").evaluateAll((items) => items.map((item) => item.dataset.id));
    assert.deepEqual(ids, [...decisions.decisions].reverse().map((row) => row.id), "newest first");
    const first = ledger.locator('[data-id="dec-orbit-office-7"]');
    await first.getByText("the feed is unusable", { exact: true }).waitFor();
    await first.getByText("Standing", { exact: true }).waitFor();
    await ledger.locator('[data-id="dec-orbit-office-8"]').getByText("no recorded reason").waitFor();
    await noOverflow(page, "decisions"); await shot(page, "scribe-decisions");
  }
});

test("the Agreement view shows per-category aggregates only, low-sample in words, and no actions, at 375px", { timeout: 60000 }, async (t) => {
  const { page, url } = await browserFixture(t);
  await page.setViewportSize({ width: 375, height: 1000 });
  await page.goto(`${url}/#scribe`);
  await cardOf(page, "SCR-20261031-0900-aaaa02").waitFor();
  const section = region(page, "Agreement");
  await section.getByRole("heading", { name: "Agreement", exact: true }).waitFor();
  const enough = section.locator(`[data-category="${agreement.categories[0].category}"]`);
  await enough.getByText("ws_orbit-office · agent request").waitFor();
  await enough.getByText("81% (68%–90%)").waitFor();
  await enough.getByText("74%").waitFor();
  const low = section.locator(`[data-category="${agreement.categories[1].category}"]`);
  await low.getByText("Not enough decisions yet").waitFor();
  assert.equal(await section.getByRole("button").count(), 0, "the Agreement view has no actions");
  assert.equal(await section.locator("input, select, textarea, a").count(), 0, "read-only: no controls or links");
  assert.ok(!(await section.innerText()).includes("prediction"), "no per-request prediction text");
  await noOverflow(page, "agreement"); await shot(page, "scribe-agreement");
  await page.setViewportSize({ width: 320, height: 1000 });
  await noOverflow(page, "agreement 320");
});

test("the Tags editor reads the effective tags and, refused by Orbit, shows not-permitted with the operator command and turns read-only", { timeout: 60000 }, async (t) => {
  const { page, url, calls } = await browserFixture(t);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`${url}/?width=${page.viewportSize().width}#scribe`);
    await cardOf(page, "SCR-20261031-0900-aaaa02").waitFor();
    const form = page.getByRole("form", { name: "Tag editor" });
    await region(page, "Tags").getByText("#preferences").waitFor();
    await region(page, "Tags").getByText("#decisions").waitFor();
    await region(page, "Tags").getByText("#lesson").waitFor();
    await form.getByRole("button", { name: "Restore defaults" }).waitFor();
    await noOverflow(page, "tags"); await shot(page, "scribe-tags");

    await form.getByLabel("Name").fill("incident");
    await form.getByLabel("Instruction").fill("Something broke and what settled it.");
    await form.getByRole("button", { name: "Save tag" }).click();
    await form.getByText("The app isn't allowed to change the scribe settings yet.", { exact: false }).waitFor();
    const command = await form.locator("code").first().innerText();
    assert.match(command, /^orbit tool run office\.config-set --input '/);
    assert.match(command, /scribe\.tags\.incident/);
    assert.equal(await form.getByText("Saved.", { exact: true }).count(), 0, "a refused save claims no success");
    assert.equal(await form.getByRole("button", { name: "Save tag" }).isDisabled(), true, "the editor is read-only after the refusal");
    assert.equal(await form.getByLabel("Name").isDisabled(), true);
    assert.ok(calls.some((call) => call.name === "office.config-set"));
    await noOverflow(page, "tags refused"); await shot(page, "scribe-tags-not-permitted");
  }
});

test("the ask box answers with SCR file:line, drops a mismatched quote, and says not captured with the range", { timeout: 60000 }, async (t) => {
  const { page, url } = await browserFixture(t);
  const decision = office.why.results.find((row) => row.why !== null);
  for (const width of WIDTHS) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`${url}/?width=${page.viewportSize().width}#scribe`);
    const form = page.getByRole("form", { name: "Ask the ledger" });
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
