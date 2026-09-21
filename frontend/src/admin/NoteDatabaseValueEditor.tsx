import { useEffect, useState } from "react";
import type { JsonValue, NoteDatabaseClient, NoteDatabaseDetail, NoteDatabaseField, NoteDatabaseRecord } from "../note-databases-api";
import { useI18n } from "../i18n";
import { displayValue, recordLabel } from "./NoteDatabaseUtils";
import { noteDatabasesText } from "./note-databases-i18n";

export function NoteDatabaseValueEditor({ client, field, value, onChange, disabled = false, currentRecordId, onOpenRelated }: {
  client: NoteDatabaseClient; field: NoteDatabaseField; value: JsonValue | undefined; onChange: (value: JsonValue) => void; disabled?: boolean;
  currentRecordId?: string; onOpenRelated?: (detail: NoteDatabaseDetail, recordId: string) => void;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  if (field.field_type === "rollup") return <output className="note-database-calculated">{displayValue(value, field, locale)}<small>{t("calculated")}</small></output>;
  if (field.field_type === "relation") return <RelationValues client={client} field={field} value={value} onChange={onChange} disabled={disabled} currentRecordId={currentRecordId} onOpenRelated={onOpenRelated} />;
  if (field.field_type === "boolean") return <input type="checkbox" aria-label={field.name} checked={value === true} disabled={disabled} onChange={(event) => onChange(event.target.checked)} />;
  if (field.field_type === "single_select") return <select aria-label={field.name} disabled={disabled} value={typeof value === "string" ? value : ""} onChange={(event) => onChange(event.target.value || null)}>
    <option value="">—</option>{field.config.options?.map((option) => <option key={option.id} value={option.id}>{option.label}</option>)}</select>;
  if (field.field_type === "multi_select") {
    const selected = Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
    return <div className="note-database-checks" role="group" aria-label={field.name}>{field.config.options?.map((option) => <label key={option.id}><input type="checkbox" disabled={disabled} checked={selected.includes(option.id)}
      onChange={(event) => onChange(event.target.checked ? [...selected, option.id] : selected.filter((id) => id !== option.id))} />{option.label}</label>)}</div>;
  }
  if (field.field_type === "long_text") return <textarea rows={4} aria-label={field.name} disabled={disabled} value={typeof value === "string" ? value : ""} onChange={(event) => onChange(event.target.value || null)} />;
  return <input aria-label={field.name} disabled={disabled} type={field.field_type === "number" ? "number" : field.field_type === "date" ? "date" : field.field_type === "url" ? "url" : field.field_type === "email" ? "email" : "text"}
    step={field.field_type === "number" ? "any" : undefined} value={field.field_type === "date" && typeof value === "string" ? value.slice(0, 10) : typeof value === "string" || typeof value === "number" ? value : ""}
    onChange={(event) => onChange(event.target.value === "" ? null : field.field_type === "number" ? Number(event.target.value) : event.target.value)} />;
}

function RelationValues({ client, field, value, onChange, disabled, currentRecordId, onOpenRelated }: Parameters<typeof NoteDatabaseValueEditor>[0]) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  const selected = Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];
  const [target, setTarget] = useState<NoteDatabaseDetail | null>(null);
  const [records, setRecords] = useState<NoteDatabaseRecord[]>([]);
  const [query, setQuery] = useState(""); const [page, setPage] = useState(1); const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true); const [failed, setFailed] = useState(false); const [retry, setRetry] = useState(0);
  const targetId = field.relation_target_database_id;
  const single = field.relation_cardinality === "one_to_one" || field.relation_cardinality === "many_to_one";
  useEffect(() => {
    if (!targetId) return;
    const controller = new AbortController();
    Promise.all([client.getDatabase(targetId, controller.signal), client.listRecords(targetId, { page, per_page: 50, q: query }, controller.signal)]).then(([detail, result]) => {
      if (controller.signal.aborted) return; setTarget(detail); setRecords(result.items); setTotal(result.total); setFailed(false);
    }).catch(() => { if (!controller.signal.aborted) setFailed(true); }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [client, targetId, page, query, retry]);
  if (!targetId) return <p>{t("noRelations")}</p>;
  return <div className="note-database-relation" role="group" aria-label={field.name}>
    <input aria-label={t("chooseRelations")} type="search" value={query} placeholder={t("search")} onChange={(event) => { setQuery(event.target.value); setPage(1); setLoading(true); }} />
    <p>{t("selected", { count: selected.length })}</p>
    {selected.filter((id) => !records.some((record) => record.id === id)).map((id) => <div className="note-database-inline" key={id}><label className="note-database-check"><input type="checkbox" checked disabled={disabled} onChange={() => onChange(selected.filter((item) => item !== id))} />{id}</label>{target && onOpenRelated && <button type="button" aria-label={`${t("openRecord")}: ${id}`} onClick={() => onOpenRelated(target, id)}>{t("openRecord")}</button>}</div>)}
    {loading && <p role="status">{t("loading")}</p>}{failed && <p role="alert">{t("loadError")} <button type="button" onClick={() => { setLoading(true); setRetry((v) => v + 1); }}>{t("retry")}</button></p>}
    <div className="note-database-relation-list">{records.filter((record) => record.id !== currentRecordId).map((record) => <div className="note-database-inline" key={record.id}><label><input type="checkbox" disabled={disabled} checked={selected.includes(record.id)} onChange={(event) => onChange(event.target.checked ? single ? [record.id] : [...selected, record.id] : selected.filter((id) => id !== record.id))} />{recordLabel(record, target?.fields ?? [], locale)}</label>{target && onOpenRelated && <button type="button" aria-label={`${t("openRecord")}: ${recordLabel(record, target.fields, locale)}`} onClick={() => onOpenRelated(target, record.id)}>{t("openRecord")}</button>}</div>)}</div>
    <div className="note-database-pagination"><button type="button" disabled={page <= 1 || loading} onClick={() => { setPage((v) => v - 1); setLoading(true); }}>{t("previous")}</button><span>{page}</span><button type="button" disabled={page * 50 >= total || loading} onClick={() => { setPage((v) => v + 1); setLoading(true); }}>{t("next")}</button></div>
  </div>;
}
