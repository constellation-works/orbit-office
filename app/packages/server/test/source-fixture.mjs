import { createServer } from 'node:http';
import { mkdir, mkdtemp, readFile, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createSourcesFeature } from '../dist/features/sources/index.js';
import { createHandler } from '../dist/http/router.js';
import { videoBytes, frameBytes, audioBytes } from './source-media.mjs';

/** The repo's gitignored scratch dir; a fresh CI checkout has no .orbit/tmp yet. */
async function scratchDir() {
  const dir = fileURLToPath(new URL('../../../../.orbit/tmp/', import.meta.url));
  await mkdir(dir, { recursive: true });
  return dir;
}

export const sourceId = 'src-20261027-ingest';
export const digest = 'c'.repeat(64);
export function sourceData(id = sourceId) {
  return { id, kind: 'video', title: 'Ingest handover', sensitivity: 'normal', media_type: 'video/webm',
    text: '[00:00:00] Alex: Start with the dashboard.\n\n[00:00:01] Alex: Check the ingest queue.\n\n[00:00:02] Alex: Never restart on Friday.',
    derived_by: { transcription: 'vtt-import', frames: { route: 'hosted', model: 'fixture' } },
    chapters: [{ number: 1, title: 'The ingest queue', label: 'core', start: 0, anchor: '#t=00:00:00' }, { number: 2, title: 'Friday stories', label: 'tangent', start: 2, anchor: '#t=00:00:02' }],
    frames: [{ time: 1, anchor: '#frame=00:00:01', file: '0000.png', text: 'Queue dashboard' }],
    gotchas: [{ text: 'Never restart on Friday.', cites: [`${id}#t=00:00:02`] }],
    cited_by: [{ id: 'rb-ingest', kind: 'runbook', title: 'Restart the ingest worker' }, { id: 'FIXTURE-1', kind: 'task', title: 'Check queue capacity' }],
    task_citers_available: true, cited_by_truncated: false,
  };
}
export async function sourceServer(t) {
  const root = await mkdtemp(join(await scratchDir(), 'source-browser-'));
  t.after(() => rm(root, { recursive: true }));
  const calls = []; const captures = []; const sources = new Map([[sourceId, sourceData()]]);
  const client = { async callTool(name, input) {
    calls.push({ name, input });
    if (name === 'office.capture') {
      const title = input.title; const id = `src-20261027-upload-${captures.length + 1}`;
      const kind = /\.(mp4|webm|mov)$/.test(title) ? 'video' : 'doc';
      const text = input.text ?? (await readFile(input.upload_path)).toString('utf8');
      captures.push({ title, text, input });
      sources.set(id, kind === 'video' ? { ...sourceData(id), title } : { ...sourceData(id), title, kind, text, media_type: null, chapters: [], frames: [], gotchas: [], derived_by: {} });
      return { id, kind, title };
    }
    if (name === 'office.source-show') {
      const source = sources.get(input.id); if (!source) throw Error('unexpected source');
      if (input.media) { const bytes = input.media === 'frame' || source.kind === 'image' ? frameBytes : source.media_type?.startsWith('audio/') ? audioBytes : videoBytes; return { ...source, media: { media_type: input.media === 'frame' || source.kind === 'image' ? 'image/png' : source.media_type, content_base64: bytes.toString('base64'), too_large: false } }; }
      return source;
    }
    if (name === 'office.source-set-sensitivity') { const source = sources.get(input.id); source.sensitivity = 'private'; return { id: input.id, sensitivity: 'private' }; }
    if (name === 'office.status') return { open_weights: { label: 'Company server' } };
    if (name === 'office.video-plan') return { plan: { digest: input.sensitivity ? 'd'.repeat(64) : digest, id: input.id, sensitivity: input.sensitivity ?? 'normal', duration_ms: 180000, counts: { audio: 1, frames: 3, extract: 1 }, estimate: null } };
    if (name === 'office.batch-plan') return { plan: { digest: input.sensitivity ? 'e'.repeat(64) : digest, phase: input.phase, private: input.sensitivity === 'private', sources: captures.map((_, i) => `src-20261027-upload-${i + 1}`), videos: [], counts: { digest: 2, audio: 0, frames: 0, extract: 0, careful: 1 }, estimate: { amount: 0.25, currency: 'USD' } } };
    if (name === 'office.knowledge-show') return { page: { title: 'Restart the ingest worker', body: 'Check the queue before restarting.' } };
    if (name === 'orbit.task.show') return { title: 'Check queue capacity', description: 'Confirm the queue is empty.' };
    throw Error(`unexpected fixture call ${name}`);
  } };
  const feature = createSourcesFeature({ client, workspaceRoot: root }); const fallback = createHandler();
  const server = createServer((req, res) => {
    const route = feature.routes.find(r => r.method === req.method && r.path === req.url.split('?')[0]);
    if (route) void route.handle(req, res);
    else if (req.url.startsWith('/api/') && !req.url.endsWith('.js')) { res.writeHead(200, { 'content-type': 'application/json' }); res.end(JSON.stringify(req.url === '/api/today' ? { needs_you: [], agents_working_on: [], done_today: [], truncated: false, completion_dates_unavailable: 0 } : req.url === '/api/setup' ? { state: 'ready' } : { route: null, token: 'unset' })); }
    else fallback(req, res);
  });
  await new Promise(done => server.listen(0, '127.0.0.1', done)); t.after(() => new Promise(done => server.close(done)));
  return { url: `http://127.0.0.1:${server.address().port}`, calls, captures, sources, root };
}
