// Real application routes and UI, with only the isolated synthetic CLI on PATH.
import { createServer } from 'node:http';
import { handle } from '../../app/packages/server/dist/http/router.js';

const server = createServer((request, response) => {
  if (request.method !== 'GET' && request.method !== 'HEAD') {
    response.writeHead(405); response.end('Synthetic preview is read only.'); return;
  }
  handle(request, response);
});
server.listen(0, '127.0.0.1', () => console.log(`http://127.0.0.1:${server.address().port}`));
process.on('SIGTERM', () => { server.close(() => { process.exitCode = 0; }); server.closeIdleConnections(); });
