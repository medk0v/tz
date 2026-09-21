#!/usr/bin/env node

import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
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
const MARKER_CONTENT = "{\"requested\":true}\n";
const temporaryDirectories = [];

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

function setup() {
  const root = mkdtempSync(join(tmpdir(), "support-resolve-conversation-"));
  temporaryDirectories.push(root);
  const grantDirectory = join(root, "grants");
  const actionDirectory = join(root, "actions");
  mkdirSync(grantDirectory);
  mkdirSync(actionDirectory);
  return { grantDirectory, actionDirectory };
}

function writeGrant(grantDirectory, overrides = {}) {
  const grant = {
    version: 4,
    expires_at_unix_ms: Date.now() + 60_000,
    resolve_conversation: true,
    ...overrides,
  };
  writeFileSync(join(grantDirectory, `${GRANT_ID}.json`), JSON.stringify(grant));
}

function run(grantDirectory, actionDirectory, ...arguments_) {
  const result = spawnSync(process.execPath, [
    RUNNER_PATH,
    "resolve",
    grantDirectory,
    actionDirectory,
    ...arguments_,
  ], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  return JSON.parse(result.stdout);
}

test("creates the exact resolution marker and is idempotent", () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);

  assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID), {
    ok: true,
    requested: true,
  });
  assert.equal(
    readFileSync(join(actionDirectory, `${GRANT_ID}.resolve`), "utf8"),
    MARKER_CONTENT,
  );
  assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID), {
    ok: true,
    requested: true,
  });
});

test("requires exactly one UUID argument", () => {
  const { grantDirectory, actionDirectory } = setup();
  writeGrant(grantDirectory);

  assert.deepEqual(run(grantDirectory, actionDirectory), {
    ok: false,
    error: "invalid_arguments",
  });
  assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID, GRANT_ID), {
    ok: false,
    error: "invalid_arguments",
  });
});

test("requires an unexpired version 4 grant with explicit capability", () => {
  for (const overrides of [
    { version: 2 },
    { expires_at_unix_ms: Date.now() - 1 },
    { resolve_conversation: false },
  ]) {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory, overrides);
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID), {
      ok: false,
      error: "not_allowed",
    });
  }
});

test("rejects symlink grants and marker collisions", () => {
  {
    const { grantDirectory, actionDirectory } = setup();
    const targetPath = join(grantDirectory, "target.json");
    writeFileSync(targetPath, JSON.stringify({
      version: 4,
      expires_at_unix_ms: Date.now() + 60_000,
      resolve_conversation: true,
    }));
    symlinkSync(targetPath, join(grantDirectory, `${GRANT_ID}.json`));
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID), {
      ok: false,
      error: "invalid_grant",
    });
  }

  {
    const { grantDirectory, actionDirectory } = setup();
    writeGrant(grantDirectory);
    writeFileSync(join(actionDirectory, `${GRANT_ID}.resolve`), "unexpected\n");
    assert.deepEqual(run(grantDirectory, actionDirectory, GRANT_ID), {
      ok: false,
      error: "action_unavailable",
    });
  }
});
