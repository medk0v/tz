import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { Readable } from "node:stream";
import { test } from "node:test";
import { fetchPublicHttps, publicHttpsUrl, resolvePublicHttpsTarget } from "./support-public-network.mjs";

test("accepts project-defined HTTPS paths and queries without a customer endpoint list", () => {
  for (const url of ["https://support.example.net/v2/tickets/42?expand=events", "https://api.example.org/search", "https://catalog.example.com/items"]) {
    assert.equal(publicHttpsUrl(url).href, url);
  }
  for (const url of ["http://api.example.org", "https://user:password@api.example.org", "https://api.example.org:444", "https://api.example.org/#fragment", "https://localhost", "https://127.0.0.1", "https://[::1]", "file:///etc/passwd"]) {
    assert.throws(() => publicHttpsUrl(url));
  }
});

test("rejects private and mixed DNS answers before opening the HTTP connection", async () => {
  for (const records of [[], [{ address: "127.0.0.1" }], [{ address: "169.254.169.254" }], [{ address: "1.1.1.1" }, { address: "10.0.0.1" }]]) {
    let opened = false;
    await assert.rejects(() => fetchPublicHttps("https://api.example.org/data", { method: "GET" }, async () => records, () => { opened = true; }));
    assert.equal(opened, false);
  }
});

test("pins validated DNS for HTTP requests and retains the original TLS hostname", async () => {
  for (const address of ["1.1.1.1", "2606:4700:4700::1111"]) {
    const target = await resolvePublicHttpsTarget("https://api.example.org/data", async () => [{ address }]);
    let observedOptions;
    let sentBody;
    const response = await fetchPublicHttps("https://api.example.org/search?q=hello", {
      method: "POST", headers: { authorization: "Bearer test-secret" }, body: '{"query":"hello"}',
    }, async () => [{ address }], (url, options, onResponse) => {
      assert.equal(url.hostname, "api.example.org");
      assert.equal(url.search, "?q=hello");
      observedOptions = options;
      const request = new EventEmitter();
      request.end = (body) => {
        sentBody = body;
        const incoming = Readable.from([Buffer.from('{"ok":true}')]);
        incoming.statusCode = 200;
        incoming.rawHeaders = ["content-type", "application/json"];
        onResponse(incoming);
      };
      return request;
    });
    assert.equal(await response.text(), '{"ok":true}');
    assert.equal(observedOptions.agent, false);
    assert.equal(observedOptions.headers.authorization, "Bearer test-secret");
    assert.equal(sentBody, '{"query":"hello"}');
    observedOptions.lookup("api.example.org", { all: true }, (error, records) => {
      assert.equal(error, null);
      assert.deepEqual(records, [{ address, family: target.family }]);
    });
    observedOptions.lookup("api.example.org", {}, (error, resolved, family) => {
      assert.equal(error, null);
      assert.equal(resolved, address);
      assert.equal(family, target.family);
    });
  }
});

test("does not follow redirects from an otherwise permitted endpoint", async () => {
  let connections = 0;
  await assert.rejects(() => fetchPublicHttps("https://api.example.org/data", { method: "GET" }, async () => [{ address: "1.1.1.1" }], (_url, _options, onResponse) => {
    connections += 1;
    const request = new EventEmitter();
    request.end = () => {
      const incoming = Readable.from([]);
      incoming.statusCode = 302;
      incoming.rawHeaders = ["location", "https://127.0.0.1/private"];
      onResponse(incoming);
    };
    return request;
  }), /request_failed/);
  assert.equal(connections, 1);
});
