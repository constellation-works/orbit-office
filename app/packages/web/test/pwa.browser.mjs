/** Run after make app-build, with the verified browser kit environment. */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { createHandler } from "../../server/dist/http/router.js";

test("an open window offers the new build, preserves a draft, reloads and boots offline", { timeout: 60000 }, async (t) => {
  assert.ok(process.env.PLAYWRIGHT_MODULE, "source ORBIT_BROWSER_KIT_ENV first");
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const root = await mkdtemp(join(tmpdir(), "office-pwa-browser-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const stamp = fileURLToPath(new URL("../../../install/app_install.py", import.meta.url));
  const generations = [];
  for (const [index, label] of ["Build one", "Build two"].entries()) {
    const app = join(root, String(index));
    const web = join(app, "packages/web");
    await mkdir(web, { recursive: true });
    await cp(fileURLToPath(new URL("../dist/", import.meta.url)), join(web, "dist"), { recursive: true });
    await cp(fileURLToPath(new URL("../static/", import.meta.url)), join(web, "static"), { recursive: true });
    await writeFile(join(web, "static/index.html"), `<!doctype html><html><head><meta name="viewport" content="width=device-width"><link rel="stylesheet" href="/style.css"></head><body><h1>${label}</h1><label>Draft <textarea></textarea></label><script type="module">import { registerServiceWorker } from '/pwa/register.js'; registerServiceWorker();</script></body></html>`);
    execFileSync("python3", ["-B", stamp, "stamp", app, String(index + 1).repeat(40)], {
      env: { PATH: process.env.PATH, HOME: root, TMPDIR: root }, timeout: 10000,
    });
    generations.push({ web, info: JSON.parse(await readFile(join(web, "dist/build-info.json"), "utf8")) });
  }
  let active = 0;
  const handlers = generations.map(({ web }) => createHandler({ staticDir: join(web, "static"), distDir: join(web, "dist") }));
  const server = createServer((request, response) => handlers[active](request, response));
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  t.after(() => new Promise((done) => { server.closeAllConnections(); server.close(done); }));
  const url = `http://127.0.0.1:${server.address().port}`;
  assert.deepEqual(await (await fetch(`${url}/build-info.json`)).json(), generations[0].info);
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const context = await browser.newContext({ serviceWorkers: "allow", viewport: { width: 375, height: 800 } });
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(url);
  await page.evaluate(() => navigator.serviceWorker.ready);
  await page.waitForFunction(() => navigator.serviceWorker.controller !== null);
  assert.equal(await page.getByRole("button", { name: "Reload", exact: true }).count(), 0, "first install is not an update");
  await page.getByRole("textbox", { name: "Draft" }).fill("Keep my unsaved draft");
  active = 1;
  await page.evaluate(async () => (await navigator.serviceWorker.getRegistration()).update());
  await page.getByText("A new version of Office is ready.").waitFor();
  assert.equal(await page.getByRole("textbox", { name: "Draft" }).inputValue(), "Keep my unsaved draft");
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  if (process.env.PWA_SCREENSHOTS_DIR) {
    await mkdir(process.env.PWA_SCREENSHOTS_DIR, { recursive: true });
    for (const width of [375, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      await page.screenshot({ path: join(process.env.PWA_SCREENSHOTS_DIR, `update-${width}.png`) });
    }
  }
  await Promise.all([page.waitForNavigation(), page.getByRole("button", { name: "Reload", exact: true }).click()]);
  await page.getByRole("heading", { name: "Build two" }).waitFor();
  assert.equal(await page.getByRole("button", { name: "Reload", exact: true }).count(), 0);
  const caches = await page.evaluate(() => globalThis.caches.keys());
  assert.deepEqual(caches, [`office-shell-${generations[1].info.build}`]);
  await context.setOffline(true);
  await page.goto(url + "/offline-route");
  await page.getByRole("heading", { name: "Build two" }).waitFor();
  assert.equal(await page.evaluate(async () => (await import("/pwa/register.js")).registerServiceWorker instanceof Function), true);
  assert.deepEqual(errors, []);
});
