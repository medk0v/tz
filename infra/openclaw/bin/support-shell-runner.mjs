#!/usr/bin/env node

import { randomUUID } from "node:crypto";
import {
  chmodSync,
  chownSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  unlinkSync,
} from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { spawn } from "node:child_process";

const SOCKET_PATH = "/run/support-shell/runner.sock";
const WORK_ROOT = "/tmp/support-shell-runs";
const SANDBOX_UID = 65_534;
const SANDBOX_GID = 65_534;
const MAX_REQUEST_BYTES = 640 * 1024;
const MAX_COMMAND_BYTES = 16 * 1024;
const MAX_OUTPUT_BYTES = 1024 * 1024;
const COMMAND_TIMEOUT_MS = 20_000;
const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const ENVIRONMENT_KEY_PATTERN = /^SUPPORT_VAR_[A-Z_][A-Z0-9_]{0,79}$/;

function errorPayload(error) {
  return { ok: false, error };
}

export function parseShellRequest(payload, now = Date.now()) {
  let request;
  try {
    request = JSON.parse(payload);
  } catch {
    throw new Error("invalid_request");
  }
  if (
    request === null
    || typeof request !== "object"
    || Array.isArray(request)
    || Object.keys(request).sort().join(",")
      !== "command,environment,expires_at_unix_ms,grant_id,version"
    || request.version !== 1
    || typeof request.grant_id !== "string"
    || !UUID_PATTERN.test(request.grant_id)
    || !Number.isSafeInteger(request.expires_at_unix_ms)
    || request.expires_at_unix_ms <= now
    || typeof request.command !== "string"
    || request.command.trim() === ""
    || request.command.includes("\0")
    || Buffer.byteLength(request.command, "utf8") > MAX_COMMAND_BYTES
    || request.environment === null
    || typeof request.environment !== "object"
    || Array.isArray(request.environment)
    || Object.keys(request.environment).length > 32
    || Object.entries(request.environment).some(([key, value]) => (
      !ENVIRONMENT_KEY_PATTERN.test(key)
      || typeof value !== "string"
      || value.includes("\0")
      || Buffer.byteLength(value, "utf8") > 16 * 1024
    ))
  ) {
    throw new Error("invalid_request");
  }
  return request;
}

export function validateRunnerGrantGid(value) {
  const grantGid = Number.parseInt(value ?? "", 10);
  if (
    !Number.isSafeInteger(grantGid)
    || String(grantGid) !== value
    || grantGid < 0
    || grantGid === SANDBOX_GID
  ) {
    throw new Error("OPENCLAW_GRANT_GID is invalid");
  }
  return grantGid;
}

function terminateProcess(child) {
  if (child.pid === undefined) return;
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch {
    try {
      child.kill("SIGKILL");
    } catch {
      // The process already exited.
    }
  }
}

function sandboxProcessIds() {
  const processIds = [];
  for (const entry of readdirSync("/proc")) {
    if (!/^\d+$/.test(entry)) continue;
    try {
      const status = readFileSync(`/proc/${entry}/status`, "utf8");
      const uidLine = status.split("\n").find((line) => line.startsWith("Uid:"));
      const realUid = Number.parseInt(uidLine?.split(/\s+/)[1] ?? "", 10);
      if (realUid === SANDBOX_UID) processIds.push(Number.parseInt(entry, 10));
    } catch {
      // The process may have exited between listing and inspection.
    }
  }
  return processIds;
}

async function terminateSandboxProcesses() {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    const processIds = sandboxProcessIds();
    if (processIds.length === 0) return true;
    for (const processId of processIds) {
      try {
        process.kill(processId, "SIGKILL");
      } catch {
        // A concurrent exit is equivalent to successful cleanup.
      }
    }
    await new Promise((resolveDelay) => setTimeout(resolveDelay, 5));
  }
  return sandboxProcessIds().length === 0;
}

function restartAfterUnsafeCleanup() {
  process.exit(1);
}

export async function executeShellCommand(command, environment) {
  if (!(await terminateSandboxProcesses())) {
    restartAfterUnsafeCleanup();
    return errorPayload("sandbox_cleanup_failed");
  }
  return new Promise((resolveCommand) => {
    const workspace = join(WORK_ROOT, randomUUID());
    mkdirSync(workspace, { mode: 0o700 });
    chownSync(workspace, SANDBOX_UID, SANDBOX_GID);
    const child = spawn("/bin/sh", ["-lc", command], {
      cwd: workspace,
      detached: true,
      env: {
        HOME: workspace,
        LANG: "C.UTF-8",
        PATH: "/usr/local/bin:/usr/bin:/bin",
        TMPDIR: workspace,
        ...environment,
      },
      gid: SANDBOX_GID,
      uid: SANDBOX_UID,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = Buffer.alloc(0);
    let stderr = Buffer.alloc(0);
    let outputTooLarge = false;
    let timedOut = false;
    let finalized = false;

    const append = (current, chunk) => {
      const remaining = MAX_OUTPUT_BYTES - stdout.length - stderr.length;
      if (chunk.length > remaining) {
        outputTooLarge = true;
        terminateProcess(child);
        void terminateSandboxProcesses().then((cleaned) => {
          if (!cleaned) restartAfterUnsafeCleanup();
        });
        return current;
      }
      return Buffer.concat([current, chunk], current.length + chunk.length);
    };
    child.stdout.on("data", (chunk) => {
      stdout = append(stdout, chunk);
    });
    child.stderr.on("data", (chunk) => {
      stderr = append(stderr, chunk);
    });
    const timeout = setTimeout(() => {
      timedOut = true;
      terminateProcess(child);
      void terminateSandboxProcesses().then((cleaned) => {
        if (!cleaned) restartAfterUnsafeCleanup();
      });
    }, COMMAND_TIMEOUT_MS);

    const finalize = async (result) => {
      if (finalized) return;
      finalized = true;
      clearTimeout(timeout);
      const cleaned = await terminateSandboxProcesses();
      rmSync(workspace, { recursive: true, force: true });
      if (!cleaned) {
        resolveCommand(errorPayload("sandbox_cleanup_failed"));
        restartAfterUnsafeCleanup();
        return;
      }
      resolveCommand(result);
    };
    child.once("error", () => {
      void finalize(errorPayload("command_unavailable"));
    });
    child.once("close", (code, signal) => {
      if (outputTooLarge) {
        void finalize(errorPayload("output_too_large"));
        return;
      }
      if (timedOut) {
        void finalize(errorPayload("command_timeout"));
        return;
      }
      void finalize({
        ok: true,
        exit_code: Number.isInteger(code) ? code : null,
        signal: signal ?? null,
        stdout: stdout.toString("utf8"),
        stderr: stderr.toString("utf8"),
      });
    });
  });
}

const queue = [];
let running = false;

async function drainQueue() {
  if (running) return;
  const next = queue.shift();
  if (!next) return;
  running = true;
  try {
    if (next.expiresAtUnixMs <= Date.now()) {
      next.resolve(errorPayload("not_allowed"));
    } else {
      next.resolve(await executeShellCommand(next.command, next.environment));
    }
  } finally {
    running = false;
    void drainQueue();
  }
}

function enqueueCommand(command, environment, expiresAtUnixMs) {
  if (queue.length >= 8) {
    return Promise.resolve(errorPayload("runner_busy"));
  }
  return new Promise((resolveCommand) => {
    queue.push({ command, environment, expiresAtUnixMs, resolve: resolveCommand });
    void drainQueue();
  });
}

function prepareSocket() {
  mkdirSync(WORK_ROOT, { mode: 0o711, recursive: true });
  chmodSync(WORK_ROOT, 0o711);
  try {
    const existing = lstatSync(SOCKET_PATH);
    if (!existing.isSocket()) {
      throw new Error("runner socket path is not a socket");
    }
    unlinkSync(SOCKET_PATH);
  } catch (error) {
    if (error?.code !== "ENOENT") throw error;
  }
}

function runServer() {
  prepareSocket();
  const server = createServer((socket) => {
    let request = Buffer.alloc(0);
    let handled = false;
    socket.on("data", (chunk) => {
      if (handled) return;
      if (chunk.length > MAX_REQUEST_BYTES - request.length) {
        handled = true;
        socket.end(`${JSON.stringify(errorPayload("request_too_large"))}\n`);
        return;
      }
      request = Buffer.concat([request, chunk], request.length + chunk.length);
      const newline = request.indexOf(0x0a);
      if (newline === -1) return;
      handled = true;
      let parsed;
      try {
        parsed = parseShellRequest(request.subarray(0, newline).toString("utf8"));
      } catch (error) {
        socket.end(`${JSON.stringify(errorPayload(error.message))}\n`);
        return;
      }
      void enqueueCommand(
        parsed.command,
        parsed.environment,
        parsed.expires_at_unix_ms,
      ).then((result) => {
        socket.end(`${JSON.stringify(result)}\n`);
      });
    });
    socket.on("error", () => {
      socket.destroy();
    });
  });
  server.listen(SOCKET_PATH, () => {
    const grantGid = validateRunnerGrantGid(process.env.OPENCLAW_GRANT_GID);
    chownSync(SOCKET_PATH, 0, grantGid);
    chmodSync(SOCKET_PATH, 0o660);
  });
}

if (process.argv[1]?.endsWith("support-shell-runner.mjs")) {
  runServer();
}
