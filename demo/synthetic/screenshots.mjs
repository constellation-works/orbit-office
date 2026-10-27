// Capture actual product views with real seeded data. The server is always reaped.
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { chmod, mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const [rootArg, outputArg, mode] = process.argv.slice(2);
if (!rootArg || !outputArg || !process.env.PLAYWRIGHT_MODULE) throw new Error('usage: screenshots.mjs <seed-root> <output>; source the verified browser kit first');
const root = resolve(rootArg), output = resolve(outputArg);
const manifest = JSON.parse(await readFile(resolve(root, 'seed-manifest.json'), 'utf8'));
await mkdir(output, { recursive: true, mode: 0o700 });
const bin = resolve(root, 'preview-bin'); await mkdir(bin, { mode: 0o700, recursive: true });
// Fixed source path, quoted as a shell string. No credentials or user prose.
const quote = value => `'${value.replaceAll("'", "'\\''")}'`;
await writeFile(resolve(bin, 'orbit'), `#!/bin/sh\nexec ${quote(resolve(here, 'orbit_fixture.py'))} "$@"\n`);
await chmod(resolve(bin, 'orbit'), 0o700);
const env = { PATH: `${bin}:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin`,
              HOME: resolve(root, 'home'), TMPDIR: resolve(root, 'tmp'), LANG: 'en_US.UTF-8', TZ: 'UTC',
              ORBIT_ROOT: root, ORBIT_REGISTRY_ROOT: resolve(root, 'registry'),
              ORBIT_WORKSPACE: root, ORBIT_OFFICE_WORKSPACE: root, ORBIT_BIN: resolve(here, 'orbit_fixture.py') };
const child = spawn(process.execPath, [resolve(here, 'preview_server.mjs')], { env, cwd: root, detached: true, stdio: ['ignore', 'pipe', 'pipe'] });
child.stderr.resume();
let browser;
try {
  const address = await new Promise((accept, reject) => {
    const deadline = setTimeout(() => reject(new Error('preview startup timed out')), 15000);
    child.once('error', error => { clearTimeout(deadline); reject(error); });
    child.once('exit', () => { clearTimeout(deadline); reject(new Error('preview exited')); });
    child.stdout.once('data', data => { clearTimeout(deadline); accept(data.toString().trim()); });
  });
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  browser = await chromium.launch({ headless: true });
  let screens = [['insights', '#insights', 'Automation over time'], ['library', '#library', 'Library setup'],
                   ['source', `?source=${manifest.sources['kt-video']}#source`, 'Marked private'],
                   ['needs-you', '#today', 'SIMULATED: approve Acme CRM PATCH'],
                   ['todos', '#todos', 'SIMULATED: review the Lantern-Readers request']];
  if (mode === '--before') screens = screens.filter(([name]) => ['insights', 'source'].includes(name));
  const results = [];
  for (const theme of ['light', 'dark']) {
    for (const width of [1280, 375]) {
      const page = await browser.newPage({ viewport: { width, height: 1000 }, colorScheme: theme });
      for (const [name, path, expected] of screens) {
        // Hash-only navigation can still contain the prior view's text while
        // the next screen loads. Start a document and assert its own heading.
        await page.goto('about:blank');
        await page.goto(address + '/' + path);
        const heading = { insights: 'Insights', library: 'Library', source: 'SIMULATED Lantern KT',
                          'needs-you': 'Today', todos: 'To-dos' }[name];
        await page.getByRole('heading', { name: heading, exact: true, level: 1 }).waitFor();
        await page.getByText(expected, { exact: false }).first().waitFor();
        await page.locator('#site-footer').getByText('Built with Muse Spark Open', { exact: false }).waitFor();
        if (name === 'insights' && mode !== '--before') {
          await page.getByText('2026-09-14:', { exact: false }).waitFor();
          const weeks = await page.locator('.insights-weeks li').allTextContents();
          if (weeks.filter(v => v.includes('simulated')).length !== 6 || weeks.find(v => v.startsWith('2026-10-26:'))?.includes('simulated')) throw new Error('simulated cutoff is wrong');
        }
        if (name === 'source') {
          if (mode !== '--before') await page.getByText('third party', { exact: false }).first().waitFor();
          await page.waitForFunction(() => { const player = document.querySelector('video'); return player && (player.readyState >= 1 || player.error !== null); });
        }
        if (name === 'needs-you') await page.getByText('Through your “acme-crm” connection:', { exact: true }).waitFor();
        const file = `${name}-${theme}-${width}.png`;
        await page.screenshot({ path: resolve(output, file), fullPage: true });
        results.push(file);
      }
      await page.close();
    }
  }
  await writeFile(resolve(output, 'manifest.json'), JSON.stringify({ screenshots: results, data: 'actual seeded Library; simulated loopback inference; read-only real app' }, null, 2));
} finally {
  if (browser) await browser.close();
  if (child.exitCode === null && child.signalCode === null) {
    const signal = name => { if (child.exitCode === null && child.signalCode === null) {
      try { process.kill(-child.pid, name); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    } };
    const exited = once(child, 'exit'); signal('SIGTERM');
    const timeout = setTimeout(() => signal('SIGKILL'), 40000);
    await exited; clearTimeout(timeout);
  }
}
