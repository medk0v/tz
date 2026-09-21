// Test grants never expose general live credentials or enable native side effects.
// Optional proxy discovery credentials are consumed only by the browser broker.
// Calls still use the production CLI and the production integration broker envelope.
import { constants, openSync, closeSync, fstatSync, readFileSync } from "node:fs";
import { runIntegration } from "./support-integration.mjs";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const MAX_BROWSER_REPORT_BYTES = 1024 * 1024;

export function readTestGrant(id, directory = "/run/support-agent-secrets") {
  if (!UUID.test(id ?? "")) throw new Error("invalid_grant");
  let fd;
  try {
    fd = openSync(`${directory}/${id.toLowerCase()}.json`, constants.O_RDONLY | constants.O_NOFOLLOW);
    const stat = fstatSync(fd);
    if (!stat.isFile() || (stat.mode & 0o022) !== 0 || stat.size > 3 * 1024 * 1024) throw new Error("invalid_grant");
    const grant = JSON.parse(readFileSync(fd,"utf8"));
    if (grant.test_execution !== true) return null;
    if (grant.version !== 4 || !Number.isSafeInteger(grant.expires_at_unix_ms) || grant.expires_at_unix_ms <= Date.now()) throw new Error("invalid_grant");
    return grant;
  } finally { if (fd !== undefined) closeSync(fd); }
}

export async function interceptTestTool(tool, id, args, normalize, options = {}) {
  let grant;
  try { grant = readTestGrant(id, options.grantDirectory); }
  catch { return false; } // Preserve the ordinary wrapper's stable validation errors.
  if (!grant) return false;
  let output;
  try {
    const parameters = normalize(args, {
      allowedHosts: grant.http_allowed_hosts ?? [],
      allowGet: grant.test_capabilities?.http_get === true,
      allowPost: grant.test_capabilities?.http_post === true,
      actionId: grant.reminder_action_id,
      variables: grant.variables ?? {},
      secrets: {},
    });
    // Hex keeps this private transport inside the same strict parameter alphabet.
    const encoded = Buffer.from(JSON.stringify({ tool, parameters })).toString("hex");
    if (encoded.length > 16_000) throw new Error("invalid_arguments");
    const payload = await runIntegration([id,"tz_test","dispatch",JSON.stringify({payload:encoded})], options);
    if (payload.ok === true && payload.status_code === 202) {
      const authorization = JSON.parse(payload.body);
      if (tool !== "browser" || typeof options.liveBrowser !== "function"
          || payload.content_type !== "application/json"
          || !authorization || Object.keys(authorization).length !== 2
          || !UUID.test(authorization.token ?? "") || authorization.url !== parameters.url) {
        throw new Error("invalid_browser_authorization");
      }
      let response;
      try { response = await options.liveBrowser(authorization.url); }
      catch { response = { ok: false, error: "browser_unavailable" }; }
      let report;
      try {
        report = JSON.stringify({ token: authorization.token, response });
        if (Buffer.byteLength(report) > MAX_BROWSER_REPORT_BYTES) throw new Error("page_limit_exceeded");
      } catch {
        report = JSON.stringify({ token: authorization.token, response: { ok: false, error: "page_limit_exceeded" } });
      }
      const acknowledgement = await runIntegration([id, "tz_test", "browser_result",
        JSON.stringify({ payload: Buffer.from(report).toString("hex") })], options);
      if (acknowledgement.ok !== true || acknowledgement.status_code !== 200
          || acknowledgement.content_type !== "text/plain") throw new Error("browser_report_unavailable");
      output = acknowledgement.body;
    } else {
      output = payload.ok === true ? payload.body : JSON.stringify(payload);
    }
  } catch {
    output = JSON.stringify({ok:false,error:"test_tool_invalid_or_unavailable"});
  }
  (options.write ?? ((value) => process.stdout.write(value)))(output.endsWith("\n") ? output : `${output}\n`);
  return true;
}

export function normalizeTestAction(tool, args) {
  if (tool === "notify_operator" && args.length === 2 && args[0] === "--summary") {
    const summary = args[1];
    if (typeof summary !== "string" || !summary.trim() || summary.length > 2500 || summary.includes("\0")) throw new Error("invalid_arguments");
    return {summary: summary.trim()};
  }
  if (tool === "schedule_reminder") {
    if (args.length !== 1 || !/^\d+$/.test(args[0]) || Number(args[0])<10 || Number(args[0])>82800) throw new Error("invalid_arguments");
    return {delay_seconds:Number(args[0])};
  }
  if (args.length !== 0) throw new Error("invalid_arguments");
  return {};
}

export function httpTestParameters(request) {
  return {method:request.method,url:request.url,...(request.body === undefined ? {} : {body:request.body})};
}
