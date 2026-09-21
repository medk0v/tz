// Proxy discovery and CONNECT run only in the Gateway, never inside Chromium.
import { lookup } from "node:dns/promises";
import { isIP } from "node:net";
import { request as httpRequest } from "node:http";
import { Agent, request as httpsRequest } from "node:https";
import { connect as tlsConnect } from "node:tls";
import { fetchPublicHttpsResponse, isPublicAddress, publicHttpsUrl, resolvePublicHttpsTarget } from "./support-public-network.mjs";

export function validateProxySettings(value) {
  if (value === undefined || value === null) return null;
  if (typeof value !== "object" || Array.isArray(value)) throw new Error("proxy_configuration_invalid");
  const url = publicHttpsUrl(value.service_url);
  if (url.search || typeof value.token !== "string" || !/^[\x21-\x7e]{1,4096}$/.test(value.token)) throw new Error("proxy_configuration_invalid");
  const region = value.region ?? null;
  const country = value.country ?? null;
  if ((region !== null && (typeof region !== "string" || !/^[a-z][a-z0-9_-]{0,63}$/.test(region)))
      || (country !== null && (typeof country !== "string" || !/^[A-Z]{2}$/.test(country)))
      || (region && country)) throw new Error("proxy_configuration_invalid");
  return { service_url: url.href, token: value.token, region, country };
}

export function parseProxy(value) {
  if (!value || typeof value !== "object" || !["http", "https"].includes(value.protocol)
      || typeof value.host !== "string" || value.host.length > 253
      || !Number.isInteger(value.port) || value.port < 1 || value.port > 65535) throw new Error("proxy_response_invalid");
  const host = value.host.toLowerCase();
  if (isIP(host) ? !isPublicAddress(host) : (!host.includes('.') || !host.split('.').every(label => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label)))) throw new Error("proxy_response_invalid");
  const username = value.username ?? "";
  const password = value.password ?? "";
  if ([username, password].some(v => typeof v !== "string" || v.length > 1024 || /[\x00-\x1f\x7f]/.test(v))
      || username.includes(":")) throw new Error("proxy_response_invalid");
  // Ignore the redundant URL: only validated structured fields select the peer.
  return { protocol: value.protocol, host, port: value.port, username, password };
}

async function discover(settings, signal, fetchImplementation, lookupImplementation) {
  const url = new URL(settings.service_url);
  if (settings.region) url.searchParams.set("region", settings.region);
  if (settings.country) url.searchParams.set("country", settings.country);
  if (!settings.region && !settings.country) url.searchParams.set("country", "unknown");
  const response = await fetchImplementation(url.href, {
    method: "GET", headers: { authorization: `Bearer ${settings.token}`, accept: "application/json", "accept-encoding": "identity" },
    signal: AbortSignal.any([signal, AbortSignal.timeout(5000)]),
  });
  const reader = response.body?.getReader();
  const chunks = [];
  let length = 0;
  try {
    // Never follow redirects with the discovery credential.
    if (response.status !== 200 || !reader) throw new Error("proxy_discovery_failed");
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      length += value.byteLength;
      if (length > 16 * 1024) throw new Error("proxy_response_invalid");
      chunks.push(value);
    }
  } finally { await reader?.cancel().catch(() => {}); }
  const proxy = parseProxy(JSON.parse(Buffer.concat(chunks).toString("utf8")));
  let timer;
  const records = isIP(proxy.host) ? [{ address: proxy.host }] : await Promise.race([
    lookupImplementation(proxy.host, { all: true }),
    new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("proxy_dns_timeout")), 5000); }),
  ]).finally(() => clearTimeout(timer));
  if (!records.length || records.some(r => !isPublicAddress(r.address))) throw new Error("proxy_address_not_allowed");
  return { ...proxy, address: records[0].address, family: isIP(records[0].address) };
}

export function connectTunnel(proxy, target, signal, dependencies = {}) {
  const request = proxy.protocol === "https" ? (dependencies.httpsRequest ?? httpsRequest) : (dependencies.httpRequest ?? httpRequest);
  const secureConnect = dependencies.tlsConnect ?? tlsConnect;
  return new Promise((resolve, reject) => {
    const authority = `${target.family === 6 ? `[${target.address}]` : target.address}:443`;
    const headers = { host: authority };
    if (proxy.username || proxy.password) headers["proxy-authorization"] = `Basic ${Buffer.from(`${proxy.username}:${proxy.password}`).toString("base64")}`;
    const connection = request({
      hostname: proxy.host, port: proxy.port, method: "CONNECT", path: authority,
      headers, signal, agent: false, maxHeaderSize: 16384,
      lookup: (_host, options, callback) => options.all
        ? callback(null, [{ address: proxy.address, family: proxy.family }])
        : callback(null, proxy.address, proxy.family),
    });
    connection.on("error", () => reject(new Error("proxy_connect_failed")));
    connection.once("response", response => { response.destroy(); reject(new Error("proxy_connect_failed")); });
    connection.once("connect", (response, socket, head) => {
      if (response.statusCode !== 200 || head.length || signal.aborted) {
        socket.destroy(); reject(new Error("proxy_connect_failed")); return;
      }
      const secure = secureConnect({ socket, servername: target.hostname, rejectUnauthorized: true, ALPNProtocols: ["http/1.1"] });
      const abort = () => secure.destroy(new Error("proxy_request_aborted"));
      signal.addEventListener("abort", abort, { once: true });
      secure.once("close", () => signal.removeEventListener("abort", abort));
      secure.once("error", () => reject(new Error("proxy_tls_failed")));
      secure.once("secureConnect", () => resolve(secure));
      if (signal.aborted) abort();
    });
    connection.end();
  });
}

export function createProxyFetch(value, options = {}) {
  const settings = validateProxySettings(value);
  if (!settings) throw new Error("proxy_configuration_invalid");
  const discoveryFetch = options.discoveryFetch ?? fetchPublicHttpsResponse;
  const lookupImplementation = options.lookup ?? lookup;
  let selected;
  return async (input, requestOptions) => {
    try {
      const target = await resolvePublicHttpsTarget(input, lookupImplementation);
      // One proxy for the entire page, including concurrent resources and redirects.
      selected ??= discover(settings, options.signal ?? requestOptions.signal, discoveryFetch, lookupImplementation);
      const proxy = await selected;
      const agent = new Agent({ keepAlive: false, maxSockets: 1 });
      agent.createConnection = (_connectionOptions, callback) => {
        connectTunnel(proxy, target, requestOptions.signal, options).then(socket => callback(null, socket), callback);
      };
      try {
        return await fetchPublicHttpsResponse(input, requestOptions,
          async () => [{ address: target.address }],
          (url, httpOptions, onResponse) => {
            const req = (options.targetRequest ?? httpsRequest)(url, { ...httpOptions, agent }, onResponse);
            req.once("close", () => agent.destroy());
            return req;
          });
      } catch (error) { agent.destroy(); throw error; }
    } catch {
      // Neither peer credentials nor upstream error bodies belong in agent output.
      throw new Error("proxy_unavailable");
    }
  };
}
