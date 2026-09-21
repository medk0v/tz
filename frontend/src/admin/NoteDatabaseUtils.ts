import type { JsonValue, NoteDatabaseField, NoteDatabaseRecord, ViewConfig } from "../note-databases-api";
import type { Locale } from "../i18n";
import { noteDatabasesText } from "./note-databases-i18n";

export function recordLabel(record: NoteDatabaseRecord, fields: NoteDatabaseField[], locale: Locale) {
  const title = fields.find((field) => field.field_type === "text");
  return title && typeof record.values[title.id] === "string" && record.values[title.id] ? String(record.values[title.id]) : noteDatabasesText(locale)("untitled");
}
export function displayValue(value: JsonValue | undefined, field: NoteDatabaseField, locale: Locale): string {
  const t = noteDatabasesText(locale);
  if (value === null || value === undefined || value === "") return "—";
  if (field.field_type === "boolean") return t(value ? "yes" : "no");
  if (field.field_type === "relation") return t("selected", { count: Array.isArray(value) ? value.length : 0 });
  const label = (item: JsonValue) => field.config.options?.find((option) => option.id === item)?.label ?? String(item);
  return Array.isArray(value) ? value.map(label).join(", ") : typeof value === "object" ? JSON.stringify(value) : label(value);
}
export function normalizeView(config: Partial<ViewConfig> | undefined, fields: NoteDatabaseField[]): ViewConfig {
  const ids = fields.map((field) => field.id);
  const order = (config?.column_order ?? []).filter((id) => ids.includes(id));
  return { filters: config?.filters ?? [], sorts: config?.sorts ?? [], hidden_fields: (config?.hidden_fields ?? []).filter((id) => ids.includes(id)),
    column_order: [...order, ...ids.filter((id) => !order.includes(id))], column_widths: { ...config?.column_widths }, pinned_fields: (config?.pinned_fields ?? []).filter((id) => ids.includes(id)) };
}
export function moveItem<T>(items: T[], index: number, offset: number): T[] {
  if (index + offset < 0 || index + offset >= items.length) return items;
  const next = [...items]; const [item] = next.splice(index, 1); next.splice(index + offset, 0, item); return next;
}
export function safeDatabaseUrl(value: JsonValue | undefined) {
  if (typeof value !== "string") return undefined;
  try { return ["https:", "http:"].includes(new URL(value).protocol) ? value : undefined; } catch { return undefined; }
}
