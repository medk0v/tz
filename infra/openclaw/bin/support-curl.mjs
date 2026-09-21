#!/usr/bin/env node

import { interceptTestTool, httpTestParameters } from "./support-test-tool.mjs";

import { spawn } from "node:child_process";
import { publicHttpsUrl, resolvePublicHttpsTarget } from "./support-public-network.mjs";
export { isPublicAddress } from "./support-public-network.mjs";
import { pathToFileURL } from "node:url";
import { parsePublicHttpRequest, validatePublicHttpGrant } from "./support-public-http.mjs";

const GRANT_DIRECTORY = "/run/support-agent-secrets";
const MAX_RESPONSE_BYTES = 1024 * 1024;
const TIMEOUT_MS = 15_000;
const META_SEPARATOR = "\nSUPPORT_CURL_RESPONSE_META:";
export function parseCurlRequest(arguments_, permissions) {
  if (arguments_.length === 0 || arguments_.length > 80
      || arguments_.some((value) => typeof value !== "string" || value.includes("\0"))
      || arguments_.reduce((size, value) => size + Buffer.byteLength(value), 0) > 80 * 1024) {
    throw new Error("invalid_arguments");
  }
  let method;
  let urlInput;
  let body;
  let get = false;
  let contentType = false;
  const query = [];
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index];
    if (["--silent", "-s", "--show-error", "-S", "-sS", "--fail", "-f", "--fail-with-body"].includes(argument)) continue;
    if (argument === "--get" || argument === "-G") {
      if (get) throw new Error("invalid_arguments");
      get = true;
    } else if (["--request", "-X", "--data", "-d", "--header", "-H", "--data-urlencode"].includes(argument)) {
      const value = arguments_[++index];
      if (value === undefined) throw new Error("invalid_arguments");
      if (argument === "--request" || argument === "-X") {
        if (method !== undefined) throw new Error("invalid_arguments");
        method = value;
      } else if (argument === "--data" || argument === "-d") {
        if (body !== undefined || value.startsWith("@")) throw new Error("invalid_arguments");
        body = value;
      } else if (argument === "--header" || argument === "-H") {
        if (contentType || !/^content-type:\s*application\/json$/i.test(value)) {
          throw new Error("header_not_allowed");
        }
        contentType = true;
      } else {
        const separator = value.indexOf("=");
        if (separator < 1 || !/^[A-Za-z0-9_]+$/.test(value.slice(0, separator))) {
          throw new Error("invalid_query");
        }
        query.push([value.slice(0, separator), value.slice(separator + 1)]);
      }
    } else {
      if (argument.startsWith("-") || urlInput !== undefined) throw new Error("argument_not_allowed");
      urlInput = argument;
    }
  }
  method ??= get ? "GET" : body === undefined ? "GET" : "POST";
  if (urlInput === undefined || (get && method !== "GET")
      || (query.length > 0 && !get) || (method === "GET" && (body !== undefined || contentType))) {
    throw new Error("invalid_arguments");
  }
  const url = publicHttpsUrl(urlInput);
  for (const [key, value] of query) url.searchParams.append(key, value);
  const request = parsePublicHttpRequest(
    method === "POST" ? [method, url.href, body ?? ""] : [method, url.href],
    permissions,
  );
  return request;
}

export async function resolveCurlTarget(urlInput, lookupImplementation) {
  const target = await resolvePublicHttpsTarget(urlInput, lookupImplementation);
  return `${target.hostname}:443:${target.family === 6 ? `[${target.address}]` : target.address}`;
}

export function curlArguments(request, pinnedTarget) {
  const arguments_ = [
    "--disable", "--silent", "--show-error", "--fail-with-body", "--globoff",
    "--proto", "=https", "--proto-redir", "=https", "--noproxy", "*",
    "--connect-timeout", "10", "--max-time", "15", "--max-filesize", String(MAX_RESPONSE_BYTES),
    "--resolve", pinnedTarget, "--request", request.method,
    "--header", "Accept: application/json, text/plain;q=0.9, text/html;q=0.8",
    "--write-out", `${META_SEPARATOR}%{http_code} %{content_type}`,
  ];
  if (request.method === "POST") {
    arguments_.push("--header", "Content-Type: application/json", "--header",
      `Idempotency-Key: ${request.idempotencyKey}`, "--data-binary", "@-");
  }
  arguments_.push("--url", request.url);
  return arguments_;
}

export function readCurlResponse(output) {
  const separator = output.lastIndexOf(META_SEPARATOR);
  if (separator < 0) throw new Error("invalid_response");
  const metadata = output.slice(separator + META_SEPARATOR.length);
  if (!/^2\d\d (application\/json|text\/[^\s;]+)(?:;[^\r\n]*)?$/i.test(metadata)) {
    throw new Error("request_failed");
  }
  const body = output.slice(0, separator);
  if (Buffer.byteLength(body) > MAX_RESPONSE_BYTES) throw new Error("response_too_large");
  return body;
}

export async function performCurlRequest(request) {
  let dnsTimer;
  const pinnedTarget = await Promise.race([
    resolveCurlTarget(request.url),
    new Promise((_, reject) => { dnsTimer = setTimeout(() => reject(new Error("dns_timeout")), 5000); }),
  ]).finally(() => clearTimeout(dnsTimer));
  return new Promise((resolve, reject) => {
    // The native binary is never directly allowlisted. No caller flags or environment reach it.
    const child = spawn("/usr/bin/curl", curlArguments(request, pinnedTarget), {
      env: { PATH: "/usr/bin:/bin", LANG: "C" },
      stdio: ["pipe", "pipe", "ignore"],
      timeout: TIMEOUT_MS + 1000,
      killSignal: "SIGKILL",
    });
    const chunks = [];
    let bytes = 0;
    child.stdout.on("data", (chunk) => {
      bytes += chunk.length;
      if (bytes > MAX_RESPONSE_BYTES + 2048) {
        child.kill("SIGKILL");
        reject(new Error("response_too_large"));
      } else chunks.push(chunk);
    });
    child.on("error", () => reject(new Error("request_failed")));
    child.stdin.on("error", () => reject(new Error("request_failed")));
    child.on("close", (code) => {
      if (code !== 0) return reject(new Error("request_failed"));
      try { resolve(readCurlResponse(Buffer.concat(chunks).toString("utf8"))); }
      catch (error) { reject(error); }
    });
    child.stdin.end(request.body);
  });
}

async function main() {
  try {
    const [flag, grantId, ...arguments_] = process.argv.slice(2);
    if (flag !== "--support-grant") throw new Error("current_grant_required");
    if (await interceptTestTool("http", grantId, arguments_, (args, permissions) => httpTestParameters(parseCurlRequest(args, permissions)))) return;
    const permissions = validatePublicHttpGrant(grantId, GRANT_DIRECTORY);
    const request = parseCurlRequest(arguments_, permissions);
    const body = await performCurlRequest(request);
    process.stdout.write(body.endsWith("\n") ? body : `${body}\n`);
  } catch (error) {
    // Parse failures may contain the input URL; expose only fixed error codes.
    const code = /^[a-z_]+$/.test(error.message) ? error.message : "invalid_arguments";
    process.stdout.write(`${JSON.stringify({ ok: false, error: code })}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
