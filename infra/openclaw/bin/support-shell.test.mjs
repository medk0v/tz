#!/usr/bin/env node

import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  parseShellRequest,
  validateRunnerGrantGid,
} from "./support-shell-runner.mjs";
import {
  buildShellEnvironment,
  validateShellCommand,
  validateShellGrant,
} from "./support-shell.mjs";

test("requires a current version 4 grant with shell enabled", () => {
  const root = mkdtempSync(join(tmpdir(), "support-shell-grant-"));
  const grantDirectory = join(root, "grants");
  mkdirSync(grantDirectory);
  const grantId = randomUUID();
  const expiresAtUnixMs = Date.now() + 60_000;
  try {
    writeFileSync(join(grantDirectory, `${grantId}.json`), JSON.stringify({
      version: 4,
      expires_at_unix_ms: expiresAtUnixMs,
      shell: true,
      variables: { REGION: "eu" },
      secrets: { API_TOKEN: "secret-value" },
    }), { mode: 0o640 });
    assert.deepEqual(validateShellGrant(grantId, grantDirectory), {
      grantId,
      expiresAtUnixMs,
      environment: {
        SUPPORT_VAR_REGION: "eu",
      },
    });

    writeFileSync(join(grantDirectory, `${grantId}.json`), JSON.stringify({
      version: 4,
      expires_at_unix_ms: Date.now() + 60_000,
      shell: false,
    }), { mode: 0o640 });
    assert.throws(() => validateShellGrant(grantId, grantDirectory), /not_allowed/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("exports only bounded visible variables and never profile secrets", () => {
  assert.deepEqual(buildShellEnvironment(
    { REGION: "eu", "not-shell-safe": "ignored" },
  ), {
    SUPPORT_VAR_REGION: "eu",
  });
  assert.throws(() => buildShellEnvironment([]), /not_allowed/);
  assert.throws(() => buildShellEnvironment({ REGION: "bad\0value" }), /not_allowed/);
});

test("accepts the full Unicode size allowed for visible profile variables", () => {
  const maximumUnicodeValue = "🙂".repeat(4_096);
  const environment = buildShellEnvironment({ NOTE: maximumUnicodeValue });
  assert.equal(environment.SUPPORT_VAR_NOTE, maximumUnicodeValue);

  const request = parseShellRequest(JSON.stringify({
    version: 1,
    grant_id: randomUUID(),
    expires_at_unix_ms: Date.now() + 60_000,
    command: "printf '%s' \"$SUPPORT_VAR_NOTE\"",
    environment,
  }));
  assert.equal(request.environment.SUPPORT_VAR_NOTE, maximumUnicodeValue);
});

test("accepts exactly one bounded shell command string", () => {
  assert.equal(validateShellCommand(["printf '%s' ok"]), "printf '%s' ok");
  assert.throws(() => validateShellCommand([]), /invalid_arguments/);
  assert.throws(() => validateShellCommand(["true", "false"]), /invalid_arguments/);
  assert.throws(() => validateShellCommand([" \n\t "]), /invalid_command/);
  assert.throws(() => validateShellCommand(["bad\0command"]), /invalid_command/);
  assert.throws(() => validateShellCommand(["x".repeat(17 * 1024)]), /invalid_command/);
});

test("runner protocol rejects unknown fields and malformed scope", () => {
  const grantId = randomUUID();
  const expiresAtUnixMs = Date.now() + 60_000;
  assert.deepEqual(parseShellRequest(JSON.stringify({
    version: 1,
    grant_id: grantId,
    expires_at_unix_ms: expiresAtUnixMs,
    command: "pwd",
    environment: { SUPPORT_VAR_REGION: "eu" },
  })), {
    version: 1,
    grant_id: grantId,
    expires_at_unix_ms: expiresAtUnixMs,
    command: "pwd",
    environment: { SUPPORT_VAR_REGION: "eu" },
  });
  assert.throws(() => parseShellRequest(JSON.stringify({
    version: 1,
    grant_id: grantId,
    expires_at_unix_ms: expiresAtUnixMs,
    command: "pwd",
    environment: {},
    extra: true,
  })), /invalid_request/);
  assert.throws(() => parseShellRequest(JSON.stringify({
    version: 1,
    grant_id: "not-a-uuid",
    expires_at_unix_ms: expiresAtUnixMs,
    command: "pwd",
    environment: {},
  })), /invalid_request/);
  assert.throws(() => parseShellRequest(JSON.stringify({
    version: 1,
    grant_id: grantId,
    expires_at_unix_ms: expiresAtUnixMs,
    command: "pwd",
    environment: { PATH: "/tmp" },
  })), /invalid_request/);
  assert.throws(() => parseShellRequest(JSON.stringify({
    version: 1,
    grant_id: grantId,
    expires_at_unix_ms: expiresAtUnixMs,
    command: "pwd",
    environment: { SUPPORT_SECRET_API_TOKEN: "secret" },
  })), /invalid_request/);
  assert.throws(() => parseShellRequest(JSON.stringify({
    version: 1,
    grant_id: grantId,
    expires_at_unix_ms: Date.now() - 1,
    command: "pwd",
    environment: {},
  })), /invalid_request/);
});

test("runner socket group cannot equal the sandbox command group", () => {
  assert.equal(validateRunnerGrantGid("1000"), 1000);
  assert.throws(() => validateRunnerGrantGid("65534"), /invalid/);
  assert.throws(() => validateRunnerGrantGid("1000suffix"), /invalid/);
  assert.throws(() => validateRunnerGrantGid(""), /invalid/);
});
