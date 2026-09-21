// Run against the real offline Chromium runner; no model or customer data needed.
// node infra/openclaw/bin/support-browser.smoke.mjs /path/to/runner.sock [public-https-url]
import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";
import { requestBrowserPage } from "./support-browser.mjs";

const socketPath = process.argv[2];
if (!socketPath) throw new Error("Pass the browser runner's Unix socket path");
const permissions = () => ({ allowGet: true, allowPost: false, allowedHosts: ["explorer.example"] });
const requests = [];
const fixtureErrors = [];
let parallelStarted = 0;
let startParallel;
const parallelReady = new Promise((resolve) => { startParallel = resolve; });
const fetchImplementation = async (url, options) => {
  requests.push(url);
  const path = new URL(url).pathname;
  const parallel = path.match(/^\/parallel\/(one|two)\/(start|page|data)$/);
  if (parallel) {
    const [, identity, action] = parallel;
    if (action === "start") {
      assert.equal(options.headers.cookie, undefined);
      if (++parallelStarted === 2) startParallel();
      // This barrier fails when browser operations are accidentally serialized.
      await Promise.race([parallelReady, delay(10_000).then(() => { throw new Error("parallel browser did not start"); })]);
      return new Response(null, { status: 302, headers: {
        location: `/parallel/${identity}/page`, "set-cookie": `session=${identity}; Path=/; Secure; HttpOnly`,
      } });
    }
    assert.equal(options.headers.cookie, `session=${identity}`);
    if (action === "data") return Response.json({ identity });
    return new Response(`<body>Loading<script>fetch('/parallel/${identity}/data').then(r=>r.json()).then(data=>{document.body.textContent=data.identity;})</script>`,
      { headers: { "content-type": "text/html" } });
  }
  if (path === "/cancel") {
    await delay(5000, undefined, { signal: options.signal });
    return new Response("Unexpected completion");
  }
  if (path === "/identity/start") {
    assert.equal(options.headers.cookie, undefined, "Each read starts without earlier cookies");
    assert.match(options.headers["user-agent"], /Chrome\/\d+/);
    assert.doesNotMatch(options.headers["user-agent"], /HeadlessChrome/);
    assert.match(options.headers["accept-language"], /^en-US/);
    return new Response(null, { status: 302, headers: [
      ["location", "/identity/page"],
      ["set-cookie", "session=one; Path=/; Secure; HttpOnly"],
      ["set-cookie", "preference=two; Expires=Wed, 21 Oct 2037 07:28:00 GMT; Path=/; Secure"],
    ] });
  }
  if (path === "/identity/data") {
    assert.match(options.headers.cookie, /session=one/);
    assert.match(options.headers.cookie, /preference=two/);
    return Response.json({ userAgent: options.headers["user-agent"] });
  }
  if (path === "/identity/page") return new Response(`<!doctype html><body>Loading<script>
    fetch('/identity/data').then(r => r.json()).then(data => {
      document.body.textContent = JSON.stringify({ sameUserAgent: data.userAgent === navigator.userAgent,
        language: navigator.language, cookie: document.cookie });
    });
  </script>`, { headers: { "content-type": "text/html" } });
  if (path === "/redirect") return new Response(null, { status: 302, headers: { location: "/app" } });
  if (path === "/private") return new Response(null, { status: 302, headers: { location: "https://127.0.0.1/private" } });
  if (path === "/stream") return new Response('<body>Streaming page<script>new WebSocket("wss://explorer.example/streaming")</script>', { headers: { "content-type": "text/html" } });
  if (path === "/data") return new Response(JSON.stringify({ status: "Confirmed from JavaScript" }), { headers: { "content-type": "application/json" } });
  return new Response(`<!doctype html><title>Browser smoke</title><body><main>Loading</main>
    <a href="/next">Next page</a><script>
      const previousCookie = document.cookie;
      document.cookie = 'smoke=private';
      Promise.all([
        fetch('/data').then(r => r.json()),
        fetch('/write', {method:'POST', body:'forbidden'}).then(()=>'unexpected').catch(()=>'blocked'),
        fetch('https://forbidden.example/data').then(()=>'unexpected').catch(()=>'blocked')
      ]).then(([data, post, other]) => {
        document.querySelector('main').textContent = data.status + ' POST=' + post + ' other=' + other + ' priorCookie=' + previousCookie + ' hash=' + location.hash + ' online=' + navigator.onLine;
      });
    </script></body>`, { headers: { "content-type": "text/html" } });
};

async function read(url, options = {}) {
  for (let attempt = 0; attempt < 20; attempt++) {
    const result = await requestBrowserPage(url, options.permissions ?? permissions, {
      socketPath, fetchImplementation: async (...args) => {
        try { return await fetchImplementation(...args); }
        catch (error) { if (!args[1]?.signal?.aborted) fixtureErrors.push(error.stack); throw error; }
      }, ...options,
    });
    assert.deepEqual(fixtureErrors, [], fixtureErrors.join("\n"));
    if (result.error !== "browser_busy") return result;
    await delay(200);
  }
  throw new Error("browser stayed busy after cleanup");
}

for (let index = 0; index < 2; index++) {
  const result = await read("https://explorer.example/redirect#/transaction/abc");
  assert.equal(result.ok, true, JSON.stringify({ result, requests }));
  assert.equal(result.url, "https://explorer.example/app#/transaction/abc");
  assert.match(result.text, /Confirmed from JavaScript POST=blocked other=blocked priorCookie= hash=#\/transaction\/abc online=true/);
  assert.equal(result.partial, true);
  assert.equal(result.links[0].url, "https://explorer.example/next");
  assert.ok(result.blocked_hosts.includes("forbidden.example"));
}
assert.equal(requests.some((url) => url.includes("/write") || url.includes("forbidden.example")), false);
const privateRedirect = await read("https://explorer.example/private");
assert.equal(privateRedirect.ok, false);
assert.equal(requests.some((url) => url.includes("127.0.0.1")), false);
const stream = await read("https://explorer.example/stream");
assert.equal(stream.ok, true);
assert.equal(stream.partial, true, "An unavailable streaming connection must not look like a complete page");
console.log("PASS: Chromium renders JavaScript and hash routes, checks redirects, blocks POST/private/unlisted hosts, and forgets cookies between reads.");

for (let index = 0; index < 2; index++) {
  const identity = await read("https://explorer.example/identity/start");
  assert.equal(identity.ok, true, JSON.stringify(identity));
  assert.equal(identity.partial, false, JSON.stringify(identity));
  assert.deepEqual(JSON.parse(identity.text), { sameUserAgent: true, language: "en-US", cookie: "preference=two" });
}
console.log("PASS: Chrome User-Agent matches JavaScript, language is consistent, and separate cookies work through redirects without crossing reads.");

const parallelResults = await Promise.all(["one", "two"].map(identity => read(`https://explorer.example/parallel/${identity}/start`)));
assert.equal(parallelStarted, 2);
for (const [index, identity] of ["one", "two"].entries()) {
  assert.equal(parallelResults[index].ok, true, JSON.stringify(parallelResults[index]));
  assert.equal(parallelResults[index].text, identity);
}
const [cancelled, survivor] = await Promise.allSettled([
  read("https://explorer.example/cancel", { timeoutMs: 1000 }),
  read("https://explorer.example/identity/start"),
]);
assert.equal(cancelled.status, "rejected");
assert.match(cancelled.reason.message, /browser_timeout/);
assert.equal(survivor.status, "fulfilled", String(survivor.reason));
assert.equal(survivor.value.ok, true, JSON.stringify(survivor));
console.log("PASS: two real Chromium reads overlap, keep cookies/results isolated, and cancellation leaves the other read running.");

if (process.argv[3]) {
  const result = await read(process.argv[3], {
    permissions: () => ({ allowGet: true, allowPost: false, allowedHosts: ["*"] }), fetchImplementation: undefined,
  });
  console.log(JSON.stringify(result, null, 2));
  assert.equal(result.ok, true, "Public page failed to load");
}
