import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import { useI18n } from "../i18n";
import type { NoteDatabaseEmbed, NoteDatabasesClient } from "../note-databases-api";
import { NoteDatabasePicker, NoteDatabaseTable } from "./NoteDatabaseTable";
import { NoteDatabaseSuspendedContext } from "./note-database-state";
import "./NoteDatabaseEmbeds.css";

const labels = {
  ru: { title: "Таблицы в заметке", add: "Вставить таблицу", remove: "Убрать из заметки", confirm: "Убрать таблицу из этой заметки? Сама таблица и записи сохранятся.", error: "Не удалось обновить таблицы. Повторите попытку.", retry: "Повторить", loading: "Загрузка таблиц…", up: "Выше", down: "Ниже", save: "Сохраните заметку, чтобы вставить таблицу." },
  en: { title: "Tables in this note", add: "Insert table", remove: "Remove from note", confirm: "Remove this table from this note? The table and its records will remain.", error: "Could not update tables. Please try again.", retry: "Retry", loading: "Loading tables…", up: "Move up", down: "Move down", save: "Save the note to insert a table." },
  ro: { title: "Tabele în notă", add: "Inserează un tabel", remove: "Elimină din notă", confirm: "Elimini tabelul din această notă? Tabelul și înregistrările vor fi păstrate.", error: "Tabelele nu au putut fi actualizate. Încearcă din nou.", retry: "Reîncearcă", loading: "Se încarcă tabelele…", up: "Mută în sus", down: "Mută în jos", save: "Salvează nota pentru a insera un tabel." },
};

interface Props {
  client: NoteDatabasesClient; noteId: string; canWrite: boolean; canManage: boolean;
  suspended?: boolean;
  onNoteChanged?: () => Promise<void>; onDirtyChange?: (dirty: boolean) => void; onBusyChange?: (busy: boolean) => void;
}

export function NoteDatabaseEmbeds({ client, noteId, canWrite, canManage, suspended = false, onNoteChanged, onDirtyChange, onBusyChange }: Props) {
  const { locale } = useI18n();
  const t = labels[locale];
  const [items, setItems] = useState<NoteDatabaseEmbed[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState(false);
  const [picker, setPicker] = useState(false);
  const [pickerDirty, setPickerDirty] = useState(false);
  const [pickerBusy, setPickerBusy] = useState(false);
  const [dirtyTables, setDirtyTables] = useState<Record<string, boolean>>({});
  const [busyTables, setBusyTables] = useState<Record<string, boolean>>({});
  const latestItems = useRef<NoteDatabaseEmbed[] | null>(null);
  const protectedTables = useRef<Record<string, boolean>>({});
  const dirty = pickerDirty || Object.values(dirtyTables).some(Boolean);
  const saving = busy || pickerBusy || Object.values(busyTables).some(Boolean);
  useEffect(() => { onDirtyChange?.(dirty); }, [dirty, onDirtyChange]);
  useEffect(() => { onBusyChange?.(saving); }, [saving, onBusyChange]);
  useEffect(() => () => { onDirtyChange?.(false); onBusyChange?.(false); }, [onDirtyChange, onBusyChange]);
  useEffect(() => {
    protectedTables.current = Object.fromEntries([...Object.keys(dirtyTables), ...Object.keys(busyTables)].map((id) => [id, !!dirtyTables[id] || !!busyTables[id]]));
    const latest = latestItems.current;
    if (latest) setItems((current) => retainDraftEmbeds(current, latest, protectedTables.current));
  }, [dirtyTables, busyTables]);
  useEffect(() => {
    const controller = new AbortController();
    client.listEmbeds(noteId, controller.signal).then((rows) => {
      if (!controller.signal.aborted) { latestItems.current = rows; setItems((current) => retainDraftEmbeds(current, rows, protectedTables.current)); setError(false); }
    }).catch(() => { if (!controller.signal.aborted) setError(true); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [client, noteId, reload]);

  async function mutate(action: () => Promise<unknown>, fromPicker = false) {
    if (busy || (!fromPicker && (saving || dirty)) || !canManage) return;
    setBusy(true); setError(false);
    try {
      await action();
      setPicker(false);
      try { await onNoteChanged?.(); }
      finally { setReload((value) => value + 1); }
    } catch (failure) { setError(true); if (fromPicker) throw failure; }
    finally { setBusy(false); }
  }

  return <NoteDatabaseSuspendedContext.Provider value={suspended}><section className="note-database-embeds" aria-label={t.title} hidden={suspended} inert={suspended}>
    {loading && <p role="status">{t.loading}</p>}
    {error && <p role="alert">{t.error} <button type="button" disabled={saving} onClick={() => setReload((value) => value + 1)}>{t.retry}</button></p>}
    {items.map((embed, index) => <EmbeddedTable key={embed.id} embed={embed} client={client} canWrite={canWrite && !busy} canManage={canManage && !busy}
      onDirty={(value) => setDirtyTables((old) => old[embed.id] === value ? old : { ...old, [embed.id]: value })}
      onBusy={(value) => setBusyTables((old) => old[embed.id] === value ? old : { ...old, [embed.id]: value })}>
      {canManage && <div className="note-database-embed-controls">
        <button type="button" disabled={saving || dirty || index === 0} aria-label={t.up} onClick={() => void mutate(() => client.updateEmbed(noteId, embed.id, { database_id: embed.database_id, view_id: embed.view_id, position: index - 1, expected_version: embed.version }))}><ArrowUp size={14} /></button>
        <button type="button" disabled={saving || dirty || index === items.length - 1} aria-label={t.down} onClick={() => void mutate(() => client.updateEmbed(noteId, embed.id, { database_id: embed.database_id, view_id: embed.view_id, position: index + 1, expected_version: embed.version }))}><ArrowDown size={14} /></button>
        <button type="button" disabled={saving || dirty} onClick={() => { if (window.confirm(t.confirm)) void mutate(() => client.deleteEmbed(noteId, embed.id, embed.version)); }}><Trash2 size={14} />{t.remove}</button>
      </div>}
    </EmbeddedTable>)}
    {canManage && !picker && <button className="notes-button" type="button" disabled={saving || dirty} onClick={() => setPicker(true)}><Plus size={15} />{t.add}</button>}
    {picker && <NoteDatabasePicker client={client} canWrite={canManage} onCancel={() => setPicker(false)} onDirtyChange={setPickerDirty} onBusyChange={setPickerBusy}
      onSelect={(databaseId, viewId) => mutate(() => client.createEmbed(noteId, { database_id: databaseId, view_id: viewId }), true)} />}
  </section></NoteDatabaseSuspendedContext.Provider>;
}

function retainDraftEmbeds(current: NoteDatabaseEmbed[], latest: NoteDatabaseEmbed[], protectedIds: Record<string, boolean>) {
  const retained = current.filter((embed) => protectedIds[embed.id]);
  const result = latest.map((embed) => retained.find((old) => old.id === embed.id) ?? embed);
  result.push(...retained.filter((old) => !latest.some((embed) => embed.id === old.id)));
  return result.length === current.length && result.every((embed, index) => embed === current[index]) ? current : result;
}

function EmbeddedTable({ embed, client, canWrite, canManage, onDirty, onBusy, children }: {
  embed: NoteDatabaseEmbed; client: NoteDatabasesClient; canWrite: boolean; canManage: boolean;
  onDirty: (value: boolean) => void; onBusy: (value: boolean) => void; children: React.ReactNode;
}) {
  // Stable callbacks keep a table's cleanup from resetting sibling dirty state on every render.
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => { onDirty(dirty); }, [dirty, onDirty]);
  useEffect(() => { onBusy(busy); }, [busy, onBusy]);
  const changeDirty = useCallback((value: boolean) => setDirty(value), []);
  const changeBusy = useCallback((value: boolean) => setBusy(value), []);
  return <div className="note-database-embed"><NoteDatabaseTable key={`${embed.database_id}:${embed.view_id ?? ""}`} client={client} databaseId={embed.database_id} initialViewId={embed.view_id ?? undefined}
    canWrite={canWrite} canManage={canManage} canEditFields={canWrite} onDirtyChange={changeDirty} onBusyChange={changeBusy} />{children}</div>;
}
