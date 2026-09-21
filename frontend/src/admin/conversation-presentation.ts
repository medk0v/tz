import type { OperatorConversation } from "../api";
import type { createI18n } from "../i18n";

type Translate = ReturnType<typeof createI18n>["t"];

export function conversationName(conversation: OperatorConversation, t: Translate): string {
  return conversation.contact?.display_name?.trim()
    || conversation.contact?.email?.trim()
    || t("conversations.visitorName", { id: conversation.contact_id.replaceAll("-", "").slice(-6).toUpperCase() });
}

export function conversationPreview(conversation: OperatorConversation, t: Translate): string {
  const message = conversation.last_message;
  if (!message) return conversation.subject || t("conversations.noMessages");
  let body = message.body;
  if (message.body_format === "markdown") {
    body = body.replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/(^|\n)\s*(?:#{1,6}\s+|>\s*|[-*+]\s+|\d+\.\s+)/g, "$1")
      .replace(/[*_~`]/g, "");
  }
  return body.replace(/\s+/g, " ").trim()
    || t(message.kind === "attachment" ? "conversations.attachmentPreview" : "conversations.noMessages");
}

export function sameCalendarDay(value: string, date: Date): boolean {
  const other = new Date(value);
  return other.getFullYear() === date.getFullYear()
    && other.getMonth() === date.getMonth()
    && other.getDate() === date.getDate();
}

export function waitingLabel(since: string, now: number, t: Translate): string {
  const minutes = Math.max(0, Math.floor((now - new Date(since).getTime()) / 60_000));
  if (minutes < 1) return t("conversations.waitingJustNow");
  if (minutes < 60) return t("conversations.waitingMinutes", { count: minutes });
  if (minutes < 1440) return t("conversations.waitingHours", { count: Math.floor(minutes / 60) });
  return t("conversations.waitingDays", { count: Math.floor(minutes / 1440) });
}
