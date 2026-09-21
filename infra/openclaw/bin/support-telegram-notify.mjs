#!/usr/bin/env node

import { interceptTestTool, normalizeTestAction } from "./support-test-tool.mjs";

import { lstatSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const BOT_TOKEN_PATTERN = /^\d+:[A-Za-z0-9_-]{20,}$/;
const CHAT_ID_PATTERN = /^-?\d{1,20}$/;
const MAX_GRANT_BYTES = 3 * 1024 * 1024;
const MAX_NOTIFICATION_CHARACTERS = 4_000;
const MAX_RECIPIENTS = 32;
const GRANT_DIRECTORY = "/run/support-agent-secrets";

function finish(payload) {
  process.stdout.write(`${JSON.stringify(payload)}\n`);
  process.exit(0);
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function profileCredential(credentials, expectedKey) {
  return Object.entries(credentials).find(
    ([key]) => key.toLowerCase() === expectedKey.toLowerCase(),
  )?.[1];
}

export function resolveTelegramNotification(grant, notificationArguments, now = Date.now()) {
  if (
    !isObject(grant)
    || grant.version !== 4
    || !Number.isSafeInteger(grant.expires_at_unix_ms)
    || grant.expires_at_unix_ms < now
    || !isObject(grant.secrets)
  ) {
    throw new Error("invalid_grant");
  }

  let text;
  if (grant.telegram_dynamic_notification === true) {
    if (notificationArguments.length !== 1) throw new Error("invalid_arguments");
    [text] = notificationArguments;
    if (
      typeof text !== "string"
      || text.trim().length === 0
      || text.length > MAX_NOTIFICATION_CHARACTERS
      || text.includes("\0")
    ) {
      throw new Error("invalid_arguments");
    }
  } else {
    const withSummary = typeof grant.telegram_notification?.summary_header === "string"
      && notificationArguments.length === 2 && notificationArguments[0] === "--summary";
    if (notificationArguments.length !== 0 && !withSummary) throw new Error("invalid_arguments");
    if (
      !isObject(grant.telegram_notification)
      || typeof grant.telegram_notification.text !== "string"
      || grant.telegram_notification.text.length === 0
      || grant.telegram_notification.text.length > MAX_NOTIFICATION_CHARACTERS
    ) {
      throw new Error("not_allowed");
    }
    text = grant.telegram_notification.text;
    if (withSummary) {
      const summary = normalizeTestAction("notify_operator", notificationArguments).summary;
      const header = grant.telegram_notification.summary_header;
      if (!header.trim() || header.includes("\0")) throw new Error("not_allowed");
      text = `${header}\n\nКонтекст обращения:\n${summary}`;
      if (text.length > MAX_NOTIFICATION_CHARACTERS) throw new Error("invalid_arguments");
    }
  }

  const token = profileCredential(grant.secrets, "TELEGRAM_NOTIFY_BOT_TOKEN");
  const rawChatIds = profileCredential(grant.secrets, "TELEGRAM_NOTIFY_CHAT_IDS");
  if (typeof token !== "string" || !BOT_TOKEN_PATTERN.test(token) || typeof rawChatIds !== "string") {
    throw new Error("not_configured");
  }

  let chatIds;
  try {
    chatIds = JSON.parse(rawChatIds);
  } catch {
    throw new Error("not_configured");
  }
  if (
    !Array.isArray(chatIds)
    || chatIds.length === 0
    || chatIds.length > MAX_RECIPIENTS
    || chatIds.some((chatId) => typeof chatId !== "string" || !CHAT_ID_PATTERN.test(chatId))
  ) {
    throw new Error("not_configured");
  }

  return { token, chatIds, text };
}

export function loadTelegramNotification(
  grantIdInput,
  notificationArguments,
  grantDirectoryValue = GRANT_DIRECTORY,
) {
  if (!UUID_PATTERN.test(grantIdInput ?? "")) throw new Error("invalid_arguments");

  const grantDirectory = resolve(grantDirectoryValue);
  const grantPath = resolve(join(grantDirectory, `${grantIdInput.toLowerCase()}.json`));
  if (dirname(grantPath) !== grantDirectory) throw new Error("invalid_grant");

  let grant;
  try {
    const metadata = lstatSync(grantPath);
    if (!metadata.isFile() || metadata.isSymbolicLink() || metadata.size > MAX_GRANT_BYTES) {
      throw new Error("invalid_grant");
    }
    grant = JSON.parse(readFileSync(grantPath, "utf8"));
  } catch {
    throw new Error("invalid_grant");
  }
  return resolveTelegramNotification(grant, notificationArguments);
}

export async function performTelegramNotification(notification, fetchImplementation = fetch) {
  const endpoint = `https://api.telegram.org/bot${notification.token}/sendMessage`;
  const deliveries = await Promise.allSettled(
    notification.chatIds.map(async (chatId) => {
      const response = await fetchImplementation(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ chat_id: chatId, text: notification.text }),
        redirect: "error",
        signal: AbortSignal.timeout(15_000),
      });
      if (!response.ok) return false;
      const payload = await response.json();
      return isObject(payload) && payload.ok === true;
    }),
  );
  const delivered = deliveries.filter(
    (delivery) => delivery.status === "fulfilled" && delivery.value === true,
  ).length;
  return { ok: delivered > 0, delivered, total: notification.chatIds.length };
}

async function main() {
  let notification;
  try {
    const [grantIdInput, ...notificationArguments] = process.argv.slice(2);
    if (await interceptTestTool("notify_operator", grantIdInput, notificationArguments, (args) => normalizeTestAction("notify_operator", args))) return;
    notification = loadTelegramNotification(grantIdInput, notificationArguments);
  } catch (error) {
    finish({ ok: false, error: error.message });
  }
  finish(await performTelegramNotification(notification));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await main();
}
