#!/usr/bin/env node

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  performTelegramNotification,
  resolveTelegramNotification,
} from "./support-telegram-notify.mjs";

const NOW = 1_800_000_000_000;
const TOKEN = `123456789:${"a".repeat(32)}`;
const SECRETS = {
  TELEGRAM_NOTIFY_BOT_TOKEN: TOKEN,
  TELEGRAM_NOTIFY_CHAT_IDS: JSON.stringify(["123", "-456"]),
};

function grant(overrides = {}) {
  return {
    version: 4,
    expires_at_unix_ms: NOW + 60_000,
    secrets: SECRETS,
    ...overrides,
  };
}

test("accepts a bounded dynamic notification only with an explicit task capability", () => {
  assert.deepEqual(
    resolveTelegramNotification(
      grant({ telegram_dynamic_notification: true }),
      ["Order finished"],
      NOW,
    ),
    { token: TOKEN, chatIds: ["123", "-456"], text: "Order finished" },
  );

  for (const notificationArguments of [[], [""], ["x", "extra"], ["x".repeat(4_001)]]) {
    assert.throws(
      () => resolveTelegramNotification(
        grant({ telegram_dynamic_notification: true }),
        notificationArguments,
        NOW,
      ),
      /invalid_arguments/,
    );
  }
  assert.throws(
    () => resolveTelegramNotification(grant(), ["Order finished"], NOW),
    /invalid_arguments/,
  );
});

test("preserves fixed operator notifications without accepting caller text", () => {
  assert.deepEqual(
    resolveTelegramNotification(
      grant({ telegram_notification: { text: "Customer requests an operator" } }),
      [],
      NOW,
    ),
    {
      token: TOKEN,
      chatIds: ["123", "-456"],
      text: "Customer requests an operator",
    },
  );
  assert.throws(
    () => resolveTelegramNotification(grant(), [], NOW),
    /not_allowed/,
  );
});

test("requires current grants and configured Telegram credentials", () => {
  for (const invalidGrant of [
    grant({ version: 3, telegram_dynamic_notification: true }),
    grant({ expires_at_unix_ms: NOW - 1, telegram_dynamic_notification: true }),
    grant({ secrets: {}, telegram_dynamic_notification: true }),
    grant({
      secrets: { ...SECRETS, TELEGRAM_NOTIFY_CHAT_IDS: "[]" },
      telegram_dynamic_notification: true,
    }),
  ]) {
    assert.throws(
      () => resolveTelegramNotification(invalidGrant, ["Order finished"], NOW),
    );
  }
});

test("handoff summaries preserve trusted chat context and configured recipients", async () => {
  const summary = "Orders TEST-101 and TEST-102: customer requests an SBP phone change. Bank: Test Bank. Payout status is unverified.";
  const notification = { text: "Legacy notification", summary_header: "Chat: trusted-conversation" };
  const resolved = resolveTelegramNotification(grant({ telegram_notification: notification }), ["--summary", summary], NOW);
  assert.equal(resolved.text, `Chat: trusted-conversation\n\nКонтекст обращения:\n${summary}`);
  assert.deepEqual(resolved.chatIds, ["123", "-456"]);
  const messages = [];
  await performTelegramNotification(resolved, async (_url, options) => {
    messages.push(JSON.parse(options.body));
    return { ok: true, json: async () => ({ ok: true }) };
  });
  assert.ok(messages.every((message) => message.text === resolved.text));
  for (const args of [["--summary", ""], ["--summary", "x".repeat(2501)], ["--summary", "x\0y"], ["--summary", summary, "other-recipient"]]) {
    assert.throws(() => resolveTelegramNotification(grant({ telegram_notification: notification }), args, NOW), /invalid_arguments/);
  }
  assert.throws(() => resolveTelegramNotification(grant({ telegram_notification: { text: "Legacy" } }), ["--summary", summary], NOW), /invalid_arguments/);
  assert.equal(resolveTelegramNotification(grant({ telegram_notification: notification }), [], NOW).text, "Legacy notification");
});

test("sends the granted text only to configured recipients", async () => {
  const requests = [];
  const result = await performTelegramNotification(
    { token: TOKEN, chatIds: ["123", "-456"], text: "Order finished" },
    async (url, options) => {
      requests.push({ url, options });
      return { ok: true, json: async () => ({ ok: true }) };
    },
  );

  assert.deepEqual(result, { ok: true, delivered: 2, total: 2 });
  assert.deepEqual(
    requests.map(({ options }) => JSON.parse(options.body)),
    [
      { chat_id: "123", text: "Order finished" },
      { chat_id: "-456", text: "Order finished" },
    ],
  );
  assert.ok(requests.every(({ url }) => url === `https://api.telegram.org/bot${TOKEN}/sendMessage`));
});
