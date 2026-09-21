#!/usr/bin/env node

import { interceptTestTool, normalizeTestAction } from "./support-test-tool.mjs";

import {
  closeSync,
  constants,
  fchmodSync,
  fsyncSync,
  lstatSync,
  openSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_GRANT_BYTES = 3 * 1024 * 1024;
const MAX_MARKER_BYTES = 64;
const MARKER_CONTENT = "{\"requested\":true}\n";
const GRANT_DIRECTORY = "/run/support-agent-secrets";
const ACTION_DIRECTORY = "/run/support-agent-actions";

function finish(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
  process.exit(0);
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isExpectedMarker(markerPath) {
  try {
    const metadata = lstatSync(markerPath);
    return metadata.isFile()
      && !metadata.isSymbolicLink()
      && metadata.size <= MAX_MARKER_BYTES
      && readFileSync(markerPath, "utf8") === MARKER_CONTENT;
  } catch {
    return false;
  }
}

export function runResolveConversation(
  arguments_,
  {
    grantDirectory: grantDirectoryInput = GRANT_DIRECTORY,
    actionDirectory: actionDirectoryInput = ACTION_DIRECTORY,
  } = {},
) {
const [grantIdInput, ...extraArguments] = arguments_;
if (extraArguments.length !== 0 || !UUID_PATTERN.test(grantIdInput ?? "")) {
  finish({ ok: false, error: "invalid_arguments" });
}

const grantId = grantIdInput.toLowerCase();
const grantDirectory = resolve(grantDirectoryInput);
const actionDirectory = resolve(actionDirectoryInput);
const grantPath = resolve(join(grantDirectory, `${grantId}.json`));
const markerPath = resolve(join(actionDirectory, `${grantId}.resolve`));
if (dirname(grantPath) !== grantDirectory || dirname(markerPath) !== actionDirectory) {
  finish({ ok: false, error: "invalid_grant" });
}

let grant;
try {
  const metadata = lstatSync(grantPath);
  if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > MAX_GRANT_BYTES) {
    finish({ ok: false, error: "invalid_grant" });
  }
  grant = JSON.parse(readFileSync(grantPath, "utf8"));
} catch {
  finish({ ok: false, error: "invalid_grant" });
}

if (
  !isObject(grant)
  || grant.version !== 4
  || !Number.isSafeInteger(grant.expires_at_unix_ms)
  || grant.expires_at_unix_ms <= Date.now()
  || grant.resolve_conversation !== true
) {
  finish({ ok: false, error: "not_allowed" });
}

try {
  const metadata = lstatSync(actionDirectory);
  if (!metadata.isDirectory() || metadata.isSymbolicLink()) {
    finish({ ok: false, error: "action_unavailable" });
  }
} catch {
  finish({ ok: false, error: "action_unavailable" });
}

let markerHandle;
try {
  markerHandle = openSync(
    markerPath,
    constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW,
    0o640,
  );
  writeFileSync(markerHandle, MARKER_CONTENT, "utf8");
  fchmodSync(markerHandle, 0o640);
  fsyncSync(markerHandle);
  closeSync(markerHandle);
  markerHandle = undefined;
} catch (error) {
  if (markerHandle !== undefined) {
    try {
      closeSync(markerHandle);
    } catch {
      // The original write error is the actionable failure.
    }
  }
  if (error?.code !== "EEXIST" || !isExpectedMarker(markerPath)) {
    finish({ ok: false, error: "action_unavailable" });
  }
}

finish({ ok: true, requested: true });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [grantId, ...args] = process.argv.slice(2);
  if (!await interceptTestTool("resolve", grantId, args, (args) => normalizeTestAction("resolve", args))) runResolveConversation(process.argv.slice(2));
}
