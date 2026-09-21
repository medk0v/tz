#!/usr/bin/env node

import { createConnection } from "node:net";
import { pathToFileURL } from "node:url";
import { validatePublicHttpGrant, parsePublicHttpRequest } from "./support-public-http.mjs";
import { fetchPublicHttpsResponse } from "./support-public-network.mjs";
import { interceptTestTool, readTestGrant } from "./support-test-tool.mjs";
import { browserChannel } from "./support-browser-protocol.mjs";
import { createProxyFetch, validateProxySettings } from "./support-browser-proxy.mjs";

const GRANT_DIRECTORY = "/run/support-agent-secrets";
const RUNNER_SOCKET = "/run/support-browser/runner.sock";
const MAX_RESOURCE_BYTES = 8 * 1024 * 1024;
const MAX_PAGE_BYTES = 40 * 1024 * 1024;
// Forward Chromium's own navigation/client hints, not agent-supplied credentials.
const REQUEST_HEADERS = new Set([
  "accept", "accept-language", "user-agent", "origin", "referer", "cookie",
  "cache-control", "pragma", "if-none-match", "if-modified-since", "upgrade-insecure-requests",
  "sec-fetch-dest", "sec-fetch-mode", "sec-fetch-site", "sec-fetch-user",
  "sec-ch-ua", "sec-ch-ua-mobile", "sec-ch-ua-platform", "sec-ch-ua-platform-version",
  "sec-ch-ua-arch", "sec-ch-ua-bitness", "sec-ch-ua-model", "sec-ch-ua-full-version",
  "sec-ch-ua-full-version-list", "sec-ch-ua-wow64", "sec-ch-ua-form-factors",
]);
const BROWSER_ERRORS = new Set(["proxy_unavailable", "proxy_configuration_invalid", "invalid_arguments", "invalid_url", "not_allowed", "method_not_allowed", "url_not_allowed", "invalid_host_configuration", "browser_unavailable_in_tests", "browser_timeout", "browser_unavailable", "page_limit_exceeded"]);

export function browserUrl(input, permissions) {
  if (typeof input !== "string" || Buffer.byteLength(input) > 4096) throw new Error("invalid_url");
  const url = new URL(input);
  const hash = url.hash;
  url.hash = ""; // Hash routes, such as TRONSCAN /#/transaction/…, stay in Chromium.
  const request = parsePublicHttpRequest(["GET", url.href], permissions);
  return request.url + hash;
}

export function validateBrowserGrant(id, directory = GRANT_DIRECTORY) {
  let testGrant;
  try { testGrant = readTestGrant(id, directory); }
  catch { throw new Error("not_allowed"); }
  if (testGrant) throw new Error("browser_unavailable_in_tests");
  const grant = validatePublicHttpGrant(id, directory);
  if (!grant.allowGet) throw new Error("method_not_allowed");
  // Only the broker reads the grant. No variables, secrets or IDs reach Chromium.
  return { allowedHosts: grant.allowedHosts, allowGet: true, allowPost: false, browserProxy: validateProxySettings(grant.browserProxy) };
}

export function browserTestParameters(args, permissions) {
  if (args.length !== 1) throw new Error("invalid_arguments");
  return { url: browserUrl(args[0], permissions) };
}

// Called only after the scenario broker returns an explicit live-browser grant.
// Re-read test permissions for every resource so expiry and revocation stay effective.
export async function requestTestBrowserPage(id, urlInput, options = {}) {
  const permissions = () => {
    let grant;
    try { grant = readTestGrant(id, options.grantDirectory); }
    catch { throw new Error("not_allowed"); }
    if (!grant) throw new Error("not_allowed");
    if (grant.test_capabilities?.http_get !== true) throw new Error("method_not_allowed");
    return { allowedHosts: grant.http_allowed_hosts ?? [], allowGet: true, allowPost: false, browserProxy: validateProxySettings(grant.browser_proxy) };
  };
  try {
    const url = browserUrl(urlInput, permissions());
    return await requestBrowserPage(url, permissions, options);
  } catch (error) {
    return browserFailure(error);
  }
}

function browserFailure(error) {
  return { ok: false, error: BROWSER_ERRORS.has(error.message) ? error.message : "browser_unavailable" };
}

export async function browserResource(message, permissions, budget, signal, fetchImplementation = fetchPublicHttpsResponse) {
  if (message.method !== "GET") throw new Error("method_not_allowed");
  const url = new URL(browserUrl(message.url, permissions));
  url.hash = "";
  if (++budget.requests > 200 || budget.bytes >= MAX_PAGE_BYTES) throw new Error("page_limit_exceeded");
  const headers = { "accept-encoding": "identity" };
  for (const [name, value] of Object.entries(message.headers ?? {})) {
    if (REQUEST_HEADERS.has(name.toLowerCase()) && typeof value === "string" && value.length <= 8192) {
      headers[name.toLowerCase()] = value;
    }
  }
  const response = await fetchImplementation(url.href, {
    method: "GET", headers, signal: AbortSignal.any([signal, AbortSignal.timeout(10_000)]),
  });
  const chunks = [];
  let bytes = 0;
  const reader = response.body?.getReader();
  try {
    while (reader) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
      budget.bytes += value.byteLength;
      if (bytes > MAX_RESOURCE_BYTES || budget.bytes > MAX_PAGE_BYTES) throw new Error("page_limit_exceeded");
      chunks.push(value);
    }
  } finally { await reader?.cancel().catch(() => {}); }
  // Chromium, not the broker, follows redirects; their requests are checked again.
  const responseHeaders = Object.fromEntries(response.headers);
  // Set-Cookie is not comma-joinable (Expires itself can contain commas).
  const setCookies = response.headers.getSetCookie();
  delete responseHeaders["set-cookie"];
  delete responseHeaders["transfer-encoding"];
  delete responseHeaders["content-length"];
  return { status: response.status, headers: responseHeaders, setCookies, body: Buffer.concat(chunks).toString("base64") };
}

export function requestBrowserPage(url, permissionProvider, options = {}) {
  return new Promise((resolve, reject) => {
    const proxy = permissionProvider().browserProxy;
    const socket = createConnection(options.socketPath ?? RUNNER_SOCKET);
    const controller = new AbortController();
    const fetchImplementation = options.fetchImplementation ?? (proxy ? createProxyFetch(proxy, { signal: controller.signal }) : undefined);
    const budget = { requests: 0, bytes: 0 };
    let settled = false;
    let active = 0;
    let resourceMessages = 0;
    const finish = (error, result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      controller.abort();
      socket.destroy();
      if (error) reject(new Error(error)); else resolve(result);
    };
    // Includes bounded waiting for a browser slot and its navigation budget.
    const timer = setTimeout(() => finish("browser_timeout"), options.timeoutMs ?? 80_000);
    const send = browserChannel(socket, async (message) => {
      if (message.type === "result") {
        if (message.result?.ok === true) {
          try { browserUrl(message.result.url, permissionProvider()); }
          catch { finish("not_allowed"); return; }
        }
        finish(null, message.result);
        return;
      }
      if (message.type !== "fetch" || !Number.isSafeInteger(message.id)
          || ++resourceMessages > 200 || active >= 8) {
        finish("page_limit_exceeded");
        return;
      }
      active++;
      try {
        const response = await browserResource(message, permissionProvider(), budget, controller.signal, fetchImplementation);
        send({ type: "resource", id: message.id, ...response });
      } catch (error) {
        if (error.message === "proxy_unavailable") { finish("proxy_unavailable"); return; }
        send({ type: "resource", id: message.id, error: "resource_unavailable" });
      } finally { active--; }
    });
    socket.once("connect", () => send({ type: "open", url }));
    socket.once("error", () => finish("browser_unavailable"));
    socket.once("close", () => finish("browser_unavailable"));
  });
}

async function main() {
  try {
    const [id, urlInput, ...extra] = process.argv.slice(2);
    if (extra.length) throw new Error("invalid_arguments");
    if (await interceptTestTool("browser", id, [urlInput], browserTestParameters,
      { liveBrowser: (url) => requestTestBrowserPage(id, url) })) return;
    const permissions = () => validateBrowserGrant(id);
    const url = browserUrl(urlInput, permissions());
    const result = await requestBrowserPage(url, permissions);
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch (error) {
    process.stdout.write(`${JSON.stringify(browserFailure(error))}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await main();
