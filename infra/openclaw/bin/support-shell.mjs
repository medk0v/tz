#!/usr/bin/env node

import { interceptTestTool } from "./support-test-tool.mjs";

import {
  closeSync,
  constants,
  fstatSync,
  openSync,
  readFileSync,
} from "node:fs";
import { createConnection } from "node:net";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_GRANT_BYTES = 3 * 1024 * 1024;
const MAX_COMMAND_BYTES = 16 * 1024;
const MAX_RESPONSE_BYTES = 1024 * 1024 + 64 * 1024;
const REQUEST_TIMEOUT_MS = 25_000;
const GRANT_DIRECTORY = "/run/support-agent-secrets";
const RUNNER_SOCKET = "/run/support-shell/runner.sock";
const PROFILE_ENV_KEY_PATTERN = /^[A-Za-z_][A-Za-z0-9_]{0,79}$/;

function finish(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
  process.exit(0);
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

export function validateShellGrant(grantIdInput, grantDirectoryValue, now = Date.now()) {
  if (!UUID_PATTERN.test(grantIdInput ?? "")) {
    throw new Error("invalid_arguments");
  }
  const grantId = grantIdInput.toLowerCase();
  const grantDirectory = resolve(grantDirectoryValue);
  const grantPath = resolve(join(grantDirectory, `${grantId}.json`));
  if (dirname(grantPath) !== grantDirectory) {
    throw new Error("not_allowed");
  }
  const grantContent = readSafeFile(grantPath, MAX_GRANT_BYTES);
  if (grantContent === undefined) {
    throw new Error("not_allowed");
  }
  let grant;
  try {
    grant = JSON.parse(grantContent);
  } catch {
    throw new Error("not_allowed");
  }
  if (
    grant === null
    || typeof grant !== "object"
    || Array.isArray(grant)
    || grant.version !== 4
    || !Number.isSafeInteger(grant.expires_at_unix_ms)
    || grant.expires_at_unix_ms <= now
    || grant.shell !== true
  ) {
    throw new Error("not_allowed");
  }
  return {
    grantId,
    expiresAtUnixMs: grant.expires_at_unix_ms,
    environment: buildShellEnvironment(grant.variables),
  };
}

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function buildShellEnvironment(variablesValue) {
  if (variablesValue !== undefined && !isPlainObject(variablesValue)) {
    throw new Error("not_allowed");
  }
  const environment = {};
  for (const [key, value] of Object.entries(variablesValue ?? {})) {
    if (!PROFILE_ENV_KEY_PATTERN.test(key)) continue;
    if (
      typeof value !== "string"
      || value.includes("\0")
      || Buffer.byteLength(value, "utf8") > 16 * 1024
    ) {
      throw new Error("not_allowed");
    }
    environment[`SUPPORT_VAR_${key.toUpperCase()}`] = value;
  }
  if (Object.keys(environment).length > 32) {
    throw new Error("not_allowed");
  }
  return environment;
}

export function validateShellCommand(arguments_) {
  if (arguments_.length !== 1) {
    throw new Error("invalid_arguments");
  }
  const command = arguments_[0];
  if (
    typeof command !== "string"
    || command.trim() === ""
    || command.includes("\0")
    || Buffer.byteLength(command, "utf8") > MAX_COMMAND_BYTES
  ) {
    throw new Error("invalid_command");
  }
  return command;
}

export function requestShellRun(socketPath, request) {
  return new Promise((resolveRequest, rejectRequest) => {
    const socket = createConnection(socketPath);
    let settled = false;
    const timeout = setTimeout(() => {
      if (settled) return;
      settled = true;
      socket.destroy();
      rejectRequest(new Error("runner_timeout"));
    }, REQUEST_TIMEOUT_MS);
    let response = Buffer.alloc(0);
    const finishRequest = (callback) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      callback();
    };

    socket.once("connect", () => {
      socket.end(`${JSON.stringify(request)}\n`);
    });
    socket.on("data", (chunk) => {
      if (chunk.length > MAX_RESPONSE_BYTES - response.length) {
        socket.destroy();
        finishRequest(() => rejectRequest(new Error("runner_response_too_large")));
        return;
      }
      response = Buffer.concat([response, chunk], response.length + chunk.length);
    });
    socket.once("error", () => {
      finishRequest(() => rejectRequest(new Error("runner_unavailable")));
    });
    socket.once("close", () => {
      if (response.length === 0) {
        finishRequest(() => rejectRequest(new Error("runner_unavailable")));
        return;
      }
      let payload;
      try {
        payload = JSON.parse(response.toString("utf8"));
      } catch {
        finishRequest(() => rejectRequest(new Error("invalid_runner_response")));
        return;
      }
      finishRequest(() => resolveRequest(payload));
    });
  });
}

async function main() {
  let grantId;
  let expiresAtUnixMs;
  let environment;
  let command;
  try {
    const [grantIdInput, ...commandArguments] = process.argv.slice(2);
    if (await interceptTestTool("shell", grantIdInput, commandArguments, (args) => ({command: validateShellCommand(args)}))) return;
    ({ grantId, expiresAtUnixMs, environment } = validateShellGrant(
      grantIdInput,
      GRANT_DIRECTORY,
    ));
    command = validateShellCommand(commandArguments);
  } catch (error) {
    finish({ ok: false, error: error.message });
  }

  try {
    const result = await requestShellRun(RUNNER_SOCKET, {
      version: 1,
      grant_id: grantId,
      expires_at_unix_ms: expiresAtUnixMs,
      command,
      environment,
    });
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch (error) {
    finish({ ok: false, error: error.message || "runner_unavailable" });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
