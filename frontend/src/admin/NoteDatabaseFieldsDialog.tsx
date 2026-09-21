import { useEffect, useState, type FormEvent } from "react";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import type { CreateField, FieldType, NoteDatabase, NoteDatabaseClient, NoteDatabaseDetail, NoteDatabaseField, RelationCardinality, RollupOperation, SelectOption } from "../note-databases-api";
import { useI18n } from "../i18n";
import { NoteDatabaseDialog } from "./NoteDatabaseDialog";
import { moveItem } from "./NoteDatabaseUtils";
import { noteDatabaseError, noteDatabasesText } from "./note-databases-i18n";

const TYPES: FieldType[] = ["text", "long_text", "number", "date", "boolean", "single_select", "multi_select", "url", "email", "relation", "rollup"];
export function NoteDatabaseFieldsDialog({ client, detail, canReorder = true, onClose, onChange, onDirtyChange, onBusyChange }: {
  client: NoteDatabaseClient; detail: NoteDatabaseDetail; canReorder?: boolean; onClose: () => void; onChange: () => void; onDirtyChange?: (dirty: boolean) => void; onBusyChange?: (busy: boolean) => void;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  const [editing, setEditing] = useState<string | null>(null); const [busy, setBusy] = useState(false); const [dirty, setDirty] = useState(false); const [error, setError] = useState("");
  async function reorder(index: number, offset: number) {
    if (!canReorder) return;
    setBusy(true); setError("");
    try { await client.reorderFields(detail.database.id, moveItem(detail.fields, index, offset).map((field) => field.id), detail.database.version); onChange(); }
    catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  async function remove(field: NoteDatabaseField) {
    if (!window.confirm(t("deleteFieldConfirm"))) return;
    setBusy(true); setError("");
    try { await client.deleteField(detail.database.id, field.id, field.version); if (editing === field.id) setEditing(null); onChange(); }
    catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  function select(id: string) { if (!dirty || window.confirm(t("discardConfirm"))) { setEditing(id); setDirty(false); } }
  return <NoteDatabaseDialog title={t("fields")} onClose={onClose} dirty={dirty} busy={busy} onDirtyChange={onDirtyChange} onBusyChange={onBusyChange} wide>
    <ul className="note-database-management-list">{detail.fields.map((field, index) => <li key={field.id}><button type="button" aria-label={`${t("editField")}: ${field.name}`} disabled={busy} onClick={() => select(field.id)}><strong>{field.name}</strong><span>{t(field.field_type)}</span></button>
      {canReorder && <><button type="button" aria-label={`${t("moveUp")}: ${field.name}`} disabled={busy || index === 0} onClick={() => void reorder(index, -1)}><ArrowUp size={15} /></button>
      <button type="button" aria-label={`${t("moveDown")}: ${field.name}`} disabled={busy || index === detail.fields.length - 1} onClick={() => void reorder(index, 1)}><ArrowDown size={15} /></button></>}
      <button type="button" aria-label={`${t("deleteField")}: ${field.name}`} disabled={busy} onClick={() => void remove(field)}><Trash2 size={15} /></button></li>)}</ul>
    <button type="button" className="secondary-button" disabled={busy} onClick={() => select("new")}><Plus size={15} />{t("addField")}</button>
    {error && <p className="note-database-error" role="alert">{error}</p>}
    {editing && <FieldForm key={editing} client={client} detail={detail} current={detail.fields.find((field) => field.id === editing)} onBusyChange={setBusy} onDirtyChange={setDirty}
      onSaved={() => { setEditing(null); setDirty(false); onChange(); }} />}
  </NoteDatabaseDialog>;
}

function FieldForm({ client, detail, current, onSaved, onBusyChange, onDirtyChange }: {
  client: NoteDatabaseClient; detail: NoteDatabaseDetail; current?: NoteDatabaseField; onSaved: () => void; onBusyChange: (busy: boolean) => void; onDirtyChange: (dirty: boolean) => void;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  const [name, setName] = useState(current?.name ?? ""); const [type, setType] = useState<FieldType>(current?.field_type ?? "text");
  const [options, setOptions] = useState<SelectOption[]>(current?.config.options ?? []);
  const [databases, setDatabases] = useState<NoteDatabase[]>([detail.database]); const [targetId, setTargetId] = useState(current?.relation_target_database_id ?? detail.database.id);
  const [cardinality, setCardinality] = useState<RelationCardinality>(current?.relation_cardinality ?? "many_to_many"); const [inverse, setInverse] = useState(t("inverseDefault"));
  const relationFields = detail.fields.filter((field) => field.field_type === "relation");
  const [relationId, setRelationId] = useState(current?.rollup_relation_field_id ?? relationFields[0]?.id ?? "");
  const [operation, setOperation] = useState<RollupOperation>(current?.rollup_operation ?? "count"); const [targetFieldId, setTargetFieldId] = useState(current?.rollup_target_field_id ?? "");
  const [targetFields, setTargetFields] = useState<NoteDatabaseField[]>([]); const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  const rollupTargetId = relationFields.find((field) => field.id === relationId)?.relation_target_database_id;
  const [baseline] = useState(() => JSON.stringify([name, type, options, targetId, cardinality, inverse, relationId, operation, targetFieldId]));
  const dirty = JSON.stringify([name, type, options, targetId, cardinality, inverse, relationId, operation, targetFieldId]) !== baseline;
  useEffect(() => { onDirtyChange(dirty); return () => onDirtyChange(false); }, [dirty, onDirtyChange]);
  useEffect(() => {
    const controller = new AbortController(); client.listDatabases(controller.signal).then((items) => { if (!controller.signal.aborted) setDatabases(items); }).catch(() => { if (!controller.signal.aborted) setError(noteDatabasesText(locale)("loadError")); });
    return () => controller.abort();
  }, [client, locale]);
  useEffect(() => {
    if (type !== "rollup" || !rollupTargetId) return;
    const controller = new AbortController(); client.getDatabase(rollupTargetId, controller.signal).then((value) => { if (!controller.signal.aborted) setTargetFields(value.fields); }).catch(() => { if (!controller.signal.aborted) setError(noteDatabasesText(locale)("loadError")); });
    return () => controller.abort();
  }, [client, rollupTargetId, type, locale]);
  const selectType = type === "single_select" || type === "multi_select";
  const fixed = !!current && (current.field_type === "relation" || current.field_type === "rollup");
  const valid = name.trim() && (!selectType || options.every((option) => option.label.trim()))
    && (type !== "relation" || (targetId && inverse.trim())) && (type !== "rollup" || (relationId && (operation === "count" || targetFieldId)));
  async function submit(event: FormEvent) {
    event.preventDefault(); event.stopPropagation(); if (!valid || busy) return; setBusy(true); onBusyChange(true); setError("");
    const config = { ...(current?.field_type === type ? current.config : {}), ...(selectType ? { options: options.map((option) => ({ ...option, label: option.label.trim() })) } : {}) };
    const input: CreateField = { name: name.trim(), field_type: type, config };
    if (!current && type === "relation" && cardinality !== "many_to_one") input.relation = { target_database_id: targetId, cardinality, inverse_name: inverse.trim() };
    if (type === "rollup") input.rollup = { relation_field_id: relationId, operation, target_field_id: operation === "count" ? null : targetFieldId };
    try {
      if (current) await client.updateField(detail.database.id, current.id, { name: input.name, field_type: type, config, expected_version: current.version });
      else await client.createField(detail.database.id, input);
      onSaved();
    } catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); onBusyChange(false); }
  }
  return <form className="note-database-form note-database-subform" onSubmit={(event) => void submit(event)}><h3>{t(current ? "editField" : "addField")}</h3>
    <label>{t("name")}<input autoFocus value={name} maxLength={160} disabled={busy} required onChange={(event) => setName(event.target.value)} /></label>
    <label>{t("type")}<select value={type} disabled={busy || fixed} onChange={(event) => setType(event.target.value as FieldType)}>{TYPES.filter((item) => !current || fixed || (item !== "relation" && item !== "rollup")).map((item) => <option value={item} key={item}>{t(item)}</option>)}</select></label>
    {selectType && <fieldset><legend>{t("options")}</legend>{options.map((option, index) => <div className="note-database-inline" key={option.id}><input aria-label={`${t("optionLabel")} ${index + 1}`} value={option.label} disabled={busy} required onChange={(event) => setOptions((items) => items.map((item) => item.id === option.id ? { ...item, label: event.target.value } : item))} /><button type="button" aria-label={`${t("remove")} ${index + 1}`} disabled={busy} onClick={() => setOptions((items) => items.filter((item) => item.id !== option.id))}><Trash2 size={14} /></button></div>)}
      <button type="button" disabled={busy} onClick={() => setOptions((items) => [...items, { id: crypto.randomUUID(), label: "" }])}>{t("addOption")}</button></fieldset>}
    {type === "relation" && <><label>{t("targetDatabase")}<select value={targetId} disabled={busy || !!current} onChange={(event) => setTargetId(event.target.value)}>{databases.map((db) => <option value={db.id} key={db.id}>{db.name}</option>)}</select></label>
      <label>{t("cardinality")}<select value={cardinality} disabled={busy || !!current} onChange={(event) => setCardinality(event.target.value as RelationCardinality)}>{current?.relation_cardinality === "many_to_one" && <option value="many_to_one">{t("many_to_one")}</option>}{(["one_to_one", "one_to_many", "many_to_many"] as const).map((item) => <option key={item} value={item}>{t(item)}</option>)}</select></label>
      {!current && <label>{t("inverseName")}<input value={inverse} maxLength={160} disabled={busy} onChange={(event) => setInverse(event.target.value)} /></label>}</>}
    {type === "rollup" && <><label>{t("relationField")}<select value={relationId} disabled={busy || !!current} onChange={(event) => { setRelationId(event.target.value); setTargetFieldId(""); }}><option value="">—</option>{relationFields.map((field) => <option key={field.id} value={field.id}>{field.name}</option>)}</select></label>
      <label>{t("operation")}<select value={operation} disabled={busy || !!current} onChange={(event) => { setOperation(event.target.value as RollupOperation); setTargetFieldId(""); }}>{(["count", "sum", "min", "max", "show_values"] as const).map((op) => <option key={op} value={op}>{t(op)}</option>)}</select></label>
      {operation !== "count" && <label>{t("targetField")}<select value={targetFieldId} disabled={busy || !!current} onChange={(event) => setTargetFieldId(event.target.value)}><option value="">—</option>{targetFields.filter((field) => operation === "show_values" ? field.field_type !== "relation" && field.field_type !== "rollup" : field.field_type === "number" || (operation !== "sum" && field.field_type === "date")).map((field) => <option key={field.id} value={field.id}>{field.name}</option>)}</select></label>}</>}
    {fixed && <p className="note-database-help">{t("structuralFixed")}</p>}{error && <p className="note-database-error" role="alert">{error}</p>}
    <footer><button type="submit" className="primary-button" disabled={busy || !valid}>{t(busy ? "saving" : "save")}</button></footer>
  </form>;
}
