#!/usr/bin/env node

import assert from "node:assert/strict";
import {
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { afterEach, test } from "node:test";

const RUNNER_PATH = join(
  dirname(fileURLToPath(import.meta.url)),
  "support-wrapper-test-runner.mjs",
);
const GRANT_ID = "50f675a8-43bc-4dbd-89ad-7b522419a1e0";
const RETRY_GRANT_ID = "019c9e10-b484-7cc2-b743-fb251615eaed";
const REMINDER_ACTION_ID = "019c9e0f-20c8-7d3d-a9ad-9d3d2c55e2ca";
const temporaryDirectories = [];

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

function setup() {
  const root = mkdtempSync(join(tmpdir(), "support-schedule-reminder-"));
  temporaryDirectories.push(root);
  const grantDirectory = join(root, "grants");
  const actionDirectory = join(root, "actions");
  mkdirSync(grantDirectory);
  mkdirSync(actionDirectory);
  return { root, grantDirectory, actionDirectory };
}

function writeGrant(grantDirectory, overrides = {}, grantId = GRANT_ID) {
  const grant = {
    version: 4,
    expires_at_unix_ms: Date.now() + 60_000,
    schedule_reminder: true,
    reminder_action_id: REMINDER_ACTION_ID,
    ...overrides,
  };
  writeFileSync(join(grantDirectory, `${grantId}.json`), JSON.stringify(grant));
}

function run(grantDirectory, actionDirectory, ...arguments_) {
  const result = spawnSync(process.execPath, [
    RUNNER_PATH,
    "reminder",
    grantDirectory,
    actionDirectory,
    ...arguments_,
  ], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  assert.match(result.stdout, /^\{.*\}\n$/);
  return JSON.parse(result.stdout);
}

test("creates the exact reminder marker with restrictive permissions and is idempotent", () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);

  assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID.toUpperCase(), "300"), {
    ok: true,
    requested: true,
    delay_seconds: 300,
  });
  const markerPath = join(actionDirectory, `${REMINDER_ACTION_ID}.reminder`);
  assert.equal(readFileSync(markerPath, "utf8"), "{\"delay_seconds\":300}\n");
  assert.equal(statSync(markerPath).mode & 0o777, 0o640);
  assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
    ok: true,
    requested: true,
    delay_seconds: 300,
  });
});

test("reuses the stable reminder action across provider retries", () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  writeGrant(grantDirectory, {}, RETRY_GRANT_ID);

  assert.equal(run(grantDirectory, actionDirectory, GRANT_ID, "300").ok, true);
  assert.deepEqual(run(grantDirectory, actionDirectory, RETRY_GRANT_ID, "300"), {
    ok: true,
    requested: true,
    delay_seconds: 300,
  });
  assert.equal(
    readFileSync(join(actionDirectory, `${REMINDER_ACTION_ID}.reminder`), "utf8"),
    "{\"delay_seconds\":300}\n",
  );
});

test("accepts the inclusive delay boundaries", () => {
  for (const delay of ["10", "82800"]) {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory);

    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, delay), {
      ok: true,
      requested: true,
      delay_seconds: Number(delay),
    });
    assert.equal(
      readFileSync(join(actionDirectory, `${REMINDER_ACTION_ID}.reminder`), "utf8"),
      `{\"delay_seconds\":${delay}}\n`,
    );
  }
});

test("requires exactly one UUID and one canonical integer delay", () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);

  for (const arguments_ of [
    [],
    [GRANT_ID],
    ["not-a-uuid", "5"],
    [GRANT_ID, "5", "extra"],
    [GRANT_ID, "0"],
    [GRANT_ID, "9"],
    [GRANT_ID, "82801"],
    [GRANT_ID, "-1"],
    [GRANT_ID, "+5"],
    [GRANT_ID, "01"],
    [GRANT_ID, "1.5"],
    [GRANT_ID, "1e3"],
    [GRANT_ID, " 5"],
    [GRANT_ID, "5 "],
  ]) {
    assert.deepEqual(run(grantDirectory, actionDirectory, ...arguments_), {
      ok: false,
      error: "invalid_arguments",
    });
  }
});

test("requires an unexpired version 4 grant with explicit capability", () => {
  for (const overrides of [
    { version: 2 },
    { version: 3 },
    { expires_at_unix_ms: Date.now() - 1 },
    { expires_at_unix_ms: "never" },
    { schedule_reminder: false },
    { schedule_reminder: undefined },
    { reminder_action_id: undefined },
    { reminder_action_id: "not-a-uuid" },
  ]) {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory, overrides);
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
      ok: false,
      error: "not_allowed",
    });
  }
});

test("rejects malformed, oversized, and symlinked grants", () => {
  {
    const { grantDirectory, actionDirectory } = setup();
    writeFileSync(join(grantDirectory, `${GRANT_ID}.json`), "not-json");
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
      ok: false,
      error: "invalid_grant",
    });
  }

  {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory, { padding: "x".repeat(3 * 1024 * 1024) });
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
      ok: false,
      error: "invalid_grant",
    });
  }

  {
    const { grantDirectory, actionDirectory } = setup();
    const targetPath = join(grantDirectory, "target.json");
    writeFileSync(targetPath, JSON.stringify({
      version: 4,
      expires_at_unix_ms: Date.now() + 60_000,
      schedule_reminder: true,
      reminder_action_id: REMINDER_ACTION_ID,
    }));
    symlinkSync(targetPath, join(grantDirectory, `${GRANT_ID}.json`));
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
      ok: false,
      error: "invalid_grant",
    });
  }
});

test("rejects unavailable action directories and unsafe marker collisions", () => {
  {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory);
    rmSync(actionDirectory, { recursive: true });
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
      ok: false,
      error: "action_unavailable",
    });
  }

  {
    const { root, grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory);
    const targetDirectory = join(root, "action-target");
    mkdirSync(targetDirectory);
    rmSync(actionDirectory, { recursive: true });
    symlinkSync(targetDirectory, actionDirectory);
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
      ok: false,
      error: "action_unavailable",
    });
  }

  {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory);
    writeFileSync(
      join(actionDirectory, `${REMINDER_ACTION_ID}.reminder`),
      "{\"delay_seconds\":301}\n",
    );
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
      ok: false,
      error: "action_unavailable",
    });
  }

  {
    const { root, grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory);
    const targetPath = join(root, "target.reminder");
    writeFileSync(targetPath, "{\"delay_seconds\":300}\n");
    symlinkSync(targetPath, join(actionDirectory, `${REMINDER_ACTION_ID}.reminder`));
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
      ok: false,
      error: "action_unavailable",
    });
  }
});

test("does not accept an existing marker that became group-writable", () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);
  const markerPath = join(actionDirectory, `${REMINDER_ACTION_ID}.reminder`);
  writeFileSync(markerPath, "{\"delay_seconds\":300}\n", { mode: 0o660 });
  chmodSync(markerPath, 0o660);

  assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, "300"), {
    ok: false,
    error: "action_unavailable",
  });
});
