import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { interceptTestTool, normalizeTestAction, httpTestParameters } from "./support-test-tool.mjs";
import { parseCurlRequest } from "./support-curl.mjs";
import { browserTestParameters } from "./support-browser.mjs";

test("test commands use the broker and never create real action markers or use network credentials", async () => {
  const root = mkdtempSync(join(tmpdir(),"support-test-tool-"));
  const grantDirectory=join(root,"grants"), actionDirectory=join(root,"actions");
  mkdirSync(grantDirectory);mkdirSync(actionDirectory);
  const id=randomUUID();
  writeFileSync(join(grantDirectory,`${id}.json`),JSON.stringify({version:4,expires_at_unix_ms:Date.now()+60000,test_execution:true,
    variables:{},secrets:{},public_http_get:false,public_http_post:false,shell:false,resolve_conversation:false,schedule_reminder:false,
    test_capabilities:{http_get:true,http_post:false},http_allowed_hosts:["api.example.com"],integration_lookup:true,
    integrations:[{key:"tz_test",actions:[{key:"dispatch",parameter_names:["payload"]}]}],reminder_action_id:randomUUID()}),{mode:0o640});
  let observed;
  const timer=setInterval(()=>{
    const path=join(actionDirectory,`${id}.integration-request`);
    const response=join(actionDirectory,`${id}.integration-response`);
    if(existsSync(path)&&!existsSync(response)) {
      observed=JSON.parse(Buffer.from(JSON.parse(readFileSync(path,"utf8")).parameters.payload,"hex").toString());
      writeFileSync(response,JSON.stringify({ok:true,status_code:200,content_type:"text/plain",body:'{"ok":true,"requested":true}'}),{mode:0o640});
    }
  },5);
  try {
    let output="";
    const handled=await interceptTestTool("schedule_reminder",id,["300"],(args)=>normalizeTestAction("schedule_reminder",args),{grantDirectory,actionDirectory,write:(text)=>{output+=text;},responseTimeoutMs:1000});
    assert.equal(handled,true);
    assert.deepEqual(observed,{tool:"schedule_reminder",parameters:{delay_seconds:300}});
    assert.equal(JSON.parse(output).ok,true);
    assert.deepEqual(readdirSync(actionDirectory),[]);
    const normalizeHttp = (args, permissions) => httpTestParameters(parseCurlRequest(args, permissions));
    await interceptTestTool("http", id, ["https://api.example.com/custom"], normalizeHttp, {grantDirectory,actionDirectory,write:()=>{},responseTimeoutMs:1000});
    assert.deepEqual(observed, {tool:"http",parameters:{method:"GET",url:"https://api.example.com/custom"}});
    await interceptTestTool("browser", id, ["https://api.example.com/#/transaction/abc"], browserTestParameters, {grantDirectory,actionDirectory,write:()=>{},responseTimeoutMs:1000});
    assert.deepEqual(observed, {tool:"browser",parameters:{url:"https://api.example.com/#/transaction/abc"}});
    output = "";
    await interceptTestTool("http", id, ["https://other.example.org/custom"], normalizeHttp, {grantDirectory,actionDirectory,write:(text)=>{output+=text;},responseTimeoutMs:1000});
    assert.equal(JSON.parse(output).error, "test_tool_invalid_or_unavailable");
    assert.equal(observed.parameters.url, "https://api.example.com/#/transaction/abc");
    assert.deepEqual(readdirSync(actionDirectory),[]);
    const parameters=httpTestParameters(parseCurlRequest(["-sS","https://api.example.com/v2/items/00000000-0000-0000-0000-000000000000"],{allowedHosts:["api.example.com"],allowGet:true,allowPost:false}));
    assert.deepEqual(parameters,{method:"GET",url:"https://api.example.com/v2/items/00000000-0000-0000-0000-000000000000"});
    assert.throws(()=>normalizeTestAction("schedule_reminder",["300","extra"]));
    assert.throws(()=>normalizeTestAction("notify_operator",["attacker chat"]));
    await interceptTestTool("notify_operator", id, ["--summary", "Orders TEST-101 and TEST-102: change SBP phone"], (args) => normalizeTestAction("notify_operator", args), {grantDirectory,actionDirectory,write:()=>{},responseTimeoutMs:1000});
    assert.deepEqual(observed, {tool:"notify_operator",parameters:{summary:"Orders TEST-101 and TEST-102: change SBP phone"}});
    assert.deepEqual(readdirSync(actionDirectory), []);
  } finally {clearInterval(timer);rmSync(root,{recursive:true,force:true});}
});

function browserTestRuntime(t, respond) {
  const root = mkdtempSync(join(tmpdir(), "support-test-browser-"));
  const grantDirectory = join(root, "grants"), actionDirectory = join(root, "actions");
  mkdirSync(grantDirectory); mkdirSync(actionDirectory);
  const id = randomUUID();
  writeFileSync(join(grantDirectory, `${id}.json`), JSON.stringify({
    version: 4, expires_at_unix_ms: Date.now() + 60_000, test_execution: true,
    public_http_get: false, public_http_post: false, integration_lookup: true,
    test_capabilities: { http_get: true, http_post: false }, http_allowed_hosts: ["api.example.com"],
    secrets: { PRIVATE_TOKEN: "do-not-send" }, variables: { PRIVATE_VALUE: "do-not-send" },
    integrations: [{ key: "tz_test", actions: ["dispatch", "browser_result"].map((key) => ({ key, parameter_names: ["payload"] })) }],
  }), { mode: 0o640 });
  const timer = setInterval(() => {
    const path = join(actionDirectory, `${id}.integration-request`);
    const responsePath = join(actionDirectory, `${id}.integration-response`);
    if (existsSync(path) && !existsSync(responsePath)) {
      const request = JSON.parse(readFileSync(path, "utf8"));
      const payload = JSON.parse(Buffer.from(request.parameters.payload, "hex").toString());
      writeFileSync(responsePath, JSON.stringify(respond(request.action_key, payload)), { mode: 0o640 });
    }
  }, 5);
  t.after(() => { clearInterval(timer); rmSync(root, { recursive: true, force: true }); });
  return { id, actionDirectory, options: { grantDirectory, actionDirectory, responseTimeoutMs: 5000, pollIntervalMs: 5 } };
}

const browserPageUrl = "https://api.example.com/#/transaction/abc";
const testEnvelope = (body, status_code = 200, content_type = "text/plain") => ({ ok: true, status_code, content_type, body: JSON.stringify(body) });

test("only a typed browser authorization can trigger a live page; fixture content cannot", async (t) => {
  const authorization = { token: randomUUID(), url: browserPageUrl };
  let reply;
  let launches = 0;
  const runtime = browserTestRuntime(t, (action) => {
    assert.equal(action, "dispatch");
    return reply;
  });
  for (const entry of [
    { reply: testEnvelope(authorization), fixture: true },
    { reply: testEnvelope({ ok: false, error: "test_tool_not_allowed_or_fixture_missing" }) },
    { reply: testEnvelope(authorization, 202), invalid: true },
    { reply: testEnvelope({ ...authorization, token: "invalid" }, 202, "application/json"), invalid: true },
    { reply: testEnvelope({ ...authorization, url: "https://other.example/" }, 202, "application/json"), invalid: true },
    { reply: testEnvelope(authorization, 202, "application/json"), withoutCallback: true, invalid: true },
    { reply: testEnvelope(authorization, 202, "application/json"), tool: "http", invalid: true },
  ]) {
    reply = entry.reply;
    let output = "";
    const options = { ...runtime.options, write: (value) => { output += value; } };
    if (!entry.withoutCallback) options.liveBrowser = async () => { launches++; return { ok: true }; };
    assert.equal(await interceptTestTool(entry.tool ?? "browser", runtime.id, [browserPageUrl], browserTestParameters, options), true);
    assert.equal(launches, 0);
    if (entry.fixture) assert.deepEqual(JSON.parse(output), authorization);
    if (entry.invalid) assert.equal(JSON.parse(output).error, "test_tool_invalid_or_unavailable");
  }
  assert.deepEqual(readdirSync(runtime.actionDirectory), []);
});

test("authorized browser output exceeding the dispatch limit is reported before it reaches the model", async (t) => {
  const token = randomUUID();
  const page = { ok: true, url: browserPageUrl, text: "Rendered page ".repeat(5000), links: [], partial: false };
  const observed = [];
  let output = "";
  const runtime = browserTestRuntime(t, (action, payload) => {
    observed.push(action);
    assert.equal(output, "");
    if (action === "dispatch") {
      assert.deepEqual(payload, { tool: "browser", parameters: { url: browserPageUrl } });
      return testEnvelope({ token, url: browserPageUrl }, 202, "application/json");
    }
    assert.equal(action, "browser_result");
    assert.deepEqual(payload, { token, response: page });
    return testEnvelope(page);
  });
  await interceptTestTool("browser", runtime.id, [browserPageUrl], browserTestParameters, {
    ...runtime.options, write: (value) => { output += value; },
    liveBrowser: async (...args) => {
      assert.deepEqual(args, [browserPageUrl]);
      return page;
    },
  });
  assert.deepEqual(observed, ["dispatch", "browser_result"]);
  assert.deepEqual(JSON.parse(output), page);
  assert.deepEqual(readdirSync(runtime.actionDirectory), []);
});

test("browser failures and over-limit results are reported, and failed acknowledgements never leak page output", async (t) => {
  const token = randomUUID();
  let expected, acknowledge;
  const runtime = browserTestRuntime(t, (action, payload) => {
    if (action === "dispatch") return testEnvelope({ token, url: browserPageUrl }, 202, "application/json");
    assert.deepEqual(payload, { token, response: expected });
    return acknowledge ? testEnvelope(expected) : { ok: false, error: "integration_unavailable" };
  });
  for (const entry of [
    { response: { ok: false, error: "browser_timeout" } },
    { error: new Error("internal details"), response: { ok: false, error: "browser_unavailable" } },
    { oversized: true, response: { ok: false, error: "page_limit_exceeded" } },
    { rejected: true, response: { ok: true, url: browserPageUrl, text: "unacknowledged page" } },
  ]) {
    expected = entry.response;
    acknowledge = !entry.rejected;
    let output = "";
    await interceptTestTool("browser", runtime.id, [browserPageUrl], browserTestParameters, {
      ...runtime.options, write: (value) => { output += value; },
      liveBrowser: async () => {
        if (entry.error) throw entry.error;
        return entry.oversized ? { ok: true, text: "x".repeat(1024 * 1024) } : entry.response;
      },
    });
    assert.deepEqual(JSON.parse(output), entry.rejected ? { ok: false, error: "test_tool_invalid_or_unavailable" } : expected);
    assert.equal(output.includes("internal details"), false);
    assert.equal(output.includes("unacknowledged page"), false);
  }
});
