import { lazy, Suspense, useEffect, useRef, useState, type FormEvent } from "react";
import type { NoteDatabaseClient, NoteDatabaseDetail, NoteDatabaseField, NoteDatabaseRecord, RecordInput } from "../note-databases-api";
import { useI18n } from "../i18n";
import { NoteDatabaseDialog } from "./NoteDatabaseDialog";
import { NoteDatabaseValueEditor } from "./NoteDatabaseValueEditor";
import { recordLabel } from "./NoteDatabaseUtils";
import { noteDatabaseError, noteDatabasesText } from "./note-databases-i18n";

const NoteEditor = lazy(() => import("./NoteEditor"));
export function NoteDatabaseRecordDialog({ client, databaseId, recordId, fields, fieldId, canWrite, onClose, onSaved, onDirtyChange, onBusyChange }: {
  client: NoteDatabaseClient; databaseId: string; recordId: string; fields: NoteDatabaseField[]; fieldId?: string; canWrite: boolean;
  onClose: () => void; onSaved: () => void; onDirtyChange?: (dirty: boolean) => void; onBusyChange?: (busy: boolean) => void;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  const [record, setRecord] = useState<NoteDatabaseRecord | null>(null);
  const [draft, setDraft] = useState<RecordInput | null>(null); const [baseline, setBaseline] = useState("");
  const [loading, setLoading] = useState(true); const [failed, setFailed] = useState(false); const [retry, setRetry] = useState(0);
  const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  const [related, setRelated] = useState<{ detail: NoteDatabaseDetail; id: string } | null>(null);
  const [relatedDirty, setRelatedDirty] = useState(false); const [relatedBusy, setRelatedBusy] = useState(false);
  const dirty = !!draft && JSON.stringify(draft) !== baseline;
  const dirtyRef = useRef(dirty);
  useEffect(() => { dirtyRef.current = dirty; }, [dirty]);
  useEffect(() => {
    const controller = new AbortController();
    client.getRecord(databaseId, recordId, controller.signal).then((item) => {
      if (controller.signal.aborted) return;
      if (dirtyRef.current) { setFailed(false); return; }
      const input = { values: item.values, content_markdown: item.content_markdown ?? "" };
      setRecord(item); setDraft(input); setBaseline(JSON.stringify(input)); setFailed(false);
    }).catch(() => { if (!controller.signal.aborted) setFailed(true); }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [client, databaseId, recordId, retry]);
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); event.stopPropagation();
    if (!canWrite || busy || !record || !draft || !dirty || [...draft.content_markdown].length > 200_000 || !event.currentTarget.checkValidity()) return;
    setBusy(true); setError("");
    try {
      const values = Object.fromEntries(fields.filter((field) => field.field_type !== "rollup").map((field) => [field.id, draft.values[field.id] ?? null]));
      const saved = await client.updateRecord(databaseId, recordId, { values, content_markdown: draft.content_markdown, expected_version: record.version });
      const input = { values: saved.values, content_markdown: saved.content_markdown ?? "" };
      setRecord(saved); setDraft(input); setBaseline(JSON.stringify(input)); onSaved();
    } catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  async function remove() {
    if (!canWrite || busy || !record || !window.confirm(t("deleteRecordConfirm"))) return;
    setBusy(true); setError("");
    try { await client.deleteRecord(databaseId, record.id, record.version); onSaved(); onClose(); }
    catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  const selectedField = fields.find((field) => field.id === fieldId);
  return <NoteDatabaseDialog title={selectedField ? t("editCell", { field: selectedField.name }) : record ? recordLabel({ ...record, values: draft?.values ?? record.values }, fields, locale) : t("record")}
    onClose={onClose} dirty={dirty || relatedDirty} busy={busy || relatedBusy} onDirtyChange={onDirtyChange} onBusyChange={onBusyChange} wide={!fieldId} suspended={!!related}>
    {loading ? <p role="status">{t("loading")}</p> : failed || !record || !draft ? <div role="alert"><p>{t("loadError")}</p><button type="button" onClick={() => { setLoading(true); setRetry((v) => v + 1); }}>{t("retry")}</button></div> :
      <form className="note-database-form" onSubmit={(event) => void save(event)} onKeyDown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === "s") { event.preventDefault(); event.stopPropagation(); event.currentTarget.requestSubmit(); } }}>
        {error && <p className="note-database-error" role="alert">{error}</p>}
        {(fieldId ? fields.filter((field) => field.id === fieldId) : fields).map((field) => <div className="note-database-field" key={field.id}><span>{field.name}</span>
          <NoteDatabaseValueEditor client={client} field={field} value={draft.values[field.id]} disabled={!canWrite || busy} currentRecordId={recordId} onOpenRelated={busy ? undefined : (detail, id) => setRelated({ detail, id })} onChange={(value) => setDraft((current) => current ? { ...current, values: { ...current.values, [field.id]: value } } : current)} />
        </div>)}
        {!fieldId && <section className="note-database-record-body"><h3>{t("recordBody")}</h3><Suspense fallback={<p>{t("loading")}</p>}><NoteEditor value={draft.content_markdown} readOnly={!canWrite || busy} onChange={(content_markdown) => setDraft((current) => current ? { ...current, content_markdown } : current)} /></Suspense></section>}
        {[...draft.content_markdown].length > 200_000 && <p className="note-database-error" role="alert">{t("tooLarge")}</p>}
        <footer>{canWrite && !fieldId && <button type="button" className="note-database-danger" disabled={busy} onClick={() => void remove()}>{t("deleteRecord")}</button>}
          {canWrite && <button type="submit" className="primary-button" disabled={busy || !dirty || [...draft.content_markdown].length > 200_000}>{t(busy ? "saving" : "save")}</button>}
        </footer>
      </form>}
    {related && <NoteDatabaseRecordDialog client={client} databaseId={related.detail.database.id} recordId={related.id} fields={related.detail.fields} canWrite={canWrite} onClose={() => setRelated(null)} onSaved={() => { onSaved(); if (!dirty) setRetry((value) => value + 1); }} onDirtyChange={setRelatedDirty} onBusyChange={setRelatedBusy} />}
  </NoteDatabaseDialog>;
}
