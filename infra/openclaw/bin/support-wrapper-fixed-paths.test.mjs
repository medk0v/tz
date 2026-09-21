#!/usr/bin/env node

import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import {
  mkdtempSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { test } from "node:test";

const BIN_DIRECTORY = dirname(fileURLToPath(import.meta.url));

test("all production wrappers ignore hostile security-path environment overrides", () => {
  const root = mkdtempSync(join(tmpdir(), "support-wrapper-fixed-paths-"));
  const grantDirectory = join(root, "grants");
  const actionDirectory = join(root, "actions");
  mkdirSync(grantDirectory);
  mkdirSync(actionDirectory);
  const grantId = randomUUID();
  const articleId = randomUUID();
  const reminderActionId = randomUUID();
  writeFileSync(join(grantDirectory, `${grantId}.json`), JSON.stringify({
    version: 4,
    expires_at_unix_ms: Date.now() + 60_000,
    secrets: {
      API_TOKEN: "test-token",
      TELEGRAM_NOTIFY_BOT_TOKEN: "invalid",
      TELEGRAM_NOTIFY_CHAT_IDS: "[]",
    },
    telegram_notification: { text: "hostile" },
    resolve_conversation: true,
    schedule_reminder: true,
    reminder_action_id: reminderActionId,
    public_http_get: true,
    public_http_post: false,
    shell: true,
    knowledge_lookup: false,
    integration_lookup: true,
    integrations: [{
      key: "order_api",
      actions: [{ key: "get_status", parameter_names: ["order_id"] }],
    }],
  }));

  const cases = [
    ["support-telegram-notify.mjs", [grantId], "invalid_grant"],
    ["support-resolve-conversation.mjs", [grantId], "invalid_grant"],
    ["support-schedule-reminder.mjs", [grantId, "300"], "invalid_grant"],
    ["support-public-http.mjs", [grantId, "https://127.0.0.1"], "not_allowed"],
    ["support-curl.mjs", ["--support-grant", grantId, "https://api.example.com"], "not_allowed"],
    ["support-knowledge-article.mjs", [grantId, articleId], "invalid_grant"],
    ["support-integration.mjs", [grantId, "order_api", "get_status", '{"order_id":"42"}'], "invalid_grant"],
    ["support-shell.mjs", [grantId, "pwd"], "not_allowed"],
    ["support-browser.mjs", [grantId, "https://explorer.example"], "not_allowed"],
  ];
  try {
    for (const [script, arguments_, expectedError] of cases) {
      const source = readFileSync(join(BIN_DIRECTORY, script), "utf8");
      assert.equal(source.includes("process.env.SUPPORT_"), false, script);
      const result = spawnSync(process.execPath, [join(BIN_DIRECTORY, script), ...arguments_], {
        encoding: "utf8",
        env: {
          ...process.env,
          SUPPORT_AGENT_GRANT_DIR: grantDirectory,
          SUPPORT_AGENT_ACTION_DIR: actionDirectory,
          SUPPORT_PUBLIC_HTTP_HOSTS: "127.0.0.1",
          SUPPORT_SHELL_RUNNER_SOCKET: join(root, "hostile.sock"),
          SUPPORT_BROWSER_RUNNER_SOCKET: join(root, "hostile-browser.sock"),
        },
      });
      assert.equal(result.status, 0, `${script}: ${result.stderr}`);
      assert.equal(result.stderr, "", script);
      assert.equal(JSON.parse(result.stdout).error, expectedError, script);
    }
    assert.deepEqual(readdirSync(actionDirectory), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("browser is allowlisted only as a grant-checking wrapper with an offline runner", () => {
  const root = resolve(BIN_DIRECTORY, "../../..");
  const compose = readFileSync(join(root, "infra/openclaw/compose.yaml"), "utf8");
  const browser = compose.split("\n  browser-runner:\n")[1].split("\n  shell-runner:\n")[0];
  assert.ok(browser.includes("network_mode: none"));
  assert.ok(browser.includes("read_only: true"));
  assert.equal(browser.includes("agent-secrets"), false);
  const manage = readFileSync(join(root, "infra/openclaw/manage.sh"), "utf8");
  assert.equal(manage.match(/"\/usr\/local\/bin\/support-browser"/g)?.length, 2);
  assert.ok(manage.includes('isDeepStrictEqual(support.tools.allow, ["exec"])'));
});

test("the integration wrapper is packaged and allowlisted under its fixed path", () => {
  const repositoryRoot = resolve(BIN_DIRECTORY, "../../..");
  const dockerfile = readFileSync(join(repositoryRoot, "infra/openclaw/Dockerfile"), "utf8");
  const manageScript = readFileSync(join(repositoryRoot, "infra/openclaw/manage.sh"), "utf8");
  const wrapperPath = "/usr/local/bin/support-integration";

  assert.ok(dockerfile.includes(
    `COPY bin/support-integration.mjs ${wrapperPath}`,
  ));
  assert.equal(
    manageScript.match(new RegExp(`"${wrapperPath}"`, "g"))?.length,
    2,
  );
});

test("curl resolves to the protected launcher without approving the native binary", () => {
  const repositoryRoot = resolve(BIN_DIRECTORY, "../../..");
  const dockerfile = readFileSync(join(repositoryRoot, "infra/openclaw/Dockerfile"), "utf8");
  const manageScript = readFileSync(join(repositoryRoot, "infra/openclaw/manage.sh"), "utf8");
  assert.ok(dockerfile.includes("COPY bin/support-curl.mjs /usr/local/bin/curl"));
  assert.ok(dockerfile.includes("COPY bin/support-public-http.mjs /usr/local/bin/support-public-http.mjs"));
  assert.ok(dockerfile.includes("COPY bin/support-public-network.mjs /usr/local/bin/support-public-network.mjs"));
  assert.equal(manageScript.match(/"\/usr\/local\/bin\/curl"/g)?.length, 2);
  assert.equal(manageScript.includes('"/usr/bin/curl"'), false);
});

test("the dedicated workspace cannot remain in OpenClaw first-run bootstrap", () => {
  const repositoryRoot = resolve(BIN_DIRECTORY, "../../..");
  const manageScript = readFileSync(join(repositoryRoot, "infra/openclaw/manage.sh"), "utf8");

  assert.ok(manageScript.includes(
    'rm -f -- "$SUPPORT_WORKSPACE_DIR/BOOTSTRAP.md"',
  ));
  assert.ok(manageScript.includes(
    '[[ ! -e "$SUPPORT_WORKSPACE_DIR/BOOTSTRAP.md" ]]',
  ));
  assert.ok(manageScript.includes(
    'cmp -s "$WORKSPACE_IDENTITY_TEMPLATE" "$SUPPORT_WORKSPACE_DIR/IDENTITY.md"',
  ));
  assert.equal(
    manageScript.match(/configure_gateway\n\s+install_workspace_policy/g)?.length,
    2,
  );
  assert.equal(
    manageScript.match(/prepare_local_state\n\s+install_workspace_policy/g)?.length,
    2,
  );
  assert.ok(manageScript.includes(
    "verify_support_agent_config() {\n  verify_workspace_policy",
  ));
});
