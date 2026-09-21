import assert from "node:assert/strict";
import { test } from "node:test";
import { curlArguments, isPublicAddress, parseCurlRequest, readCurlResponse, resolveCurlTarget } from "./support-curl.mjs";

const permissions = { allowedHosts: ["*"], allowGet: true, allowPost: true, actionId: "50f675a8-43bc-4dbd-89ad-7b522419a1e1" };
const orderUrl = "https://api.example.com/v2/items/bd50eadf-dce3-46f7-873f-61530782ab4a";

test("accepts GET, URL-encoded query parameters and JSON POST curl arguments", () => {
  assert.equal(parseCurlRequest([
    "--silent", "--show-error", "--fail-with-body", "--request", "GET", orderUrl,
  ], permissions).url, orderUrl);
  const rate = parseCurlRequest([
    "--get", "https://api.example.com/v2/catalog", "--data-urlencode", "deposit_method=Bank & cash",
  ], permissions);
  assert.equal(new URL(rate.url).searchParams.get("deposit_method"), "Bank & cash");
  assert.equal(new URL(rate.url).searchParams.size, 1);
  assert.equal(parseCurlRequest([
    "--request", "POST", "https://api.example.com/v2/search",
    "--header", "Content-Type: application/json", "--data", '{"amount":10}',
  ], permissions).body, '{"amount":10}');
});

test("uses the agent domain policy for arbitrary GET and POST paths", () => {
  for (const args of [[orderUrl], ["-X", "POST", "https://api.example.com/v2/tickets?notify=true", "-d", "{}"]]) {
    assert.doesNotThrow(() => parseCurlRequest(args, { ...permissions, allowedHosts: ["api.example.com"] }));
    for (const allowedHosts of [undefined, [], ["support.example.net"]]) {
      assert.throws(() => parseCurlRequest(args, { ...permissions, allowedHosts }));
    }
  }
});

test("permits public explorer hostnames for any network while retaining method grants", () => {
  for (const host of ["etherscan.io", "bscscan.com", "apilist.tronscanapi.com", "mempool.space", "solscan.io", "tonviewer.com"]) {
    assert.equal(parseCurlRequest([`https://${host}/transaction/123`], permissions).method, "GET");
  }
  assert.throws(() => parseCurlRequest([orderUrl], { ...permissions, allowGet: false }));
  assert.throws(() => parseCurlRequest([
    "-X", "POST", "https://api.example.com/v2/search", "-d", "{}",
  ], { ...permissions, allowPost: false }));
});

test("rejects unsafe curl flags, files, credentials, methods and multiple targets before networking", () => {
  for (const args of [
    ["-L", orderUrl], ["--location", orderUrl], ["--insecure", orderUrl],
    ["--config", "/tmp/config"], ["-o", "/tmp/output", orderUrl],
    ["--proxy", "http://localhost", orderUrl], ["--resolve", "api.example.com:443:127.0.0.1", orderUrl],
    ["--upload-file", "/run/support-agent-secrets/a.json", orderUrl],
    ["--data", "@/etc/passwd", orderUrl], ["--get", orderUrl, "--data-urlencode", "name@/etc/passwd"],
    [orderUrl, "https://etherscan.io"], ["-X", "DELETE", orderUrl],
    ["-H", "Authorization: Bearer secret", orderUrl], ["-H", "Host: localhost", orderUrl],
    ["-X", "POST", "https://api.example.com/v2/search", "-d", "[]"],
    ["file:///etc/passwd"], ["http://api.example.com"], ["https://user:pass@api.example.com"],
    ["https://api.example.com:444"], ["https://api.example.com/#fragment"],
    ["https://127.0.0.1"], ["https://[::1]"], ["https://localhost"],
  ]) assert.throws(() => parseCurlRequest(args, permissions), JSON.stringify(args));
});

test("rejects non-public IPv4 and IPv6 destinations including mapped and transition addresses", () => {
  for (const address of [
    "127.0.0.1", "10.0.0.1", "172.16.1.1", "192.168.1.1", "169.254.169.254", "100.100.100.200",
    "0.0.0.0", "192.0.0.1", "198.18.0.1", "224.0.0.1", "255.255.255.255", "192.0.2.1",
    "::", "::1", "::ffff:127.0.0.1", "fc00::1", "fe80::1", "ff02::1", "64:ff9b::7f00:1",
    "2001::1", "2001:db8::1", "2002:7f00:1::", "3fff::1", "not-an-ip",
  ]) assert.equal(isPublicAddress(address), false, address);
  for (const address of ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111", "2001:4860:4860::8888"]) {
    assert.equal(isPublicAddress(address), true, address);
  }
});

test("pins an approved DNS result and rejects private or mixed DNS answers", async () => {
  assert.equal(await resolveCurlTarget(orderUrl, async () => [{ address: "1.1.1.1", family: 4 }]), "api.example.com:443:1.1.1.1");
  assert.equal(await resolveCurlTarget(orderUrl, async () => [{ address: "2606:4700::1111", family: 6 }]), "api.example.com:443:[2606:4700::1111]");
  for (const records of [[], [{ address: "127.0.0.1" }], [{ address: "1.1.1.1" }, { address: "10.0.0.1" }]]) {
    await assert.rejects(() => resolveCurlTarget(orderUrl, async () => records));
  }
  const args = curlArguments(parseCurlRequest([orderUrl], permissions), "api.example.com:443:1.1.1.1");
  assert.equal(args[0], "--disable");
  assert.equal(args[args.indexOf("--resolve") + 1], "api.example.com:443:1.1.1.1");
  assert.equal(args.includes("--location"), false);
  assert.equal(args.includes("--insecure"), false);
  assert.equal(args[args.indexOf("--noproxy") + 1], "*");
});

test("validates actual HTTP status, textual content type, and bounded output", () => {
  assert.equal(readCurlResponse('{"state":"finished"}\nSUPPORT_CURL_RESPONSE_META:200 application/json'), '{"state":"finished"}');
  assert.equal(readCurlResponse("<html>transaction</html>\nSUPPORT_CURL_RESPONSE_META:200 text/html; charset=utf-8"), "<html>transaction</html>");
  for (const response of [
    "body", "body\nSUPPORT_CURL_RESPONSE_META:302 text/html", "body\nSUPPORT_CURL_RESPONSE_META:404 application/json",
    "body\nSUPPORT_CURL_RESPONSE_META:200 application/octet-stream",
    `${"x".repeat(1024 * 1024 + 1)}\nSUPPORT_CURL_RESPONSE_META:200 text/plain`,
  ]) assert.throws(() => readCurlResponse(response));
});
