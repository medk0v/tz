import { useEffect, useState, type FormEvent } from "react";
import type { NoteDatabase, NoteDatabaseClient, NoteDatabaseDetail } from "../note-databases-api";
import { useI18n } from "../i18n";
import { NoteDatabaseDialog } from "./NoteDatabaseDialog";
import { noteDatabaseError, noteDatabasesText } from "./note-databases-i18n";

export function NoteDatabasePicker({ client, canWrite, onSelect, onCancel, onDirtyChange, onBusyChange }: {
  client: NoteDatabaseClient; canWrite: boolean; onSelect: (databaseId: string, viewId: string | null) => void | Promise<void>; onCancel?: () => void; onDirtyChange?: (dirty: boolean) => void; onBusyChange?: (busy: boolean) => void;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  const [items, setItems] = useState<NoteDatabase[]>([]); const [selectedId, setSelectedId] = useState(""); const [detail, setDetail] = useState<NoteDatabaseDetail | null>(null); const [viewId, setViewId] = useState("");
  const [create, setCreate] = useState(false); const [name, setName] = useState(""); const [description, setDescription] = useState("");
  const [loading, setLoading] = useState(true); const [busy, setBusy] = useState(false); const [error, setError] = useState(""); const [retry, setRetry] = useState(0);
  useEffect(() => { const controller = new AbortController(); client.listDatabases(controller.signal).then((value) => { if (!controller.signal.aborted) { setItems(value); setError(""); } }).catch(() => { if (!controller.signal.aborted) setError(noteDatabasesText(locale)("loadError")); }).finally(() => { if (!controller.signal.aborted) setLoading(false); }); return () => controller.abort(); }, [client, locale, retry]);
  useEffect(() => { if (!selectedId) return; const controller = new AbortController(); client.getDatabase(selectedId, controller.signal).then((value) => { if (!controller.signal.aborted) setDetail(value); }).catch(() => { if (!controller.signal.aborted) setError(noteDatabasesText(locale)("loadError")); }); return () => controller.abort(); }, [client, selectedId, locale]);
  async function submit(event: FormEvent<HTMLFormElement>) { event.preventDefault(); event.stopPropagation(); if (busy || !event.currentTarget.checkValidity()) return; setBusy(true); setError("");
    try {
      let id = selectedId;
      if (create && canWrite) { const saved = await client.createDatabase({ name: name.trim(), description, icon: "" }); id = saved.id; setItems((value) => [saved, ...value]); setSelectedId(id); setCreate(false); setName(""); setDescription(""); setViewId(""); }
      if (id) await onSelect(id, create ? null : viewId || null);
    } catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  return <NoteDatabaseDialog title={t("chooseTable")} onClose={() => onCancel?.()} dirty={!!name || !!description} busy={busy} onDirtyChange={onDirtyChange} onBusyChange={onBusyChange}><form className="note-database-form" onSubmit={(event) => void submit(event)}>
    {loading ? <p role="status">{t("loading")}</p> : <><div className="note-database-inline"><button type="button" aria-pressed={!create} disabled={busy} onClick={() => setCreate(false)}>{t("existing")}</button>{canWrite && <button type="button" aria-pressed={create} disabled={busy} onClick={() => setCreate(true)}>{t("createTable")}</button>}</div>
      {create ? <><label>{t("name")}<input autoFocus value={name} maxLength={160} required disabled={busy} onChange={(event) => setName(event.target.value)} /></label><label>{t("description")}<textarea value={description} maxLength={4000} disabled={busy} onChange={(event) => setDescription(event.target.value)} /></label></> : !items.length ? <p>{t("emptyTables")}</p> : <><label>{t("selectTable")}<select value={selectedId} required disabled={busy} onChange={(event) => { setSelectedId(event.target.value); setDetail(null); setViewId(""); }}><option value="">—</option>{items.map((item) => <option key={item.id} value={item.id}>{item.icon} {item.name}</option>)}</select></label>{detail?.database.id === selectedId && <label>{t("chooseView")}<select value={viewId} disabled={busy} onChange={(event) => setViewId(event.target.value)}><option value="">{t("allRecords")}</option>{detail.views.map((view) => <option value={view.id} key={view.id}>{view.name}</option>)}</select></label>}</>}
      <footer><button type="submit" className="primary-button" disabled={busy || (create ? !name.trim() : !selectedId)}>{t(busy ? "saving" : create ? "createTable" : "insert")}</button></footer></>}
    {error && <div className="note-database-error" role="alert"><p>{error}</p><button type="button" disabled={busy} onClick={() => { setLoading(true); setRetry((value) => value + 1); }}>{t("retry")}</button></div>}
  </form></NoteDatabaseDialog>;
}
