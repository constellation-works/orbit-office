import assert from "node:assert/strict";
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

test("path traversal never escapes the web roots", async (t) => {
  const { port, host } = await startTestServer(t);
  for (const path of ["/../secret.txt", "/%2e%2e/secret.txt", "/static/../../secret.txt"]) {
    const res = await request(port, { path, headers: { host } });
    assert.notEqual(res.status, 200, `${path} must not be served`);
    assert.doesNotMatch(res.body, /not served/);
  }
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
