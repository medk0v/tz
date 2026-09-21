import { useEffect, useState, type CSSProperties, type FormEvent } from "react";
import { ArrowDown, ArrowUp, ChevronLeft, ChevronRight, ExternalLink, Plus, RefreshCw, Settings2 } from "lucide-react";
import type { NoteDatabaseClient, NoteDatabaseDetail, NoteDatabaseRecords } from "../note-databases-api";
import { useI18n } from "../i18n";
import { NoteDatabaseDialog } from "./NoteDatabaseDialog";
import { NoteDatabaseFieldsDialog } from "./NoteDatabaseFieldsDialog";
import { NoteDatabaseViewsDialog } from "./NoteDatabaseViewsDialog";
import { NoteDatabaseRecordDialog } from "./NoteDatabaseRecordDialog";
import { displayValue, moveItem, normalizeView, recordLabel, safeDatabaseUrl } from "./NoteDatabaseUtils";
import { noteDatabaseError, noteDatabasesText } from "./note-databases-i18n";
import "./NoteDatabase.css";
export { NoteDatabasePicker } from "./NoteDatabasePicker";

type Dialog = { kind: "fields" | "views" | "settings" } | { kind: "record"; id: string; fieldId?: string };
export function NoteDatabaseTable({ client, databaseId, initialViewId, canWrite, canManage = canWrite, canEditFields = canManage, onMutation, onDirtyChange, onBusyChange }: {
  client: NoteDatabaseClient; databaseId: string; initialViewId?: string | null; canWrite: boolean; canManage?: boolean; canEditFields?: boolean;
  onMutation?: () => void; onDirtyChange?: (dirty: boolean) => void; onBusyChange?: (busy: boolean) => void;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  const [detail, setDetail] = useState<NoteDatabaseDetail | null>(null); const [records, setRecords] = useState<NoteDatabaseRecords | null>(null);
  const [viewId, setViewId] = useState(initialViewId ?? ""); const [page, setPage] = useState(1); const [search, setSearch] = useState(""); const [query, setQuery] = useState("");
  const [revision, setRevision] = useState(0); const [loading, setLoading] = useState(true); const [error, setError] = useState(""); const [busy, setBusy] = useState(false);
  const [dialogBusy, setDialogBusy] = useState(false);
  const [dialog, setDialog] = useState<Dialog | null>(null); const [dialogDirty, setDialogDirty] = useState(false); const [deleted, setDeleted] = useState(false);
  useEffect(() => { onDirtyChange?.(dialogDirty); return () => onDirtyChange?.(false); }, [dialogDirty, onDirtyChange]);
  useEffect(() => { onBusyChange?.(busy || dialogBusy); return () => onBusyChange?.(false); }, [busy, dialogBusy, onBusyChange]);
  useEffect(() => {
    if (!busy) return;
    const guard = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ""; };
    window.addEventListener("beforeunload", guard); return () => window.removeEventListener("beforeunload", guard);
  }, [busy]);
  useEffect(() => {
    const timer = window.setTimeout(() => { setQuery(search.trim()); setPage(1); }, 250);
    return () => window.clearTimeout(timer);
  }, [search]);
  useEffect(() => {
    const controller = new AbortController();
    Promise.allSettled([client.getDatabase(databaseId, controller.signal), client.listRecords(databaseId, { page, per_page: 50, q: query, view_id: viewId || undefined }, controller.signal)])
      .then(([data, rows]) => {
        if (controller.signal.aborted) return;
        if (data.status === "rejected") throw data.reason;
        setDetail(data.value);
        if (viewId && !data.value.views.some((view) => view.id === viewId)) { setViewId(""); setPage(1); return; }
        if (rows.status === "rejected") throw rows.reason;
        if (page > 1 && (page - 1) * 50 >= rows.value.total) { setPage(Math.max(1, Math.ceil(rows.value.total / 50))); return; }
        setRecords(rows.value); setError("");
      })
      .catch(() => { if (!controller.signal.aborted) setError(noteDatabasesText(locale)("loadError")); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [client, databaseId, page, query, viewId, revision, locale]);
  function refresh() { setLoading(true); setRevision((value) => value + 1); }
  function changed() { refresh(); onMutation?.(); }
  async function addRecord() {
    if (!canWrite || busy) return; setBusy(true); setError("");
    try { const record = await client.createRecord(databaseId, { values: {}, content_markdown: "" }); changed(); setDialog({ kind: "record", id: record.id }); }
    catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  async function reorder(id: string, offset: number) {
    if (!detail || !canManage || busy || viewId || query) return;
    setBusy(true); setError("");
    try {
      const ids: string[] = []; let nextPage = 1; let total = 0;
      do { const result = await client.listRecords(databaseId, { page: nextPage++, per_page: 500 }); ids.push(...result.items.map((record) => record.id)); total = result.total; if (!result.items.length) break; } while (ids.length < total);
      const index = ids.indexOf(id);
      if (index < 0 || ids.length !== total) throw new Error("The record list changed.");
      await client.reorderRecords(databaseId, moveItem(ids, index, offset), detail.database.version); changed();
    } catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  const view = detail?.views.find((item) => item.id === viewId); const config = normalizeView(view?.config, detail?.fields ?? []);
  const order = [...config.pinned_fields.filter((id) => !config.hidden_fields.includes(id)), ...config.column_order.filter((id) => !config.hidden_fields.includes(id) && !config.pinned_fields.includes(id))];
  const fields = order.flatMap((id) => { const field = detail?.fields.find((field) => field.id === id); return field ? [field] : []; });
  let offset = 0;
  const styles = new Map<string, CSSProperties>(fields.map((field) => {
    const width = config.column_widths[field.id] ?? 180; const pinned = config.pinned_fields.includes(field.id);
    const style: CSSProperties = { width, minWidth: width, maxWidth: width, ...(pinned ? { position: "sticky", left: offset } : {}) };
    if (pinned) offset += width; return [field.id, style];
  }));
  if (deleted) return <p className="note-database-help">{t("emptyTables")}</p>;
  return <section className="note-database" aria-label={detail?.database.name ?? t("tables")} aria-busy={loading || busy}>
    <header className="note-database-header"><div><h3>{detail?.database.icon} {detail?.database.name ?? t("tables")}</h3>{detail?.database.description && <p>{detail.database.description}</p>}</div>
      <button type="button" aria-label={t("refresh")} title={t("refresh")} disabled={busy || loading} onClick={refresh}><RefreshCw size={16} /></button>
      {canManage && detail && <button type="button" aria-label={t("settings")} title={t("settings")} disabled={busy || loading} onClick={() => setDialog({ kind: "settings" })}><Settings2 size={16} /></button>}</header>
    {error && <div className="note-database-error" role="alert"><span>{error}</span><button type="button" disabled={busy || loading} onClick={refresh}>{t("retry")}</button></div>}
    {!detail || !records ? loading && <p role="status">{t("loading")}</p> : <>
      <div className="note-database-toolbar"><label className="note-database-view-select"><span className="sr-only">{t("chooseView")}</span><select value={viewId} disabled={busy || loading} onChange={(event) => { setViewId(event.target.value); setPage(1); setLoading(true); }}><option value="">{t("allRecords")}</option>{detail.views.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}</select></label>
        {canManage && <button type="button" disabled={busy || loading} onClick={() => setDialog({ kind: "views" })}>{t("views")}</button>}{canEditFields && <button type="button" disabled={busy || loading} onClick={() => setDialog({ kind: "fields" })}>{t("fields")}</button>}
        <input type="search" aria-label={t("search")} placeholder={t("search")} value={search} disabled={busy} onChange={(event) => setSearch(event.target.value)} />
        {canWrite && <button type="button" className="secondary-button" disabled={busy || loading || !detail.fields.length} onClick={() => void addRecord()}><Plus size={15} />{t("addRecord")}</button>}</div>
      {!detail.fields.length && <p className="note-database-help">{t("emptyFields")}{canEditFields && <button type="button" onClick={() => setDialog({ kind: "fields" })}>{t("addField")}</button>}</p>}
      <div className="note-database-scroll"><table><thead><tr>{fields.map((field) => <th key={field.id} scope="col" style={styles.get(field.id)} className={config.pinned_fields.includes(field.id) ? "is-pinned" : undefined}><span title={field.name}>{field.name}</span><small>{t(field.field_type)}</small></th>)}<th scope="col" className="note-database-row-actions"><span className="sr-only">{t("record")}</span></th></tr></thead>
        <tbody>{records.items.map((record, index) => <tr key={record.id}>{fields.map((field) => {
          const value = displayValue(record.values[field.id], field, locale); const href = field.field_type === "url" ? safeDatabaseUrl(record.values[field.id]) : null;
          return <td key={field.id} style={styles.get(field.id)} className={config.pinned_fields.includes(field.id) ? "is-pinned" : undefined}>{canWrite && field.field_type !== "rollup" ? <button type="button" className="note-database-cell" aria-label={t("editCell", { field: field.name })} disabled={busy || loading} title={value || undefined} onClick={() => setDialog({ kind: "record", id: record.id, fieldId: field.id })}>{value || <span aria-hidden="true">—</span>}</button> : href ? <a href={href} target="_blank" rel="noopener noreferrer" className="note-database-cell">{value}</a> : <span className="note-database-cell" title={value}>{value || "—"}</span>}</td>;
        })}<td className="note-database-row-actions"><button type="button" aria-label={`${t("openRecord")}: ${recordLabel(record, detail.fields, locale)}`} title={t("openRecord")} disabled={busy || loading} onClick={() => setDialog({ kind: "record", id: record.id })}><ExternalLink size={14} /></button>
          {canManage && !viewId && !query && <><button type="button" aria-label={`${t("moveUp")}: ${recordLabel(record, detail.fields, locale)}`} disabled={busy || loading || (page === 1 && index === 0)} onClick={() => void reorder(record.id, -1)}><ArrowUp size={13} /></button><button type="button" aria-label={`${t("moveDown")}: ${recordLabel(record, detail.fields, locale)}`} disabled={busy || loading || (page - 1) * 50 + index >= records.total - 1} onClick={() => void reorder(record.id, 1)}><ArrowDown size={13} /></button></>}</td></tr>)}</tbody></table></div>
      {!records.items.length && <p className="note-database-empty">{t("empty")}</p>}
      <footer className="note-database-pagination"><span aria-live="polite">{loading ? t("loading") : t("pagination", { from: records.total ? (page - 1) * 50 + 1 : 0, to: Math.min(page * 50, records.total), total: records.total })}</span><button type="button" aria-label={t("previous")} disabled={loading || busy || page <= 1} onClick={() => { setPage((value) => value - 1); setLoading(true); }}><ChevronLeft size={17} /></button><button type="button" aria-label={t("next")} disabled={loading || busy || page * 50 >= records.total} onClick={() => { setPage((value) => value + 1); setLoading(true); }}><ChevronRight size={17} /></button></footer>
    </>}
    {detail && dialog?.kind === "record" && <NoteDatabaseRecordDialog key={`${dialog.id}:${dialog.fieldId ?? "card"}`} client={client} databaseId={databaseId} fields={detail.fields} recordId={dialog.id} fieldId={dialog.fieldId} canWrite={canWrite} onClose={() => setDialog(null)} onSaved={changed} onDirtyChange={setDialogDirty} onBusyChange={setDialogBusy} />}
    {detail && canEditFields && dialog?.kind === "fields" && <NoteDatabaseFieldsDialog client={client} detail={detail} canReorder={canManage} onClose={() => setDialog(null)} onChange={changed} onDirtyChange={setDialogDirty} onBusyChange={setDialogBusy} />}
    {detail && canManage && dialog?.kind === "views" && <NoteDatabaseViewsDialog client={client} detail={detail} initialViewId={viewId || undefined} onSelect={(id) => { setViewId(id); setPage(1); }} onClose={() => setDialog(null)} onChange={changed} onDirtyChange={setDialogDirty} onBusyChange={setDialogBusy} />}
    {detail && canManage && dialog?.kind === "settings" && <DatabaseSettings client={client} detail={detail} onClose={() => setDialog(null)} onSaved={changed} onDeleted={() => { setDialog(null); setDeleted(true); onMutation?.(); }} onDirtyChange={setDialogDirty} onBusyChange={setDialogBusy} />}
  </section>;
}

function DatabaseSettings({ client, detail, onClose, onSaved, onDeleted, onDirtyChange, onBusyChange }: { client: NoteDatabaseClient; detail: NoteDatabaseDetail; onClose: () => void; onSaved: () => void; onDeleted: () => void; onDirtyChange: (dirty: boolean) => void; onBusyChange: (busy: boolean) => void }) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale); const database = detail.database;
  const [name, setName] = useState(database.name); const [description, setDescription] = useState(database.description); const [icon, setIcon] = useState(database.icon);
  const [busy, setBusy] = useState(false); const [error, setError] = useState(""); const dirty = name !== database.name || description !== database.description || icon !== database.icon;
  async function save(event: FormEvent<HTMLFormElement>) { event.preventDefault(); event.stopPropagation(); if (busy || !event.currentTarget.checkValidity()) return; setBusy(true); setError("");
    try { await client.updateDatabase(database.id, { name: name.trim(), description, icon, expected_version: database.version }); onSaved(); onClose(); } catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); } }
  async function remove() { if (busy || !window.confirm(t("deleteTableConfirm"))) return; setBusy(true); setError(""); try { await client.deleteDatabase(database.id, database.version); onDeleted(); } catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); } }
  return <NoteDatabaseDialog title={t("settings")} onClose={onClose} dirty={dirty} busy={busy} onDirtyChange={onDirtyChange} onBusyChange={onBusyChange}><form className="note-database-form" onSubmit={(event) => void save(event)}>
    <label>{t("name")}<input value={name} maxLength={160} required disabled={busy} onChange={(event) => setName(event.target.value)} /></label><label>{t("description")}<textarea value={description} maxLength={4000} disabled={busy} onChange={(event) => setDescription(event.target.value)} /></label><label>{t("icon")}<input value={icon} maxLength={32} disabled={busy} onChange={(event) => setIcon(event.target.value)} /></label>
    {error && <p role="alert" className="note-database-error">{error}</p>}<footer><button type="button" className="note-database-danger" disabled={busy} onClick={() => void remove()}>{t("deleteTable")}</button><button type="submit" className="primary-button" disabled={busy || !dirty || !name.trim()}>{t(busy ? "saving" : "save")}</button></footer></form></NoteDatabaseDialog>;
}
