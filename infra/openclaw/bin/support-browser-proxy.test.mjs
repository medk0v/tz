import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { PassThrough, Readable } from "node:stream";
import { test } from "node:test";
import { createServer as httpServer, request as httpRequest } from "node:http";
import { createServer as httpsServer } from "node:https";
import { connect as netConnect } from "node:net";
import { connect as tlsConnect } from "node:tls";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connectTunnel, createProxyFetch, parseProxy, validateProxySettings } from "./support-browser-proxy.mjs";

const settings = { service_url: "https://discovery.example/api/proxies/random", token: "discovery-secret" };
const peer = { protocol: "http", host: "proxy.example", port: 10000, username: "user", password: "pass" };
const dns = async host => [{ address: host === "proxy.example" ? "8.8.8.8" : "1.1.1.1" }];

test("validates service destination, mutually exclusive filters and proxy peers", () => {
  for (const value of [{ ...settings, region: "europe", country: "DE" }, { ...settings, country: "de" }, { ...settings, token: "a\r\nb" }, { ...settings, service_url: "https://user:pass@discovery.example/random" }]) assert.throws(() => validateProxySettings(value));
  for (const value of [{ ...peer, protocol: "socks5" }, { ...peer, host: "127.0.0.1" }, { ...peer, host: "169.254.169.254" }, { ...peer, port: 0 }, { ...peer, username: "a:b" }]) assert.throws(() => parseProxy(value));
  assert.deepEqual(parseProxy({ ...peer, url: "http://127.0.0.1:9" }), peer);
});

function fakeTarget(observed) {
  return (url, options, callback) => {
    observed.push({ url: url.href, options });
    const request = new EventEmitter();
    request.end = () => {
      const incoming = Readable.from([Buffer.from("page")]);
      incoming.statusCode = 200;
      incoming.rawHeaders = ["content-type", "text/plain", "set-cookie", "one=1", "set-cookie", "two=2"];
      callback(incoming);
      incoming.once("end", () => request.emit("close"));
    };
    return request;
  };
}
test("discovers once per page, defaults to unknown country, and keeps Bearer away from target", async () => {
  for (const filters of [{}, { region: "europe" }, { country: "DE" }]) {
    const requests = [], target = [];
    const fetch = createProxyFetch({ ...settings, ...filters }, {
      lookup: dns,
      discoveryFetch: async (url, options) => { requests.push({ url, options }); return Response.json(peer); },
      targetRequest: fakeTarget(target),
    });
    const responses = await Promise.all(["/page", "/style.css"].map(path => fetch(`https://site.example${path}`, { method: "GET", headers: { cookie: "page=1" }, signal: AbortSignal.timeout(2000) })));
    assert.equal(requests.length, 1);
    assert.deepEqual(Object.fromEntries(new URL(requests[0].url).searchParams), Object.keys(filters).length ? filters : { country: "unknown" });
    assert.equal(requests[0].options.headers.authorization, "Bearer discovery-secret");
    assert.equal(requests[0].options.headers.cookie, undefined);
    for (const request of target) {
      assert.equal(request.options.headers.authorization, undefined);
      assert.equal(request.options.headers["proxy-authorization"], undefined);
      assert.equal(request.options.headers.cookie, "page=1");
      assert.ok(request.options.agent);
    }
    for (const response of responses) { assert.equal(await response.text(), "page"); assert.deepEqual(response.headers.getSetCookie(), ["one=1", "two=2"]); }
  }
});

test("rejects failed discovery, oversized replies, and private DNS without a direct fallback", async () => {
  for (const response of [new Response(null, { status: 302, headers: { location: "https://other.example" } }), new Response("private-secret", { status: 401 }), new Response("x".repeat(17000)), Response.json({ ...peer, host: "internal.example" })]) {
    let connected = false;
    const fetch = createProxyFetch(settings, {
      lookup: async host => host === "internal.example" ? [{ address: "10.0.0.1" }] : dns(host),
      discoveryFetch: async () => response,
      targetRequest: () => { connected = true; },
    });
    await assert.rejects(fetch("https://site.example/", { method: "GET", signal: AbortSignal.timeout(2000) }), /^Error: proxy_unavailable$/);
    assert.equal(connected, false);
  }
  let discovered = false;
  const fetch = createProxyFetch(settings, { lookup: async () => [{ address: "127.0.0.1" }], discoveryFetch: async () => { discovered = true; } });
  await assert.rejects(fetch("https://internal.example/", { method: "GET", signal: AbortSignal.timeout(2000) }));
  assert.equal(discovered, false);
});

test("CONNECT pins both peers, authenticates only the proxy, and verifies target TLS", async () => {
  for (const protocol of ["http", "https"]) {
    let options, tlsOptions;
    const socket = new PassThrough();
    const secure = new PassThrough();
    const request = value => {
      options = value;
      const req = new EventEmitter();
      req.end = () => queueMicrotask(() => req.emit("connect", { statusCode: 200 }, socket, Buffer.alloc(0)));
      return req;
    };
    const tunnel = await connectTunnel({ ...peer, protocol, address: "8.8.8.8", family: 4 }, { hostname: "site.example", address: "1.1.1.1", family: 4 }, new AbortController().signal, {
      httpRequest: protocol === "http" ? request : () => assert.fail(), httpsRequest: protocol === "https" ? request : () => assert.fail(),
      tlsConnect: value => { tlsOptions = value; queueMicrotask(() => secure.emit("secureConnect")); return secure; },
    });
    assert.equal(options.path, "1.1.1.1:443");
    assert.equal(options.headers["proxy-authorization"], `Basic ${Buffer.from("user:pass").toString("base64")}`);
    assert.equal(options.headers.authorization, undefined);
    options.lookup(peer.host, {}, (err, address) => { assert.equal(err, null); assert.equal(address, "8.8.8.8"); });
    assert.equal(tlsOptions.servername, "site.example");
    assert.equal(tlsOptions.rejectUnauthorized, true);
    assert.equal(tlsOptions.socket, socket);
    assert.equal(tunnel, secure);
    tunnel.destroy(); socket.destroy();
  }
});

test("rejects denied CONNECT and destroys the tunnel on cancellation", async () => {
  const socket = new PassThrough();
  await assert.rejects(connectTunnel({ ...peer, address: "8.8.8.8", family: 4 }, { hostname: "site.example", address: "1.1.1.1", family: 4 }, new AbortController().signal, {
    httpRequest: () => { const req = new EventEmitter(); req.end = () => req.emit("connect", { statusCode: 407 }, socket, Buffer.alloc(0)); return req; },
    tlsConnect: () => assert.fail("TLS must not start after denied CONNECT"),
  }), /proxy_connect_failed/);
  assert.equal(socket.destroyed, true);
  const control = new AbortController(); const secure = new PassThrough();
  const pending = connectTunnel({ ...peer, address: "8.8.8.8", family: 4 }, { hostname: "site.example", address: "1.1.1.1", family: 4 }, control.signal, {
    httpRequest: () => { const req = new EventEmitter(); req.end = () => req.emit("connect", { statusCode: 200 }, new PassThrough(), Buffer.alloc(0)); return req; },
    tlsConnect: () => secure,
  });
  control.abort();
  await assert.rejects(pending, /proxy_tls_failed/);
  assert.equal(secure.destroyed, true);
});

test("real CONNECT carries HTTPS responses and rejects an untrusted target certificate", { timeout: 15000 }, async t => {
  const directory = mkdtempSync(join(tmpdir(), "support-proxy-tls-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  writeFileSync(join(directory, "cert.cnf"), "[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN=site.example\n[ext]\nsubjectAltName=DNS:site.example\n");
  execFileSync("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-config", join(directory, "cert.cnf"), "-keyout", join(directory, "key.pem"), "-out", join(directory, "cert.pem")], { stdio: "ignore" });
  const cert = readFileSync(join(directory, "cert.pem"));
  const sockets = new Set();
  const track = socket => { sockets.add(socket); socket.once("close", () => sockets.delete(socket)); };
  const seen = [];
  const target = httpsServer({ key: readFileSync(join(directory, "key.pem")), cert }, (req, res) => {
    seen.push(req.headers);
    res.writeHead(200, { "content-type": "text/plain", "set-cookie": ["one=1; Secure", "two=2; Secure"] });
    res.end("verified target page");
  });
  const proxy = httpServer();
  for (const server of [target, proxy]) server.on("connection", track);
  t.after(async () => {
    for (const socket of sockets) socket.destroy();
    await Promise.all([target, proxy].map(server => new Promise(resolve => server.close(resolve))));
  });
  await new Promise(resolve => target.listen(0, "127.0.0.1", resolve));
  await new Promise(resolve => proxy.listen(0, "127.0.0.1", resolve));
  proxy.on("connect", (req, downstream, head) => {
    assert.equal(req.url, "1.1.1.1:443");
    assert.equal(req.headers["proxy-authorization"], `Basic ${Buffer.from("user:pass").toString("base64")}`);
    const upstream = netConnect(target.address().port, "127.0.0.1", () => {
      downstream.write("HTTP/1.1 200 Connection Established\r\n\r\n");
      if (head.length) upstream.write(head);
      upstream.pipe(downstream); downstream.pipe(upstream);
    });
    track(upstream);
    upstream.on("error", () => downstream.destroy());
    downstream.on("error", () => upstream.destroy());
    downstream.on("close", () => upstream.destroy());
  });
  const dependencies = {
    lookup: dns, discoveryFetch: async () => Response.json(peer),
    // Map the validated public fixture address onto the local test proxy only.
    httpRequest: options => { assert.equal(options.hostname, peer.host); return httpRequest({ ...options, hostname: "127.0.0.1", port: proxy.address().port, lookup: undefined }); },
  };
  const fetch = createProxyFetch(settings, { ...dependencies, tlsConnect: options => tlsConnect({ ...options, ca: cert }) });
  const result = await fetch("https://site.example/page", { method: "GET", headers: { cookie: "session=local" }, signal: AbortSignal.timeout(5000) });
  assert.equal(await result.text(), "verified target page");
  assert.deepEqual(result.headers.getSetCookie(), ["one=1; Secure", "two=2; Secure"]);
  assert.equal(seen[0].host, "site.example");
  assert.equal(seen[0].cookie, "session=local");
  assert.equal(seen[0].authorization, undefined);
  assert.equal(seen[0]["proxy-authorization"], undefined);
  const untrusted = createProxyFetch(settings, dependencies);
  await assert.rejects(untrusted("https://site.example/page", { method: "GET", signal: AbortSignal.timeout(5000) }), /proxy_unavailable/);
  assert.equal(seen.length, 1, "Untrusted TLS must not send the HTTP request");
});
