#!/usr/bin/env node

import assert from "node:assert/strict";
import {
  chmodSync,
  mkdtempSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  parseHeaderBindings,
  parsePublicHttpRequest,
  performPublicHttpRequest,
  validatePublicHttpGrant,
} from "./support-public-http.mjs";

const GRANT_ID = "50f675a8-43bc-4dbd-89ad-7b522419a1e0";
const ACTION_ID = "50f675a8-43bc-4dbd-89ad-7b522419a1e1";
const ALLOW_BOTH = { actionId: ACTION_ID, allowedHosts: ["api.example.com"], allowGet: true, allowPost: true };
const ALLOW_GET = { actionId: ACTION_ID, allowedHosts: ["api.example.com"], allowGet: true, allowPost: false };
const ALLOW_POST = { actionId: ACTION_ID, allowedHosts: ["api.example.com"], allowGet: false, allowPost: true };

test("requires an active public-HTTP capability for the current run", () => {
  const grantDirectory = mkdtempSync(join(tmpdir(), "support-public-http-"));
  const grantPath = join(grantDirectory, `${GRANT_ID}.json`);
  try {
    writeFileSync(grantPath, JSON.stringify({
      version: 4,
      expires_at_unix_ms: Date.now() + 60_000,
      public_http_get: true,
      http_allowed_hosts: ["api.example.com"],
      public_http_post: false,
      reminder_action_id: ACTION_ID,
      variables: { REGION: "eu" },
      secrets: { API_TOKEN: "secret-value" },
    }), { mode: 0o640 });
    chmodSync(grantPath, 0o640);
    assert.equal(
      validatePublicHttpGrant(GRANT_ID.toUpperCase(), grantDirectory).grantId,
      GRANT_ID,
    );
    assert.equal(
      validatePublicHttpGrant(GRANT_ID, grantDirectory).allowGet,
      true,
    );
    assert.deepEqual(
      validatePublicHttpGrant(GRANT_ID, grantDirectory).secrets,
      { API_TOKEN: "secret-value" },
    );

    for (const grant of [
      { version: 4, expires_at_unix_ms: Date.now() + 60_000, knowledge_lookup: true },
      { version: 4, expires_at_unix_ms: Date.now() - 1, public_http_get: true, reminder_action_id: ACTION_ID },
      { version: 3, expires_at_unix_ms: Date.now() + 60_000, public_http_get: true, reminder_action_id: ACTION_ID },
    ]) {
      writeFileSync(grantPath, JSON.stringify(grant), { mode: 0o640 });
      chmodSync(grantPath, 0o640);
      assert.throws(() => validatePublicHttpGrant(GRANT_ID, grantDirectory));
    }

    writeFileSync(grantPath, JSON.stringify({
      version: 4,
      expires_at_unix_ms: Date.now() + 60_000,
      public_http_get: true,
      http_allowed_hosts: ["api.example.com"],
      reminder_action_id: ACTION_ID,
    }), { mode: 0o660 });
    chmodSync(grantPath, 0o660);
    assert.throws(() => validatePublicHttpGrant(GRANT_ID, grantDirectory));
  } finally {
    rmSync(grantDirectory, { recursive: true, force: true });
  }
});

test("accepts a maximum-sized valid profile secret set in the runtime grant", () => {
  const grantDirectory = mkdtempSync(join(tmpdir(), "support-public-http-large-"));
  const grantPath = join(grantDirectory, `${GRANT_ID}.json`);
  const secrets = Object.fromEntries(Array.from(
    { length: 32 },
    (_, index) => [`SECRET_${index}`, "\u0001".repeat(8_192)],
  ));
  try {
    writeFileSync(grantPath, JSON.stringify({
      version: 4,
      expires_at_unix_ms: Date.now() + 60_000,
      public_http_get: true,
      http_allowed_hosts: ["api.example.com"],
      public_http_post: false,
      reminder_action_id: ACTION_ID,
      variables: {},
      secrets,
    }), { mode: 0o640 });
    assert.ok(statSync(grantPath).size > 512 * 1024);
    assert.equal(
      Object.keys(validatePublicHttpGrant(GRANT_ID, grantDirectory).secrets).length,
      32,
    );
  } finally {
    rmSync(grantDirectory, { recursive: true, force: true });
  }
});

test("accepts only allowlisted HTTPS GET and JSON POST requests", () => {
  assert.deepEqual(
    parsePublicHttpRequest(["GET", "https://api.example.com/v2/catalog"], ALLOW_BOTH),
    {
      method: "GET",
      url: "https://api.example.com/v2/catalog",
      body: undefined,
    },
  );
  assert.deepEqual(
    parsePublicHttpRequest(
      ["POST", "https://api.example.com/v2/search", '{"amount":10}'],
      ALLOW_BOTH,
    ),
    {
      method: "POST",
      url: "https://api.example.com/v2/search",
      body: '{"amount":10}',
      idempotencyKey: ACTION_ID,
    },
  );
});

test("GET and POST permissions are independent", () => {
  assert.deepEqual(
    parsePublicHttpRequest(
      ["GET", "https://api.example.com/v2/catalog"],
      ALLOW_GET,
    ),
    {
      method: "GET",
      url: "https://api.example.com/v2/catalog",
      body: undefined,
    },
  );
  assert.throws(() => parsePublicHttpRequest(
    ["POST", "https://api.example.com/v2/search", '{"amount":10}'],
    ALLOW_GET,
  ));
  assert.deepEqual(parsePublicHttpRequest(
    ["POST", "https://api.example.com/v2/search", '{"amount":10}'],
    ALLOW_POST,
  ), {
    method: "POST",
    url: "https://api.example.com/v2/search",
    body: '{"amount":10}',
    idempotencyKey: ACTION_ID,
  });
  assert.throws(() => parsePublicHttpRequest(
    ["GET", "https://api.example.com/v2/catalog"],
    ALLOW_POST,
  ));
});

test("enforces each agent's domains for both methods, with explicit public opt-in", () => {
  for (const args of [["GET", "https://other.example.org/custom?q=test"], ["POST", "https://other.example.org/custom?q=test", "{}"]]) {
    for (const allowedHosts of [undefined, [], ["api.example.com"]]) {
      assert.throws(() => parsePublicHttpRequest(args, { ...ALLOW_BOTH, allowedHosts }));
    }
    for (const allowedHosts of [["other.example.org"], ["*"]]) {
      assert.equal(parsePublicHttpRequest(args, { ...ALLOW_BOTH, allowedHosts }).url, args[1]);
    }
  }
});

test("rejects file access, credentials, custom ports, fragments, and other hosts", () => {
  for (const url of [
    "file:///run/support-agent-secrets/grant.json",
    "http://api.example.com/v2/catalog",
    "https://user:password@api.example.com/v2/catalog",
    "https://api.example.com:444/v2/catalog",
    "https://api.example.com/v2/catalog#fragment",
    "https://api.example.com.evil.example/v2/catalog",
    "https://evil.example/v2/catalog",
  ]) {
    assert.throws(() => parsePublicHttpRequest(["GET", url], ALLOW_GET));
  }
});

test("uses an exact configured hostname allowlist", () => {
  assert.deepEqual(
    parsePublicHttpRequest(
      ["GET", "https://api.api.example.com/data"],
      { ...ALLOW_GET, allowedHosts: ["api.example.com", "api.api.example.com"] },
    ),
    {
      method: "GET",
      url: "https://api.api.example.com/data",
      body: undefined,
    },
  );
  assert.throws(() => parsePublicHttpRequest(
    ["GET", "https://not-api.api.example.com/data"],
    { ...ALLOW_GET, allowedHosts: ["api.example.com", "api.api.example.com"] },
  ));
});

test("does not read the public host allowlist from process environment", () => {
  const previous = process.env.SUPPORT_PUBLIC_HTTP_HOSTS;
  process.env.SUPPORT_PUBLIC_HTTP_HOSTS = "evil.example";
  try {
    assert.throws(() => parsePublicHttpRequest(["GET", "https://evil.example/data"], ALLOW_GET));
    assert.equal(
      parsePublicHttpRequest(["GET", "https://api.example.com/data"], ALLOW_GET).url,
      "https://api.example.com/data",
    );
  } finally {
    if (previous === undefined) delete process.env.SUPPORT_PUBLIC_HTTP_HOSTS;
    else process.env.SUPPORT_PUBLIC_HTTP_HOSTS = previous;
  }
});

test("rejects extra arguments and non-object JSON bodies", () => {
  assert.throws(() => parsePublicHttpRequest([], ALLOW_BOTH));
  assert.throws(() => parsePublicHttpRequest(["GET", "https://api.example.com", "{}", "extra"], ALLOW_BOTH));
  assert.throws(() => parsePublicHttpRequest(["POST", "https://api.example.com", "not-json"], ALLOW_BOTH));
  assert.throws(() => parsePublicHttpRequest(["POST", "https://api.example.com", "[]"], ALLOW_BOTH));
});

test("binds named profile credentials to safe request headers without exposing values", async () => {
  const permissions = {
    ...ALLOW_BOTH,
    variables: { tenant_id: "tenant-7" },
    secrets: { api_token: "secret-value" },
  };
  const bindings = JSON.stringify({
    Authorization: { source: "secret", key: "API_TOKEN", prefix: "Bearer " },
    "X-API-Key": { source: "variable", key: "TENANT_ID" },
  });
  assert.deepEqual(parseHeaderBindings(bindings, permissions), {
    headers: {
      authorization: "Bearer secret-value",
      "x-api-key": "tenant-7",
    },
    secretValues: ["secret-value"],
  });

  const request = parsePublicHttpRequest(
    ["GET", "https://api.example.com/private", bindings],
    permissions,
  );
  let observedHeaders;
  const response = await performPublicHttpRequest(request, async (_url, options) => {
    observedHeaders = options.headers;
    return new Response("echo=secret-value", {
      status: 200,
      headers: { "content-type": "text/plain" },
    });
  });
  assert.equal(observedHeaders.authorization, "Bearer secret-value");
  assert.equal(observedHeaders["x-api-key"], "tenant-7");
  assert.equal(response, "echo=[REDACTED]");

  for (const invalid of [
    { Host: { source: "secret", key: "API_TOKEN" } },
    { Accept: { source: "secret", key: "API_TOKEN" } },
    { "X-HTTP-Method-Override": { source: "secret", key: "API_TOKEN" } },
    { "X-Original-URL": { source: "secret", key: "API_TOKEN" } },
    { Authorization: { source: "secret", key: "MISSING" } },
    { Authorization: { source: "literal", key: "API_TOKEN" } },
    { Authorization: { source: "secret", key: "API_TOKEN", prefix: "bad\n" } },
  ]) {
    assert.throws(() => parseHeaderBindings(JSON.stringify(invalid), permissions));
  }
  assert.throws(() => parseHeaderBindings(JSON.stringify({
    Authorization: { source: "variable", key: "EMPTY", prefix: "DELETE" },
  }), { ...permissions, variables: { EMPTY: "" } }));
});

test("returns only bounded textual response bodies", async () => {
  let observedUrl;
  let observedOptions;
  const response = await performPublicHttpRequest(
    { method: "GET", url: "https://api.example.com/data", body: undefined },
    async (url, options) => {
      observedUrl = url;
      observedOptions = options;
      return new Response('{"ok":true}', {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    },
  );
  assert.equal(response, '{"ok":true}');
  assert.equal(observedUrl, "https://api.example.com/data");
  assert.equal(observedOptions.method, "GET");
  assert.equal(observedOptions.redirect, "error");
  assert.ok(observedOptions.signal instanceof AbortSignal);

  await assert.rejects(() => performPublicHttpRequest(
    { method: "GET", url: "https://api.example.com/image", body: undefined },
    async () => new Response("image", {
      status: 200,
      headers: { "content-type": "image/png" },
    }),
  ));

  await assert.rejects(() => performPublicHttpRequest(
    { method: "GET", url: "https://api.example.com/large", body: undefined },
    async () => new Response(new Uint8Array((1024 * 1024) + 1), {
      status: 200,
      headers: { "content-type": "text/plain" },
    }),
  ));
});

test("POST sends only the canonical JSON body and fixed headers", async () => {
  let observedOptions;
  await performPublicHttpRequest(
    {
      method: "POST",
      url: "https://api.example.com/v2/search",
      body: '{"amount":10}',
      idempotencyKey: ACTION_ID,
    },
    async (_url, options) => {
      observedOptions = options;
      return new Response("ok", {
        status: 200,
        headers: { "content-type": "text/plain" },
      });
    },
  );
  assert.equal(observedOptions.method, "POST");
  assert.equal(observedOptions.body, '{"amount":10}');
  assert.deepEqual(observedOptions.headers, {
    "content-type": "application/json",
    accept: "application/json, text/plain;q=0.9",
    "idempotency-key": ACTION_ID,
  });
  assert.equal(observedOptions.redirect, "error");
});
