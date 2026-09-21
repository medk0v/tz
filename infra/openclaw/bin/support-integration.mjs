#!/usr/bin/env node

import {
  closeSync,
  constants,
  fchmodSync,
  fstatSync,
  fsyncSync,
  linkSync,
  lstatSync,
  openSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const KEY_PATTERN = /^[a-z][a-z0-9_]{1,63}$/;
const PARAMETER_NAME_PATTERN = /^[a-z][a-z0-9_]{0,63}$/;
const MAX_GRANT_BYTES = 3 * 1024 * 1024;
const MAX_INTEGRATIONS = 32;
const MAX_PARAMETERS = 32;
const MAX_PARAMETERS_BYTES = 32 * 1024;
const MAX_PARAMETER_STRING_BYTES = 512;
const MAX_RESPONSE_BYTES = 256 * 1024;
const RESPONSE_TIMEOUT_MS = 30_000;
const POLL_INTERVAL_MS = 25;
const GRANT_DIRECTORY = "/run/support-agent-secrets";
const ACTION_DIRECTORY = "/run/support-agent-actions";

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function hasExactKeys(value, expectedKeys) {
  if (!isObject(value)) return false;
  const keys = Object.keys(value);
  return keys.length === expectedKeys.length
    && expectedKeys.every((key) => Object.hasOwn(value, key));
}

function readSafeFile(path, maximumBytes) {
  let handle;
  try {
    handle = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    const metadata = fstatSync(handle);
    if (
      !metadata.isFile()
      || (metadata.mode & 0o022) !== 0
      || metadata.size === 0
      || metadata.size > maximumBytes
    ) {
      return undefined;
    }
    return readFileSync(handle, "utf8");
  } catch {
    return undefined;
  } finally {
    if (handle !== undefined) {
      try {
        closeSync(handle);
      } catch {
        // The caller receives a generic unavailable result.
      }
    }
  }
}

function pathExists(path) {
  try {
    lstatSync(path);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    return true;
  }
}

function removeFileIfPresent(path) {
  try {
    unlinkSync(path);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}

function writeAtomicMarker(temporaryPath, markerPath, content) {
  let handle;
  try {
    handle = openSync(
      temporaryPath,
      constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW,
      0o640,
    );
    writeFileSync(handle, content, "utf8");
    fchmodSync(handle, 0o640);
    fsyncSync(handle);
    closeSync(handle);
    handle = undefined;
    linkSync(temporaryPath, markerPath);
    unlinkSync(temporaryPath);
  } catch (error) {
    if (handle !== undefined) {
      try {
        closeSync(handle);
      } catch {
        // The original write error is the actionable failure.
      }
    }
    try {
      removeFileIfPresent(temporaryPath);
    } catch {
      // The caller receives a generic unavailable result.
    }
    throw error;
  }
}

async function acquireLock(path, deadline, pollIntervalMs) {
  while (Date.now() < deadline) {
    let handle;
    let created = false;
    try {
      handle = openSync(
        path,
        constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW,
        0o640,
      );
      created = true;
      writeFileSync(handle, `${process.pid}\n`, "utf8");
      fchmodSync(handle, 0o640);
      fsyncSync(handle);
      closeSync(handle);
      return;
    } catch (error) {
      if (handle !== undefined) {
        try {
          closeSync(handle);
        } catch {
          // The original lock error is the actionable failure.
        }
      }
      if (created) {
        try {
          removeFileIfPresent(path);
        } catch {
          // The caller receives a generic unavailable result.
        }
      }
      if (error?.code !== "EEXIST") throw error;
      const remaining = deadline - Date.now();
      if (remaining <= 0) break;
      await delay(Math.min(pollIntervalMs, remaining));
    }
  }
  throw new Error("integration_lock_timeout");
}

function authorizedParameterNames(grant, integrationKey, actionKey, now) {
  if (
    !isObject(grant)
    || grant.version !== 4
    || !Number.isSafeInteger(grant.expires_at_unix_ms)
    || grant.expires_at_unix_ms <= now
    || grant.integration_lookup !== true
    || !Array.isArray(grant.integrations)
    || grant.integrations.length > MAX_INTEGRATIONS + (grant.test_execution === true ? 1 : 0)
  ) {
    return undefined;
  }

  const integrationKeys = new Set();
  let matchedParameterNames;
  for (const integration of grant.integrations) {
    if (
      !hasExactKeys(integration, ["key", "actions"])
      || typeof integration.key !== "string"
      || !KEY_PATTERN.test(integration.key)
      || integrationKeys.has(integration.key)
      || !Array.isArray(integration.actions)
    ) {
      return undefined;
    }
    integrationKeys.add(integration.key);

    const actionKeys = new Set();
    for (const action of integration.actions) {
      if (
        !hasExactKeys(action, ["key", "parameter_names"])
        || typeof action.key !== "string"
        || !KEY_PATTERN.test(action.key)
        || actionKeys.has(action.key)
        || !Array.isArray(action.parameter_names)
        || action.parameter_names.length > MAX_PARAMETERS
        || action.parameter_names.some((name) => (
          typeof name !== "string" || !PARAMETER_NAME_PATTERN.test(name)
        ))
        || new Set(action.parameter_names).size !== action.parameter_names.length
      ) {
        return undefined;
      }
      actionKeys.add(action.key);
      if (integration.key === integrationKey && action.key === actionKey) {
        matchedParameterNames = action.parameter_names;
      }
    }
  }
  return matchedParameterNames;
}

function parseParameters(input, expectedNames, maximumStringBytes = MAX_PARAMETER_STRING_BYTES, maximumBytes = MAX_PARAMETERS_BYTES) {
  if (
    typeof input !== "string"
    || Buffer.byteLength(input, "utf8") > maximumBytes
  ) {
    return undefined;
  }
  let parameters;
  try {
    parameters = JSON.parse(input);
  } catch {
    return undefined;
  }
  if (!isObject(parameters)) return undefined;
  const keys = Object.keys(parameters);
  if (
    keys.length > MAX_PARAMETERS
    || keys.length !== expectedNames.length
    || expectedNames.some((name) => !Object.hasOwn(parameters, name))
    || Object.values(parameters).some((value) => (
      typeof value !== "string"
      || value.length === 0
      || Buffer.byteLength(value, "utf8") > maximumStringBytes
      || value === "."
      || value === ".."
      || !/^[A-Za-z0-9._~:@+ -]+$/.test(value)
    ))
  ) {
    return undefined;
  }
  return Object.fromEntries(
    [...expectedNames].sort().map((name) => [name, parameters[name]]),
  );
}

function parseResponse(content) {
  let response;
  try {
    response = JSON.parse(content);
  } catch {
    return undefined;
  }
  if (
    hasExactKeys(response, ["ok", "error"])
    && response.ok === false
    && response.error === "integration_unavailable"
  ) {
    return { ok: false, error: "integration_unavailable" };
  }
  if (
    hasExactKeys(response, ["ok", "status_code", "content_type", "body"])
    && response.ok === true
    && Number.isInteger(response.status_code)
    && response.status_code >= 100
    && response.status_code <= 599
    && ["application/json", "text/plain"].includes(response.content_type)
    && typeof response.body === "string"
  ) {
    return {
      ok: true,
      status_code: response.status_code,
      content_type: response.content_type,
      body: response.body,
    };
  }
  return undefined;
}

function boundedOutput(payload) {
  const output = `${JSON.stringify(payload)}\n`;
  return Buffer.byteLength(output, "utf8") <= MAX_RESPONSE_BYTES
    ? output
    : '{"ok":false,"error":"integration_unavailable"}\n';
}

export async function runIntegration(
  arguments_,
  {
    grantDirectory: grantDirectoryInput = GRANT_DIRECTORY,
    actionDirectory: actionDirectoryInput = ACTION_DIRECTORY,
    responseTimeoutMs = RESPONSE_TIMEOUT_MS,
    pollIntervalMs = POLL_INTERVAL_MS,
  } = {},
) {
  const [grantIdInput, integrationKey, actionKey, parametersInput, ...extraArguments] = arguments_;
  if (
    extraArguments.length !== 0
    || !UUID_PATTERN.test(grantIdInput ?? "")
    || !KEY_PATTERN.test(integrationKey ?? "")
    || !KEY_PATTERN.test(actionKey ?? "")
  ) {
    return { ok: false, error: "invalid_arguments" };
  }

  const grantId = grantIdInput.toLowerCase();
  const grantDirectory = resolve(grantDirectoryInput);
  const actionDirectory = resolve(actionDirectoryInput);
  const grantPath = resolve(join(grantDirectory, `${grantId}.json`));
  const requestPath = resolve(join(actionDirectory, `${grantId}.integration-request`));
  const requestTemporaryPath = resolve(join(actionDirectory, `${grantId}.integration-request.tmp`));
  const responsePath = resolve(join(actionDirectory, `${grantId}.integration-response`));
  const responseTemporaryPath = resolve(join(actionDirectory, `${grantId}.integration-response.tmp`));
  const lockPath = resolve(join(actionDirectory, `${grantId}.integration-lock`));
  if (
    dirname(grantPath) !== grantDirectory
    || [requestPath, requestTemporaryPath, responsePath, responseTemporaryPath, lockPath]
      .some((path) => dirname(path) !== actionDirectory)
  ) {
    return { ok: false, error: "invalid_grant" };
  }

  const grantContent = readSafeFile(grantPath, MAX_GRANT_BYTES);
  if (grantContent === undefined) {
    return { ok: false, error: "invalid_grant" };
  }
  let grant;
  try {
    grant = JSON.parse(grantContent);
  } catch {
    return { ok: false, error: "invalid_grant" };
  }
  const parameterNames = authorizedParameterNames(
    grant,
    integrationKey,
    actionKey,
    Date.now(),
  );
  if (parameterNames === undefined) {
    return { ok: false, error: "not_allowed" };
  }
  const testDispatch = grant.test_execution === true && integrationKey === "tz_test" && actionKey === "dispatch";
  const testBrowserResult = grant.test_execution === true && integrationKey === "tz_test" && actionKey === "browser_result";
  const parameters = parseParameters(parametersInput, parameterNames,
    testBrowserResult ? 2 * 1024 * 1024 : testDispatch ? 16_000 : MAX_PARAMETER_STRING_BYTES,
    testBrowserResult ? 2 * 1024 * 1024 + 64 : MAX_PARAMETERS_BYTES);
  if (parameters === undefined) {
    return { ok: false, error: "invalid_arguments" };
  }

  let lockAcquired = false;
  let result = { ok: false, error: "integration_unavailable" };
  const deadline = Math.min(
    Date.now() + Math.min(RESPONSE_TIMEOUT_MS, Math.max(1, responseTimeoutMs)),
    grant.expires_at_unix_ms,
  );
  try {
    const actionDirectoryMetadata = lstatSync(actionDirectory);
    if (!actionDirectoryMetadata.isDirectory() || actionDirectoryMetadata.isSymbolicLink()) {
      return result;
    }
    await acquireLock(lockPath, deadline, Math.max(1, pollIntervalMs));
    lockAcquired = true;
    if (grant.expires_at_unix_ms <= Date.now()) {
      throw new Error("integration_grant_expired");
    }

    removeFileIfPresent(requestPath);
    removeFileIfPresent(requestTemporaryPath);
    removeFileIfPresent(responsePath);
    removeFileIfPresent(responseTemporaryPath);
    writeAtomicMarker(
      requestTemporaryPath,
      requestPath,
      `${JSON.stringify({ integration_key: integrationKey, action_key: actionKey, parameters })}\n`,
    );

    while (Date.now() < deadline) {
      const responseContent = readSafeFile(responsePath, testDispatch || testBrowserResult ? 2 * 1024 * 1024 : MAX_RESPONSE_BYTES);
      if (responseContent !== undefined) {
        result = parseResponse(responseContent)
          ?? { ok: false, error: "integration_unavailable" };
        break;
      }
      if (pathExists(responsePath)) break;
      const remaining = deadline - Date.now();
      if (remaining <= 0) break;
      await delay(Math.min(Math.max(1, pollIntervalMs), remaining));
    }
  } catch {
    result = { ok: false, error: "integration_unavailable" };
  } finally {
    if (lockAcquired) {
      let cleanupFailed = false;
      for (const path of [
        requestPath,
        requestTemporaryPath,
        responsePath,
        responseTemporaryPath,
        lockPath,
      ]) {
        try {
          removeFileIfPresent(path);
        } catch {
          cleanupFailed = true;
        }
      }
      if (cleanupFailed) {
        result = { ok: false, error: "integration_unavailable" };
      }
    }
  }
  return result;
}

async function main() {
  const payload = await runIntegration(process.argv.slice(2));
  process.stdout.write(boundedOutput(payload));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
