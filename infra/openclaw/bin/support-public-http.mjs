#!/usr/bin/env node

import { interceptTestTool, httpTestParameters } from "./support-test-tool.mjs";
import { fetchPublicHttps, publicHttpsUrl } from "./support-public-network.mjs";

import {
  closeSync,
  constants,
  fstatSync,
  openSync,
  readFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_GRANT_BYTES = 3 * 1024 * 1024;
const MAX_URL_BYTES = 4 * 1024;
const MAX_REQUEST_BYTES = 64 * 1024;
const MAX_HEADER_BINDINGS_BYTES = 32 * 1024;
const MAX_RESPONSE_BYTES = 1024 * 1024;
const REQUEST_TIMEOUT_MS = 15_000;
const GRANT_DIRECTORY = "/run/support-agent-secrets";
const CREDENTIAL_KEY_PATTERN = /^[A-Za-z0-9_][A-Za-z0-9_.-]{0,79}$/;
const ALLOWED_CREDENTIAL_HEADERS = new Set(["authorization", "x-api-key"]);

function finish(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
  process.exit(0);
}

function allowedHosts(value) {
  if (!Array.isArray(value) || value.length > 64 || value.some((host) => typeof host !== "string")) {
    throw new Error("invalid_host_configuration");
  }
  const hosts = value.map((host) => host.trim().toLowerCase());
  for (const host of hosts) {
    if (host !== "*" && (host.length > 253 || publicHttpsUrl(`https://${host}`).hostname !== host)) {
      throw new Error("invalid_host_configuration");
    }
  }
  return new Set(hosts);
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

function validatedCredentials(value) {
  if (value === undefined) return {};
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("not_allowed");
  }
  const entries = Object.entries(value);
  const normalizedKeys = new Set(entries.map(([key]) => key.toLowerCase()));
  if (
    entries.length > 32
    || normalizedKeys.size !== entries.length
    || entries.some(([key, credential]) => (
      !CREDENTIAL_KEY_PATTERN.test(key)
      || typeof credential !== "string"
      || credential.includes("\0")
    ))
  ) {
    throw new Error("not_allowed");
  }
  return value;
}

export function validatePublicHttpGrant(grantIdInput, grantDirectoryValue, now = Date.now()) {
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
    || (grant.public_http_get !== true && grant.public_http_post !== true)
    || !UUID_PATTERN.test(grant.reminder_action_id ?? "")
  ) {
    throw new Error("not_allowed");
  }
  return {
    grantId,
    actionId: grant.reminder_action_id.toLowerCase(),
    allowedHosts: [...allowedHosts(grant.http_allowed_hosts ?? [])],
    allowGet: grant.public_http_get === true,
    allowPost: grant.public_http_post === true,
    variables: validatedCredentials(grant.variables),
    secrets: validatedCredentials(grant.secrets),
    browserProxy: grant.browser_proxy,
  };
}

function credentialValue(credentials, key) {
  return Object.entries(credentials)
    .find(([candidate]) => candidate.toLowerCase() === key.toLowerCase())?.[1];
}

export function parseHeaderBindings(input, permissions) {
  if (input === undefined) return { headers: {}, secretValues: [] };
  if (Buffer.byteLength(input, "utf8") > MAX_HEADER_BINDINGS_BYTES) {
    throw new Error("invalid_headers");
  }
  let bindings;
  try {
    bindings = JSON.parse(input);
  } catch {
    throw new Error("invalid_headers");
  }
  if (
    bindings === null
    || typeof bindings !== "object"
    || Array.isArray(bindings)
    || Object.keys(bindings).length > 16
  ) {
    throw new Error("invalid_headers");
  }

  const headers = {};
  const secretValues = [];
  for (const [headerName, binding] of Object.entries(bindings)) {
    const normalizedHeader = headerName.toLowerCase();
    if (
      !ALLOWED_CREDENTIAL_HEADERS.has(normalizedHeader)
      || binding === null
      || typeof binding !== "object"
      || Array.isArray(binding)
      || Object.keys(binding).some((key) => !["source", "key", "prefix", "suffix"].includes(key))
      || !["secret", "variable"].includes(binding.source)
      || typeof binding.key !== "string"
      || !CREDENTIAL_KEY_PATTERN.test(binding.key)
      || (binding.prefix !== undefined && typeof binding.prefix !== "string")
      || (binding.suffix !== undefined && typeof binding.suffix !== "string")
    ) {
      throw new Error("invalid_headers");
    }
    const source = binding.source === "secret" ? permissions.secrets : permissions.variables;
    const credential = credentialValue(source ?? {}, binding.key);
    if (credential === undefined) {
      throw new Error("credential_not_available");
    }
    if (credential === "") {
      throw new Error("credential_not_available");
    }
    const value = `${binding.prefix ?? ""}${credential}${binding.suffix ?? ""}`;
    if (
      /[\u0000-\u001f\u007f]/.test(value)
      || Buffer.byteLength(value, "utf8") > 8 * 1024
    ) {
      throw new Error("invalid_headers");
    }
    headers[normalizedHeader] = value;
    if (binding.source === "secret" && credential !== "") {
      secretValues.push(credential);
    }
  }
  return { headers, secretValues: [...new Set(secretValues)] };
}

export function parsePublicHttpRequest(arguments_, permissions) {
  if (arguments_.length < 2 || arguments_.length > 4) {
    throw new Error("invalid_arguments");
  }
  const [method, urlInput, payloadInput, postHeadersInput] = arguments_;
  if (method !== "GET" && method !== "POST") {
    throw new Error("method_not_allowed");
  }
  const jsonInput = method === "POST" ? payloadInput : undefined;
  const headersInput = method === "POST" ? postHeadersInput : payloadInput;
  if (
    (method === "GET" && arguments_.length > 3)
    || (method === "POST" && (arguments_.length < 3 || arguments_.length > 4))
  ) {
    throw new Error("invalid_arguments");
  }
  if (Buffer.byteLength(urlInput, "utf8") > MAX_URL_BYTES) {
    throw new Error("invalid_url");
  }
  let url;
  try { url = publicHttpsUrl(urlInput); }
  catch { throw new Error("url_not_allowed"); }
  const hosts = allowedHosts(permissions.allowedHosts ?? []);
  if (!hosts.has("*") && !hosts.has(url.hostname.toLowerCase())) {
    throw new Error("url_not_allowed");
  }

  const { headers, secretValues } = parseHeaderBindings(headersInput, permissions);
  const credentialScope = Object.keys(headers).length > 0 ? { headers, secretValues } : {};
  if (method === "GET") {
    if (!permissions.allowGet) {
      throw new Error("method_not_allowed");
    }
    return { method, url: url.href, body: undefined, ...credentialScope };
  }
  if (!permissions.allowPost) {
    throw new Error("method_not_allowed");
  }
  if (Buffer.byteLength(jsonInput, "utf8") > MAX_REQUEST_BYTES) {
    throw new Error("invalid_json");
  }
  let body;
  try {
    body = JSON.parse(jsonInput);
  } catch {
    throw new Error("invalid_json");
  }
  if (body === null || typeof body !== "object" || Array.isArray(body)) {
    throw new Error("invalid_json");
  }
  return {
    method,
    url: url.href,
    body: JSON.stringify(body),
    idempotencyKey: permissions.actionId,
    ...credentialScope,
  };
}

export async function performPublicHttpRequest(request, fetchImplementation = fetchPublicHttps) {
  const credentialHeaders = request.headers ?? {};
  const response = await fetchImplementation(request.url, {
    method: request.method,
    body: request.body,
    headers: request.method === "POST"
      ? {
          ...credentialHeaders,
          "content-type": "application/json",
          accept: "application/json, text/plain;q=0.9",
          "idempotency-key": request.idempotencyKey,
        }
      : {
          ...credentialHeaders,
          accept: "application/json, text/plain;q=0.9, text/html;q=0.8",
        },
    redirect: "error",
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
  });
  if (!response.ok || !response.body) {
    throw new Error("request_failed");
  }
  const contentType = response.headers.get("content-type")?.toLowerCase() ?? "";
  if (!(contentType.startsWith("application/json") || contentType.startsWith("text/"))) {
    throw new Error("response_not_text");
  }

  const reader = response.body.getReader();
  const chunks = [];
  let totalBytes = 0;
  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    totalBytes += value.byteLength;
    if (totalBytes > MAX_RESPONSE_BYTES) {
      await reader.cancel();
      throw new Error("response_too_large");
    }
    chunks.push(value);
  }
  return (request.secretValues ?? [])
    .filter((secret) => secret !== "")
    .sort((left, right) => right.length - left.length)
    .reduce(
      (redacted, secret) => redacted.split(secret).join("[REDACTED]"),
      Buffer.concat(chunks, totalBytes).toString("utf8"),
    );
}

async function main() {
  let request;
  try {
    const [grantIdInput, ...requestArguments] = process.argv.slice(2);
    if (await interceptTestTool("http", grantIdInput, requestArguments, (args, permissions) => httpTestParameters(parsePublicHttpRequest(args, permissions)))) return;
    const grant = validatePublicHttpGrant(grantIdInput, GRANT_DIRECTORY);
    request = parsePublicHttpRequest(
      requestArguments,
      grant,
    );
  } catch (error) {
    finish({ ok: false, error: error.message });
  }

  try {
    const responseBody = await performPublicHttpRequest(request);
    process.stdout.write(responseBody);
    if (!responseBody.endsWith("\n")) process.stdout.write("\n");
  } catch (error) {
    finish({ ok: false, error: error.message || "request_failed" });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
