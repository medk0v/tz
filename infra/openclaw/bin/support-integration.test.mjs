#!/usr/bin/env node

import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";

import { runIntegration } from "./support-integration.mjs";

const INTEGRATION_KEY = "order_api";
const ACTION_KEY = "get_status";
const PARAMETER_NAMES = ["order_id", "region_code"];
// Allow for synchronous lock and marker fsync calls on shared CI runners.
const REQUEST_TIMEOUT_MS = 5_000;

function validGrant(overrides = {}) {
  return {
    version: 4,
    expires_at_unix_ms: Date.now() + 60_000,
    integration_lookup: true,
    integrations: [{
      key: INTEGRATION_KEY,
      actions: [{ key: ACTION_KEY, parameter_names: PARAMETER_NAMES }],
    }],
    ...overrides,
  };
}

function createRuntime(grant = validGrant()) {
  const root = mkdtempSync(join(tmpdir(), "support-integration-"));
  const grantDirectory = join(root, "grants");
  const actionDirectory = join(root, "actions");
  const grantId = randomUUID();
  mkdirSync(grantDirectory);
  mkdirSync(actionDirectory);
  writeFileSync(
    join(grantDirectory, `${grantId}.json`),
    `${JSON.stringify(grant)}\n`,
    { mode: 0o640 },
  );
  return {
    root,
    grantDirectory,
    actionDirectory,
    grantId,
    options: {
      grantDirectory,
      actionDirectory,
      responseTimeoutMs: REQUEST_TIMEOUT_MS,
      pollIntervalMs: 5,
    },
  };
}

function integrationArguments(grantId, parameters = {
  order_id: "order-42",
  region_code: "eu-west",
}) {
  return [grantId, INTEGRATION_KEY, ACTION_KEY, JSON.stringify(parameters)];
}

async function waitForPath(path, timeoutMs = REQUEST_TIMEOUT_MS) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (existsSync(path)) return;
    await delay(5);
  }
  assert.fail(`timed out waiting for ${path}`);
}

test("requires the exact integration action in a current capability grant", async () => {
  const cases = [
    validGrant({ integration_lookup: false }),
    validGrant({ expires_at_unix_ms: Date.now() - 1 }),
    validGrant({ integrations: [] }),
    validGrant({
      integrations: [{
        key: INTEGRATION_KEY,
        actions: [{ key: "other_action", parameter_names: PARAMETER_NAMES }],
      }],
    }),
    validGrant({
      integrations: [{
        key: "other_api",
        actions: [{ key: ACTION_KEY, parameter_names: PARAMETER_NAMES }],
      }],
    }),
  ];

  for (const grant of cases) {
    const runtime = createRuntime(grant);
    try {
      assert.deepEqual(
        await runIntegration(integrationArguments(runtime.grantId), runtime.options),
        { ok: false, error: "not_allowed" },
      );
      assert.deepEqual(readdirSync(runtime.actionDirectory), []);
    } finally {
      rmSync(runtime.root, { recursive: true, force: true });
    }
  }
});

test("rejects unsafe grants and transport metadata", async () => {
  const catalogCases = [
    validGrant({
      integrations: Array.from({ length: 33 }, (_, index) => ({
        key: `api_${index}`,
        actions: [],
      })),
    }),
    validGrant({
      integrations: [{
        key: INTEGRATION_KEY,
        url: "https://example.test",
        actions: [{ key: ACTION_KEY, parameter_names: PARAMETER_NAMES }],
      }],
    }),
    validGrant({
      integrations: [{
        key: INTEGRATION_KEY,
        actions: [{
          key: ACTION_KEY,
          method: "GET",
          parameter_names: PARAMETER_NAMES,
        }],
      }],
    }),
    validGrant({
      integrations: [{
        key: INTEGRATION_KEY,
        actions: [{ key: ACTION_KEY, parameter_names: ["OrderId"] }],
      }],
    }),
    validGrant({
      integrations: [{
        key: INTEGRATION_KEY,
        actions: [{ key: ACTION_KEY, parameter_names: ["a".repeat(65)] }],
      }],
    }),
  ];

  for (const grant of catalogCases) {
    const runtime = createRuntime(grant);
    try {
      assert.deepEqual(
        await runIntegration(integrationArguments(runtime.grantId), runtime.options),
        { ok: false, error: "not_allowed" },
      );
    } finally {
      rmSync(runtime.root, { recursive: true, force: true });
    }
  }

  const runtime = createRuntime();
  try {
    chmodSync(join(runtime.grantDirectory, `${runtime.grantId}.json`), 0o660);
    assert.deepEqual(
      await runIntegration(integrationArguments(runtime.grantId), runtime.options),
      { ok: false, error: "invalid_grant" },
    );
  } finally {
    rmSync(runtime.root, { recursive: true, force: true });
  }
});

test("accepts parameter names at the one and 64 character boundaries", async () => {
  for (const parameterName of ["q", "a".repeat(64)]) {
    const runtime = createRuntime(validGrant({
      integrations: [{
        key: INTEGRATION_KEY,
        actions: [{ key: ACTION_KEY, parameter_names: [parameterName] }],
      }],
    }));
    const requestPath = join(runtime.actionDirectory, `${runtime.grantId}.integration-request`);
    const responsePath = join(runtime.actionDirectory, `${runtime.grantId}.integration-response`);
    try {
      const resultPromise = runIntegration(
        [
          runtime.grantId,
          INTEGRATION_KEY,
          ACTION_KEY,
          JSON.stringify({ [parameterName]: "42" }),
        ],
        runtime.options,
      );
      await waitForPath(requestPath);
      assert.deepEqual(
        JSON.parse(readFileSync(requestPath, "utf8")).parameters,
        { [parameterName]: "42" },
      );
      writeFileSync(responsePath, JSON.stringify({
        ok: true,
        status_code: 200,
        content_type: "text/plain",
        body: "accepted",
      }), { mode: 0o640 });
      assert.equal((await resultPromise).body, "accepted");
      assert.deepEqual(readdirSync(runtime.actionDirectory), []);
    } finally {
      rmSync(runtime.root, { recursive: true, force: true });
    }
  }
});

test("accepts only exact bounded safe-string parameters", async () => {
  const runtime = createRuntime();
  const invalidParameters = [
    { order_id: "order-42" },
    { order_id: "", region_code: "eu" },
    { order_id: "order-42", region_code: "eu", extra: "no" },
    { order_id: null, region_code: "eu" },
    { order_id: 42, region_code: "eu" },
    { order_id: true, region_code: "eu" },
    { order_id: ["42"], region_code: "eu" },
    { order_id: { value: "42" }, region_code: "eu" },
    { order_id: "O'Reilly", region_code: "eu" },
    { order_id: "line\nbreak", region_code: "eu" },
    { order_id: "slash\\escape", region_code: "eu" },
    { order_id: "заказ-42", region_code: "eu" },
    { order_id: ".", region_code: "eu" },
    { order_id: "..", region_code: "eu" },
    { order_id: "../admin", region_code: "eu" },
    { order_id: "foo/../../admin", region_code: "eu" },
    { order_id: "%2e%2e%2fadmin", region_code: "eu" },
    { order_id: "x".repeat(513), region_code: "eu" },
  ];
  try {
    for (const parameters of invalidParameters) {
      assert.deepEqual(
        await runIntegration(integrationArguments(runtime.grantId, parameters), runtime.options),
        { ok: false, error: "invalid_arguments" },
      );
    }
    assert.deepEqual(
      await runIntegration(
        [runtime.grantId, INTEGRATION_KEY, ACTION_KEY, `{"order_id":"${"x".repeat(33 * 1024)}","region_code":"eu"}`],
        runtime.options,
      ),
      { ok: false, error: "invalid_arguments" },
    );
    assert.deepEqual(
      await runIntegration(
        integrationArguments(runtime.grantId, {
          order_id: "x".repeat(512),
          region_code: "eu",
        }),
        { ...runtime.options, responseTimeoutMs: 5 },
      ),
      { ok: false, error: "integration_unavailable" },
    );
    assert.deepEqual(readdirSync(runtime.actionDirectory), []);
  } finally {
    rmSync(runtime.root, { recursive: true, force: true });
  }
});

test("only an authorized test browser_result action accepts the larger result payload", async () => {
  const catalog = [{ key: "tz_test", actions: ["dispatch", "browser_result", "other_action"].map((key) => ({ key, parameter_names: ["payload"] })) }];
  const runtime = createRuntime(validGrant({ test_execution: true, integrations: catalog }));
  const requestPath = join(runtime.actionDirectory, `${runtime.grantId}.integration-request`);
  const responsePath = join(runtime.actionDirectory, `${runtime.grantId}.integration-response`);
  const call = (action, payload) => [runtime.grantId, "tz_test", action, JSON.stringify({ payload })];
  try {
    for (const [action, size] of [["dispatch", 16_001], ["dispatch", 33 * 1024], ["other_action", 513], ["browser_result", 2 * 1024 * 1024 + 1]]) {
      assert.deepEqual(await runIntegration(call(action, "a".repeat(size)), runtime.options), { ok: false, error: "invalid_arguments" });
    }
    const largePayload = "a".repeat(2 * 1024 * 1024);
    const resultPromise = runIntegration(call("browser_result", largePayload), runtime.options);
    await waitForPath(requestPath);
    assert.equal(JSON.parse(readFileSync(requestPath, "utf8")).parameters.payload, largePayload);
    const body = JSON.stringify({ ok: true, text: "a".repeat(300 * 1024) });
    writeFileSync(responsePath, JSON.stringify({ ok: true, status_code: 200, content_type: "text/plain", body }), { mode: 0o640 });
    assert.equal((await resultPromise).body, body);
    assert.deepEqual(readdirSync(runtime.actionDirectory), []);
    writeFileSync(join(runtime.grantDirectory, `${runtime.grantId}.json`), JSON.stringify(validGrant({ integrations: catalog })), { mode: 0o640 });
    assert.deepEqual(await runIntegration(call("browser_result", "a".repeat(513)), runtime.options), { ok: false, error: "invalid_arguments" });
  } finally { rmSync(runtime.root, { recursive: true, force: true }); }
});

test("publishes one canonical marker and returns a bounded typed response", async () => {
  const runtime = createRuntime();
  const requestPath = join(runtime.actionDirectory, `${runtime.grantId}.integration-request`);
  const responsePath = join(runtime.actionDirectory, `${runtime.grantId}.integration-response`);
  const lockPath = join(runtime.actionDirectory, `${runtime.grantId}.integration-lock`);
  try {
    const resultPromise = runIntegration(
      integrationArguments(runtime.grantId, {
        region_code: "eu west-1",
        order_id: "order:42+retry-20",
      }),
      runtime.options,
    );
    await waitForPath(requestPath);
    assert.equal(existsSync(lockPath), true);
    assert.equal(statSync(requestPath).mode & 0o777, 0o640);
    assert.equal(
      readFileSync(requestPath, "utf8"),
      `${JSON.stringify({
        integration_key: INTEGRATION_KEY,
        action_key: ACTION_KEY,
        parameters: {
          order_id: "order:42+retry-20",
          region_code: "eu west-1",
        },
      })}\n`,
    );
    writeFileSync(responsePath, `${JSON.stringify({
      ok: true,
      status_code: 200,
      content_type: "application/json",
      body: '{"status":"ready"}',
    })}\n`, { mode: 0o640 });

    assert.deepEqual(await resultPromise, {
      ok: true,
      status_code: 200,
      content_type: "application/json",
      body: '{"status":"ready"}',
    });
    assert.deepEqual(readdirSync(runtime.actionDirectory), []);
  } finally {
    rmSync(runtime.root, { recursive: true, force: true });
  }
});

test("serializes calls sharing one grant without replacing the active marker", async () => {
  const runtime = createRuntime();
  const requestPath = join(runtime.actionDirectory, `${runtime.grantId}.integration-request`);
  const responsePath = join(runtime.actionDirectory, `${runtime.grantId}.integration-response`);
  try {
    const firstParameters = { order_id: "first", region_code: "eu" };
    const secondParameters = { order_id: "second", region_code: "us" };
    const firstPromise = runIntegration(
      integrationArguments(runtime.grantId, firstParameters),
      runtime.options,
    );
    await waitForPath(requestPath);
    const firstMarker = readFileSync(requestPath, "utf8");

    const secondPromise = runIntegration(
      integrationArguments(runtime.grantId, secondParameters),
      runtime.options,
    );
    await delay(20);
    assert.equal(readFileSync(requestPath, "utf8"), firstMarker);

    writeFileSync(responsePath, JSON.stringify({
      ok: true,
      status_code: 200,
      content_type: "text/plain",
      body: "first",
    }), { mode: 0o640 });
    assert.equal((await firstPromise).body, "first");

    await waitForPath(requestPath);
    assert.match(readFileSync(requestPath, "utf8"), /"order_id":"second"/);
    writeFileSync(responsePath, JSON.stringify({
      ok: true,
      status_code: 404,
      content_type: "text/plain",
      body: "second",
    }), { mode: 0o640 });
    assert.deepEqual(await secondPromise, {
      ok: true,
      status_code: 404,
      content_type: "text/plain",
      body: "second",
    });
    assert.deepEqual(readdirSync(runtime.actionDirectory), []);
  } finally {
    rmSync(runtime.root, { recursive: true, force: true });
  }
});

test("maps invalid responses and timeouts to one generic error and cleans owned files", async () => {
  const runtime = createRuntime();
  const requestPath = join(runtime.actionDirectory, `${runtime.grantId}.integration-request`);
  const responsePath = join(runtime.actionDirectory, `${runtime.grantId}.integration-response`);
  const invalidResponses = [
    {
      ok: true,
      status_code: 200,
      content_type: "application/json",
      body: "{}",
      headers: { authorization: "forbidden" },
    },
    { ok: true, status_code: 99, content_type: "text/plain", body: "too low" },
    { ok: true, status_code: 600, content_type: "text/plain", body: "too high" },
    { ok: true, status_code: 200.5, content_type: "text/plain", body: "not integer" },
  ];
  try {
    for (const invalidResponse of invalidResponses) {
      const invalidResponsePromise = runIntegration(
        integrationArguments(runtime.grantId),
        runtime.options,
      );
      await waitForPath(requestPath);
      writeFileSync(responsePath, JSON.stringify(invalidResponse), { mode: 0o640 });
      assert.deepEqual(await invalidResponsePromise, {
        ok: false,
        error: "integration_unavailable",
      });
      assert.deepEqual(readdirSync(runtime.actionDirectory), []);
    }

    assert.deepEqual(
      await runIntegration(integrationArguments(runtime.grantId), {
        ...runtime.options,
        responseTimeoutMs: 30,
      }),
      { ok: false, error: "integration_unavailable" },
    );
    assert.deepEqual(readdirSync(runtime.actionDirectory), []);
  } finally {
    rmSync(runtime.root, { recursive: true, force: true });
  }
});
