import assert from "node:assert/strict";
import { ReadStream } from "node:fs";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer, request as httpRequest } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { checkLoopbackRequest } from "../dist/http/guard.js";
import { createHandler } from "../dist/http/router.js";

function request(port, { method = "GET", path = "/", headers = {} } = {}) {
  return new Promise((resolve, reject) => {
    const req = httpRequest(
      { host: "127.0.0.1", port, method, path, headers },
      (res) => {
        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () =>
          resolve({
            status: res.statusCode,
            headers: res.headers,
            body: Buffer.concat(chunks).toString("utf8"),
          }),
        );
      },
    );
    req.on("error", reject);
    req.end();
  });
}

async function startTestServer(t) {
  const root = await mkdtemp(join(tmpdir(), "office-http-"));
  const staticDir = join(root, "static");
  const distDir = join(root, "dist");
  await mkdir(staticDir, { recursive: true });
  await mkdir(distDir, { recursive: true });
  await writeFile(join(staticDir, "index.html"), "<!doctype html><title>Office</title><h1>Office</h1>\n");
  await writeFile(join(staticDir, "style.css"), "body { margin: 0; }\n");
  await writeFile(join(distDir, "main.js"), "console.log('office');\n");
  await writeFile(join(distDir, "sw.js"), "self.addEventListener('fetch', () => {});\n");
  await writeFile(join(root, "secret.txt"), "not served\n");
  const server = createServer(createHandler({ staticDir, distDir }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    await new Promise((resolve) => server.close(resolve));
    await rm(root, { recursive: true, force: true });
  });
  const { port } = server.address();
  return { port, host: `127.0.0.1:${port}` };
}

test("the UI is served at / and compiled assets from dist", async (t) => {
  const { port, host } = await startTestServer(t);
  const home = await request(port, { headers: { host } });
  assert.equal(home.status, 200);
  assert.match(home.headers["content-type"], /^text\/html/);
  assert.match(home.body, /<h1>Office<\/h1>/);

  const script = await request(port, { path: "/main.js", headers: { host } });
  assert.equal(script.status, 200);
  assert.match(script.headers["content-type"], /^text\/javascript/);
  assert.match(script.body, /console\.log/);

  const css = await request(port, { path: "/style.css", headers: { host } });
  assert.equal(css.status, 200);
  assert.match(css.headers["content-type"], /^text\/css/);
});

test("a worker script response allows the app scope; other scripts do not", async (t) => {
  const { port, host } = await startTestServer(t);
  const worker = await request(port, { path: "/sw.js", headers: { host } });
  assert.equal(worker.status, 200);
  assert.equal(worker.headers["service-worker-allowed"], "/");

  const script = await request(port, { path: "/main.js", headers: { host } });
  assert.equal(script.status, 200);
  assert.equal(script.headers["service-worker-allowed"], undefined);
});

test("healthz answers on a loopback host and 404s unknown paths", async (t) => {
  const { port, host } = await startTestServer(t);
  const health = await request(port, { path: "/healthz", headers: { host } });
  assert.equal(health.status, 200);
  assert.equal(JSON.parse(health.body).ok, true);

  const missing = await request(port, { path: "/no-such-file.js", headers: { host } });
  assert.equal(missing.status, 404);
});

test("a non-loopback Host is refused on every route", async (t) => {
  const { port } = await startTestServer(t);
  for (const path of ["/", "/healthz", "/main.js"]) {
    const res = await request(port, { path, headers: { host: "evil.example.com" } });
    assert.equal(res.status, 403, `${path} with an evil Host must be refused`);
    assert.equal(JSON.parse(res.body).error, "forbidden");
  }
  // DNS-rebinding style: a public name that resolves to loopback is still refused.
  const rebinding = await request(port, {
    path: "/",
    headers: { host: "127.0.0.1.nip.io" },
  });
  assert.equal(rebinding.status, 403);
});

test("a cross-site Origin is refused; a matching loopback Origin is allowed", async (t) => {
  const { port, host } = await startTestServer(t);
  const crossSite = await request(port, {
    headers: { host, origin: "https://evil.example.com" },
  });
  assert.equal(crossSite.status, 403);

  const wrongPort = await request(port, {
    headers: { host, origin: "http://127.0.0.1:9" },
  });
  assert.equal(wrongPort.status, 403);

  const sameSite = await request(port, {
    headers: { host, origin: `http://${host}` },
  });
  assert.equal(sameSite.status, 200);

  // State-changing requests without an Origin are refused (STD-05 R17).
  const post = await request(port, { method: "POST", path: "/healthz", headers: { host } });
  assert.equal(post.status, 403);
});

test("explicit HTTP default ports are accepted as the same loopback authority", async (t) => {
  const { port } = await startTestServer(t);
  for (const [host, origin] of [
    ["localhost:80", "http://localhost"],
    ["localhost:80", "http://localhost:80"],
    ["127.0.0.1:80", "http://127.0.0.1"],
    ["[::1]:80", "http://[::1]"],
  ]) {
    const res = await request(port, {
      method: "POST",
      path: "/healthz",
      headers: { host, origin },
    });
    assert.equal(res.status, 404, `${host} and ${origin} should pass the guard`);
  }
});

test("path traversal never escapes the web roots", async (t) => {
  const { port, host } = await startTestServer(t);
  for (const path of ["/../secret.txt", "/%2e%2e/secret.txt", "/static/../../secret.txt"]) {
    const res = await request(port, { path, headers: { host } });
    assert.notEqual(res.status, 200, `${path} must not be served`);
    assert.doesNotMatch(res.body, /not served/);
  }
});

test("aborting a static response destroys its source stream", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "office-http-abort-"));
  const staticDir = join(root, "static");
  const distDir = join(root, "dist");
  await mkdir(staticDir, { recursive: true });
  await mkdir(distDir, { recursive: true });
  const SIZE = 256 * 1024;
  await writeFile(join(staticDir, "big.bin"), Buffer.alloc(SIZE, 0x61));
  await writeFile(join(staticDir, "index.html"), "<!doctype html><title>Office</title>\n");

  const captured = [];
  const origConstruct = ReadStream.prototype._construct;
  ReadStream.prototype._construct = function (...args) {
    captured.push(this);
    return origConstruct.apply(this, args);
  };

  const server = createServer(createHandler({ staticDir, distDir }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    ReadStream.prototype._construct = origConstruct;
    await new Promise((resolve) => server.close(resolve));
    await rm(root, { recursive: true, force: true });
  });
  const { port } = server.address();
  const host = `127.0.0.1:${port}`;

  // Abort deterministically on the first received chunk: the 256 KiB file
  // spans several 64 KiB chunks, so the server is mid-stream when the
  // client disconnects.
  await new Promise((resolve) => {
    let done = false;
    const finish = () => {
      if (!done) {
        done = true;
        resolve();
      }
    };
    const req = httpRequest(
      { host: "127.0.0.1", port, method: "GET", path: "/big.bin", headers: { host } },
      (res) => {
        res.on("data", () => {
          req.destroy();
          res.destroy();
        });
        res.on("end", finish);
        res.on("close", finish);
        res.on("error", finish);
      },
    );
    req.on("error", finish);
    req.end();
    setTimeout(finish, 3000);
  });

  const stream = captured.find((s) => String(s.path ?? "").endsWith("big.bin"));
  assert.ok(stream, "expected a file stream for big.bin");
  const deadline = Date.now() + 2000;
  while (!stream.destroyed && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 25));
  }
  assert.equal(stream.destroyed, true, "aborted static response must destroy its source stream");
  assert.ok(stream.fd == null, "aborted source descriptor must be closed");
  assert.equal(stream.listenerCount("error"), 0, "stream error listener must be removed");

  // The server still serves full bodies after the abort.
  const full = await request(port, { path: "/big.bin", headers: { host } });
  assert.equal(full.status, 200);
  assert.equal(full.body.length, SIZE);

  const head = await request(port, { method: "HEAD", path: "/big.bin", headers: { host } });
  assert.equal(head.status, 200);
  assert.equal(head.body, "");
  assert.equal(Number(head.headers["content-length"]), SIZE);
});

test("guard rejects missing and unparsable Host headers directly", () => {
  assert.equal(checkLoopbackRequest({ method: "GET", headers: {} }).ok, false);
  assert.equal(checkLoopbackRequest({ method: "GET", headers: { host: "" } }).ok, false);
  assert.equal(checkLoopbackRequest({ method: "GET", headers: { host: "exa mple" } }).ok, false);
  assert.equal(checkLoopbackRequest({ method: "GET", headers: { host: "192.168.1.10:4317" } }).ok, false);
  assert.equal(checkLoopbackRequest({ method: "GET", headers: { host: "localhost:4317" } }).ok, true);
  assert.equal(checkLoopbackRequest({ method: "GET", headers: { host: "[::1]:4317" } }).ok, true);
  assert.equal(
    checkLoopbackRequest({
      method: "GET",
      headers: { host: "[::1]:4317", origin: "http://[::1]:4317" },
    }).ok,
    true,
  );
});

test("guard treats explicit and implicit HTTP default ports as equivalent", () => {
  for (const [host, origin] of [
    ["localhost:80", "http://localhost"],
    ["localhost:80", "http://localhost:80"],
    ["localhost", "http://localhost:80"],
    ["127.0.0.1:80", "http://127.0.0.1"],
    ["127.0.0.1", "http://127.0.0.1:80"],
    ["[::1]:80", "http://[::1]"],
    ["[::1]", "http://[::1]:80"],
  ]) {
    assert.deepEqual(
      checkLoopbackRequest({ method: "POST", headers: { host, origin } }),
      { ok: true, host },
      `${host} and ${origin} should identify the same loopback HTTP authority`,
    );
  }

  assert.equal(
    checkLoopbackRequest({ method: "POST", headers: { host: "localhost:80", origin: "http://localhost:81" } }).ok,
    false,
  );
  assert.equal(
    checkLoopbackRequest({ method: "POST", headers: { host: "localhost:81", origin: "http://localhost" } }).ok,
    false,
  );
});
