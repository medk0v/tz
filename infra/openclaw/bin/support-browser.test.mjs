import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { mkdtempSync, writeFileSync, rmSync, unlinkSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { browserUrl, validateBrowserGrant, browserResource, requestTestBrowserPage } from "./support-browser.mjs";
import { browserChannel } from "./support-browser-protocol.mjs";

const permissions = { allowGet: true, allowPost: false, allowedHosts: ["explorer.example"] };
const resource = { method: "GET", url: "https://explorer.example/transaction/abc", headers: {} };
const signal = new AbortController().signal;

test("browser grants require GET, discard credentials and reject expired or scenario grants", () => {
  const directory = mkdtempSync(join(tmpdir(), "support-browser-grant-"));
  const id = randomUUID();
  const grant = {
    version: 4, expires_at_unix_ms: Date.now() + 60_000, reminder_action_id: randomUUID(),
    public_http_get: true, public_http_post: true, http_allowed_hosts: ["explorer.example"],
    secrets: { API_TOKEN: "never-send-to-chromium" }, variables: { ORDER_ID: "private" },
  };
  const save = (overrides = {}) => writeFileSync(join(directory, `${id}.json`), JSON.stringify({ ...grant, ...overrides }), { mode: 0o600 });
  try {
    save();
    assert.deepEqual(validateBrowserGrant(id, directory), { ...permissions, browserProxy: null });
    save({ public_http_get: false });
    assert.throws(() => validateBrowserGrant(id, directory), /method_not_allowed/);
    save({ expires_at_unix_ms: Date.now() - 1 });
    assert.throws(() => validateBrowserGrant(id, directory), /not_allowed/);
    save({ test_execution: true });
    assert.throws(() => validateBrowserGrant(id, directory), /browser_unavailable_in_tests/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

test("browser navigation supports hash routes but retains exact public HTTPS host policy", () => {
  assert.equal(browserUrl("https://explorer.example/#/transaction/abc", permissions), "https://explorer.example/#/transaction/abc");
  for (const url of ["http://explorer.example", "file:///etc/passwd", "https://127.0.0.1", "https://[::1]", "https://user:pass@explorer.example", "https://explorer.example:8443", "https://api.explorer.example", "https://elsewhere.example"]) {
    assert.throws(() => browserUrl(url, permissions), undefined, url);
  }
  assert.equal(browserUrl("https://elsewhere.example", { ...permissions, allowedHosts: ["*"] }), "https://elsewhere.example/");
});

test("all page resources remain GET-only even if an HTTP POST permission is present", async () => {
  let fetched = false;
  const fetch = async () => { fetched = true; return new Response("unexpected"); };
  for (const message of [
    { ...resource, method: "POST" }, { ...resource, method: "DELETE" },
    { ...resource, url: "https://forbidden.example/script.js" },
  ]) {
    await assert.rejects(browserResource(message, { ...permissions, allowPost: true }, { requests: 0, bytes: 0 }, signal, fetch));
  }
  assert.equal(fetched, false);
});

test("broker strips credential headers and returns redirects for a separately checked navigation", async () => {
  let calls = 0;
  const response = await browserResource({ ...resource, headers: {
    Authorization: "private-token", "X-API-Key": "private-key", Cookie: "anonymous-session", "User-Agent": "browser", Host: "internal.example",
    "Sec-Fetch-Mode": "navigate", "Sec-Fetch-Site": "none", "Sec-Fetch-Dest": "document",
    "Sec-CH-UA": '"Chromium";v="140"', "Sec-CH-UA-Platform": '"Linux"',
    "Accept-Language": "en-US,en;q=0.9", "Upgrade-Insecure-Requests": "1",
    "Proxy-Authorization": "proxy-secret", "X-Forwarded-For": "127.0.0.1",
  } }, permissions, { requests: 0, bytes: 0 }, signal, async (url, options) => {
    calls++;
    assert.equal(url, resource.url);
    assert.deepEqual(options.headers, {
      "accept-encoding": "identity", cookie: "anonymous-session", "user-agent": "browser",
      "sec-fetch-mode": "navigate", "sec-fetch-site": "none", "sec-fetch-dest": "document",
      "sec-ch-ua": '"Chromium";v="140"', "sec-ch-ua-platform": '"Linux"',
      "accept-language": "en-US,en;q=0.9", "upgrade-insecure-requests": "1",
    });
    return new Response(null, { status: 302, headers: { location: "https://forbidden.example/" } });
  });
  assert.equal(calls, 1);
  assert.equal(response.status, 302);
  assert.throws(() => browserUrl(response.headers.location, permissions), /url_not_allowed/);
});

test("broker preserves separate session cookies including comma-bearing expiry dates", async () => {
  const cookies = ["first=one; Path=/; Secure; HttpOnly", "second=two; Expires=Wed, 21 Oct 2037 07:28:00 GMT; Path=/; Secure"];
  const response = await browserResource(resource, permissions, { requests: 0, bytes: 0 }, signal,
    async () => new Response("page", { headers: cookies.map((cookie) => ["set-cookie", cookie]) }));
  assert.deepEqual(response.setCookies, cookies);
  assert.equal(response.headers["set-cookie"], undefined);
});

test("page and resource budgets stop oversized or excessive downloads", async () => {
  const fetch = async () => new Response(Buffer.alloc(8 * 1024 * 1024 + 1));
  await assert.rejects(browserResource(resource, permissions, { requests: 0, bytes: 0 }, signal, fetch), /page_limit_exceeded/);
  await assert.rejects(browserResource(resource, permissions, { requests: 200, bytes: 0 }, signal, fetch), /page_limit_exceeded/);
});

test("authorized test pages recheck GET grants without exposing secrets to the browser", async (t) => {
  const directory = mkdtempSync(join(tmpdir(), "support-test-page-"));
  const id = randomUUID(), grantPath = join(directory, `${id}.json`), socketPath = join(directory, "b.sock");
  const grant = {
    version: 4, test_execution: true, expires_at_unix_ms: Date.now() + 60_000,
    public_http_get: false, public_http_post: false, test_capabilities: { http_get: true, http_post: true },
    http_allowed_hosts: ["explorer.example"], secrets: { TOKEN: "private-token" }, variables: { PRIVATE: "private-value" },
  };
  const save = (overrides = {}) => writeFileSync(grantPath, JSON.stringify({ ...grant, ...overrides }), { mode: 0o640 });
  let connections = 0, fetched = 0, beforeFetch = () => {};
  const server = createServer((socket) => {
    connections++;
    const send = browserChannel(socket, async (message) => {
      if (message.type === "open") {
        assert.deepEqual(message, { type: "open", url: "https://explorer.example/#/details" });
        beforeFetch();
        send({ type: "fetch", id: 1, method: "GET", url: "https://explorer.example/data", headers: { Authorization: "private-token", "X-API-Key": "private-token" } });
      } else {
        assert.equal(message.type, "resource");
        send({ type: "result", result: { ok: true, url: "https://explorer.example/#/details", text: "Rendered page" } });
      }
    });
  });
  t.after(async () => {
    await new Promise((resolve) => server.close(resolve));
    rmSync(directory, { recursive: true, force: true });
  });
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(socketPath, resolve); });
  const options = {
    grantDirectory: directory, socketPath,
    fetchImplementation: async (url, request) => {
      fetched++;
      assert.equal(url, "https://explorer.example/data");
      assert.equal(request.method, "GET");
      assert.deepEqual(request.headers, { "accept-encoding": "identity" });
      return new Response("Rendered data");
    },
  };
  save();
  assert.deepEqual(await requestTestBrowserPage(id, "https://explorer.example/#/details", options), {
    ok: true, url: "https://explorer.example/#/details", text: "Rendered page",
  });
  assert.equal(fetched, 1);
  for (const [overrides, error] of [
    [{ test_capabilities: { http_get: false, http_post: true } }, "method_not_allowed"],
    [{ expires_at_unix_ms: Date.now() - 1 }, "not_allowed"],
    [{ test_execution: false }, "not_allowed"],
    [{ http_allowed_hosts: ["other.example"] }, "url_not_allowed"],
  ]) {
    save(overrides);
    assert.deepEqual(await requestTestBrowserPage(id, "https://explorer.example/#/details", options), { ok: false, error });
  }
  unlinkSync(grantPath);
  assert.deepEqual(await requestTestBrowserPage(id, "https://explorer.example/#/details", options), { ok: false, error: "not_allowed" });
  assert.equal(connections, 1);
  for (const revoke of [
    () => save({ expires_at_unix_ms: Date.now() - 1 }),
    () => unlinkSync(grantPath),
    () => save({ test_capabilities: { http_get: false } }),
  ]) {
    save();
    beforeFetch = revoke;
    assert.deepEqual(await requestTestBrowserPage(id, "https://explorer.example/#/details", options), { ok: false, error: "not_allowed" });
    assert.equal(fetched, 1);
  }
});
