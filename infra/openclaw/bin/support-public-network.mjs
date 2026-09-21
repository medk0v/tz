import { lookup } from "node:dns/promises";
import { BlockList, isIP } from "node:net";
import { request as httpsRequest } from "node:https";
import { Readable } from "node:stream";

const privateAddresses = new BlockList();
for (const [address, prefix] of [
  ["0.0.0.0", 8], ["10.0.0.0", 8], ["100.64.0.0", 10], ["127.0.0.0", 8],
  ["169.254.0.0", 16], ["172.16.0.0", 12], ["192.0.0.0", 24],
  ["192.0.2.0", 24], ["192.88.99.0", 24], ["192.168.0.0", 16],
  ["198.18.0.0", 15], ["198.51.100.0", 24], ["203.0.113.0", 24],
  ["224.0.0.0", 4], ["240.0.0.0", 4],
]) privateAddresses.addSubnet(address, prefix, "ipv4");
for (const [address, prefix] of [
  ["2001::", 23], ["2001:db8::", 32], ["2002::", 16], ["3fff::", 20],
]) privateAddresses.addSubnet(address, prefix, "ipv6");
const globalIpv6 = new BlockList();
globalIpv6.addSubnet("2000::", 3, "ipv6");

export function isPublicAddress(address) {
  const family = isIP(address);
  return family === 4
    ? !privateAddresses.check(address, "ipv4")
    : family === 6 && globalIpv6.check(address, "ipv6")
      && !privateAddresses.check(address, "ipv6");
}

export function publicHttpsUrl(input) {
  const url = new URL(input);
  if (url.protocol !== "https:" || url.username || url.password || url.port || url.hash
      || url.hostname.length > 253 || !url.hostname.includes(".") || isIP(url.hostname)
      || !url.hostname.split(".").every((label) => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/i.test(label))) throw new Error("url_not_allowed");
  return url;
}

export async function resolvePublicHttpsTarget(input, lookupImplementation = lookup) {
  const url = publicHttpsUrl(input);
  let timer;
  const records = await Promise.race([
    lookupImplementation(url.hostname, { all: true }),
    new Promise((_, reject) => { timer = setTimeout(() => reject(new Error("dns_timeout")), 5000); }),
  ]).finally(() => clearTimeout(timer));
  if (records.length === 0 || records.some(({ address }) => !isPublicAddress(address))) {
    throw new Error("address_not_allowed");
  }
  return { hostname: url.hostname, address: records[0].address, family: isIP(records[0].address) };
}

export async function fetchPublicHttps(input, options, lookupImplementation = lookup, requestImplementation = httpsRequest) {
  const response = await fetchPublicHttpsResponse(input, options, lookupImplementation, requestImplementation);
  if (!response.ok) {
    await response.body?.cancel();
    throw new Error("request_failed");
  }
  return response;
}

// Unlike the API wrapper, a browser must see redirects and HTTP error pages.
// This function never follows redirects; each destination needs its own check.
export async function fetchPublicHttpsResponse(input, options, lookupImplementation = lookup, requestImplementation = httpsRequest) {
  const url = publicHttpsUrl(input);
  const target = await resolvePublicHttpsTarget(url.href, lookupImplementation);
  return new Promise((resolve, reject) => {
    const request = requestImplementation(url, {
      method: options.method,
      headers: options.headers,
      signal: options.signal,
      agent: false,
      // Keep the original hostname for TLS and HTTP while pinning the validated address.
      lookup: (_hostname, settings, callback) => {
        if (settings.all) callback(null, [{ address: target.address, family: target.family }]);
        else callback(null, target.address, target.family);
      },
    }, (incoming) => {
      const status = incoming.statusCode;
      if (!status || status < 200 || status > 599) {
        incoming.destroy();
        reject(new Error("request_failed"));
        return;
      }
      const headers = new Headers();
      for (let index = 0; index < incoming.rawHeaders.length; index += 2) {
        headers.append(incoming.rawHeaders[index], incoming.rawHeaders[index + 1]);
      }
      const body = [204, 205, 304].includes(status) ? null : Readable.toWeb(incoming);
      if (body === null) incoming.resume();
      resolve(new Response(body, { status, headers }));
    });
    request.on("error", () => reject(new Error("request_failed")));
    request.end(options.body);
  });
}
