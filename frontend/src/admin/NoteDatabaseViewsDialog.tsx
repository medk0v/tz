import { useEffect, useState, type FormEvent } from "react";
import { ArrowDown, ArrowUp, Copy, Plus, Trash2 } from "lucide-react";
import type { JsonValue, NoteDatabaseClient, NoteDatabaseDetail, NoteDatabaseField, NoteDatabaseView, ViewConfig, ViewFilter } from "../note-databases-api";
import { useI18n } from "../i18n";
import { NoteDatabaseDialog } from "./NoteDatabaseDialog";
import { NoteDatabaseValueEditor } from "./NoteDatabaseValueEditor";
import { moveItem, normalizeView } from "./NoteDatabaseUtils";
import { noteDatabaseError, noteDatabasesText } from "./note-databases-i18n";

const OPERATORS = ["equals", "not_equals", "contains", "greater_than", "less_than", "is_empty", "is_not_empty"] as const;
export function NoteDatabaseViewsDialog({ client, detail, initialViewId, onClose, onChange, onSelect, onDirtyChange, onBusyChange }: {
  client: NoteDatabaseClient; detail: NoteDatabaseDetail; initialViewId?: string; onClose: () => void; onChange: () => void;
  onSelect: (id: string) => void; onDirtyChange?: (dirty: boolean) => void; onBusyChange?: (busy: boolean) => void;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  const [editing, setEditing] = useState(initialViewId ?? "new"); const [copy, setCopy] = useState(false);
  const [busy, setBusy] = useState(false); const [dirty, setDirty] = useState(false); const [error, setError] = useState("");
  function select(id: string, duplicate = false) { if (!dirty || window.confirm(t("discardConfirm"))) { setEditing(id); setCopy(duplicate); setDirty(false); } }
  async function reorder(index: number, offset: number) {
    setBusy(true); setError("");
    try { await client.reorderViews(detail.database.id, moveItem(detail.views, index, offset).map((view) => view.id), detail.database.version); onChange(); }
    catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  async function remove(view: NoteDatabaseView) {
    if (!window.confirm(t("deleteViewConfirm"))) return;
    setBusy(true); setError("");
    try { await client.deleteView(detail.database.id, view.id, view.version); if (editing === view.id) setEditing("new"); if (initialViewId === view.id) onSelect(""); onChange(); }
    catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); }
  }
  return <NoteDatabaseDialog title={t("views")} onClose={onClose} dirty={dirty} busy={busy} onDirtyChange={onDirtyChange} onBusyChange={onBusyChange} wide>
    <ul className="note-database-management-list">{detail.views.map((view, index) => <li key={view.id}><button type="button" disabled={busy} onClick={() => select(view.id)}><strong>{view.name}</strong></button>
      <button type="button" aria-label={`${t("duplicateView")}: ${view.name}`} disabled={busy} onClick={() => select(view.id, true)}><Copy size={14} /></button>
      <button type="button" aria-label={`${t("moveUp")}: ${view.name}`} disabled={busy || index === 0} onClick={() => void reorder(index, -1)}><ArrowUp size={14} /></button>
      <button type="button" aria-label={`${t("moveDown")}: ${view.name}`} disabled={busy || index === detail.views.length - 1} onClick={() => void reorder(index, 1)}><ArrowDown size={14} /></button>
      <button type="button" aria-label={`${t("deleteView")}: ${view.name}`} disabled={busy} onClick={() => void remove(view)}><Trash2 size={14} /></button></li>)}</ul>
    <button type="button" className="secondary-button" disabled={busy} onClick={() => select("new")}><Plus size={15} />{t("addView")}</button>
    {error && <p className="note-database-error" role="alert">{error}</p>}
    <ViewForm key={`${editing}:${copy}`} client={client} detail={detail} current={detail.views.find((view) => view.id === editing)} duplicate={copy}
      onBusyChange={setBusy} onDirtyChange={setDirty} onSaved={(id) => { onSelect(id); onChange(); setDirty(false); onClose(); }} />
  </NoteDatabaseDialog>;
}

function ViewForm({ client, detail, current, duplicate, onBusyChange, onDirtyChange, onSaved }: {
  client: NoteDatabaseClient; detail: NoteDatabaseDetail; current?: NoteDatabaseView; duplicate: boolean; onBusyChange: (busy: boolean) => void;
  onDirtyChange: (dirty: boolean) => void; onSaved: (id: string) => void;
}) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  const [name, setName] = useState(current ? `${current.name}${duplicate ? t("copySuffix") : ""}`.slice(0, 160) : "");
  const [config, setConfig] = useState(() => normalizeView(current?.config, detail.fields));
  const [baseline, setBaseline] = useState(() => JSON.stringify({ name, config }));
  const [busy, setBusy] = useState(false); const [error, setError] = useState("");
  const dirty = JSON.stringify({ name, config }) !== baseline;
  useEffect(() => { onDirtyChange(dirty); return () => onDirtyChange(false); }, [dirty, onDirtyChange]);
  function patch(value: Partial<ViewConfig>) { setConfig((current) => ({ ...current, ...value })); }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); event.stopPropagation(); if (!name.trim() || busy || !event.currentTarget.checkValidity()) return;
    setBusy(true); onBusyChange(true); setError("");
    try {
      const saved = current && !duplicate ? await client.updateView(detail.database.id, current.id, { name: name.trim(), config, expected_version: current.version }) : await client.createView(detail.database.id, { name: name.trim(), config });
      setBaseline(JSON.stringify({ name, config })); onSaved(saved.id);
    } catch (failure) { setError(noteDatabaseError(locale, failure)); } finally { setBusy(false); onBusyChange(false); }
  }
  return <form className="note-database-form note-database-subform" onSubmit={(event) => void save(event)}>
    <h3>{t(current && !duplicate ? "editView" : "addView")}</h3><label>{t("name")}<input value={name} maxLength={160} required disabled={busy} onChange={(event) => setName(event.target.value)} /></label>
    <fieldset><legend>{t("filters")}</legend>{config.filters.map((filter, index) => {
      const field = detail.fields.find((field) => field.id === filter.field_id);
      return <div className="note-database-rule" key={index}><select aria-label={`${t("fields")} ${index + 1}`} value={filter.field_id} disabled={busy} onChange={(event) => { const next = detail.fields.find((item) => item.id === event.target.value); patch({ filters: config.filters.map((value, i) => i === index ? { field_id: event.target.value, operator: "equals", value: defaultFilterValue(next) } : value) }); }}>{detail.fields.map((field) => <option key={field.id} value={field.id}>{field.name}</option>)}</select>
        <select aria-label={`${t("operation")} ${index + 1}`} value={filter.operator} disabled={busy} onChange={(event) => patch({ filters: config.filters.map((value, i) => i === index ? { ...value, operator: event.target.value, value: event.target.value.startsWith("is_") ? null : defaultFilterValue(field, event.target.value) } : value) })}>{operatorsFor(field).map((op) => <option key={op} value={op}>{t(op)}</option>)}</select>
        {field && !filter.operator.startsWith("is_") && <FilterValue client={client} field={field} filter={filter} disabled={busy} onChange={(value) => patch({ filters: config.filters.map((item, i) => i === index ? { ...item, value } : item) })} />}
        <button type="button" aria-label={`${t("remove")} ${t("filters")} ${index + 1}`} disabled={busy} onClick={() => patch({ filters: config.filters.filter((_, i) => i !== index) })}><Trash2 size={14} /></button></div>;
    })}<button type="button" disabled={busy || !detail.fields.length || config.filters.length >= 50} onClick={() => patch({ filters: [...config.filters, { field_id: detail.fields[0].id, operator: "equals", value: defaultFilterValue(detail.fields[0]) }] })}>{t("addFilter")}</button></fieldset>
    <fieldset><legend>{t("sorts")}</legend>{config.sorts.map((sort, index) => <div className="note-database-rule" key={index}>
      <select aria-label={`${t("sorts")} ${index + 1}`} value={sort.field_id} disabled={busy} onChange={(event) => patch({ sorts: config.sorts.map((value, i) => i === index ? { ...value, field_id: event.target.value } : value) })}>{detail.fields.filter((field) => field.id === sort.field_id || !config.sorts.some((sort) => sort.field_id === field.id)).map((field) => <option key={field.id} value={field.id}>{field.name}</option>)}</select>
      <select aria-label={`${t("asc")} / ${t("desc")}`} value={sort.direction} disabled={busy} onChange={(event) => patch({ sorts: config.sorts.map((value, i) => i === index ? { ...value, direction: event.target.value as "asc" | "desc" } : value) })}><option value="asc">{t("asc")}</option><option value="desc">{t("desc")}</option></select>
      <button type="button" aria-label={`${t("moveUp")} ${t("sorts")} ${index + 1}`} disabled={busy || index === 0} onClick={() => patch({ sorts: moveItem(config.sorts, index, -1) })}><ArrowUp size={14} /></button>
      <button type="button" aria-label={`${t("remove")} ${t("sorts")} ${index + 1}`} disabled={busy} onClick={() => patch({ sorts: config.sorts.filter((_, i) => i !== index) })}><Trash2 size={14} /></button></div>)}
      <button type="button" disabled={busy || config.sorts.length >= Math.min(5, detail.fields.length)} onClick={() => { const field = detail.fields.find((field) => !config.sorts.some((sort) => sort.field_id === field.id)); if (field) patch({ sorts: [...config.sorts, { field_id: field.id, direction: "asc" }] }); }}>{t("addSort")}</button></fieldset>
    <fieldset><legend>{t("columns")}</legend><div className="note-database-column-settings">{config.column_order.map((id, index) => {
      const field = detail.fields.find((field) => field.id === id); if (!field) return null;
      return <div key={id}><strong>{field.name}</strong><label><input type="checkbox" checked={!config.hidden_fields.includes(id)} disabled={busy} onChange={(event) => patch({ hidden_fields: event.target.checked ? config.hidden_fields.filter((field) => field !== id) : [...config.hidden_fields, id], pinned_fields: event.target.checked ? config.pinned_fields : config.pinned_fields.filter((field) => field !== id) })} />{t("visible")}</label>
        <label><input type="checkbox" checked={config.pinned_fields.includes(id)} disabled={busy || config.hidden_fields.includes(id)} onChange={(event) => patch({ pinned_fields: event.target.checked ? [...config.pinned_fields, id] : config.pinned_fields.filter((field) => field !== id) })} />{t("pinned")}</label>
        <label>{t("width")}<input type="number" min={64} max={1000} aria-label={`${t("width")}: ${field.name}`} value={config.column_widths[id] ?? 180} disabled={busy} onChange={(event) => patch({ column_widths: { ...config.column_widths, [id]: Number(event.target.value) } })} /></label>
        <button type="button" aria-label={`${t("moveUp")}: ${field.name}`} disabled={busy || index === 0} onClick={() => patch({ column_order: moveItem(config.column_order, index, -1) })}><ArrowUp size={14} /></button>
        <button type="button" aria-label={`${t("moveDown")}: ${field.name}`} disabled={busy || index === config.column_order.length - 1} onClick={() => patch({ column_order: moveItem(config.column_order, index, 1) })}><ArrowDown size={14} /></button></div>;
    })}</div></fieldset>
    {error && <p role="alert" className="note-database-error">{error}</p>}<footer><button type="submit" className="primary-button" disabled={busy || !name.trim()}>{t(busy ? "saving" : "save")}</button></footer>
  </form>;
}

function operatorsFor(field?: NoteDatabaseField) {
  return OPERATORS.filter((operator) => {
    if (operator === "contains") return !!field && ["text", "long_text", "url", "email", "single_select", "multi_select", "relation"].includes(field.field_type);
    if (operator === "greater_than" || operator === "less_than") return field?.field_type === "number" || field?.field_type === "date" || (field?.field_type === "rollup" && field.rollup_operation !== "show_values");
    return true;
  });
}
function defaultFilterValue(field?: NoteDatabaseField, operator = "equals"): JsonValue {
  if (operator === "contains") return "";
  if (field?.field_type === "boolean") return false;
  if (field?.field_type === "number" || (field?.field_type === "rollup" && ["sum", "count"].includes(field.rollup_operation ?? ""))) return 0;
  if (field?.field_type === "multi_select" || field?.field_type === "relation" || (field?.field_type === "rollup" && field.rollup_operation === "show_values")) return [];
  return "";
}
function FilterValue({ client, field, filter, disabled, onChange }: { client: NoteDatabaseClient; field: NoteDatabaseField; filter: ViewFilter; disabled: boolean; onChange: (value: JsonValue) => void }) {
  const { locale } = useI18n(); const t = noteDatabasesText(locale);
  if (field.field_type === "rollup") return <input aria-label={t("filterValue")} disabled={disabled} type={["sum", "count"].includes(field.rollup_operation ?? "") ? "number" : "text"} step="any" value={typeof filter.value === "object" ? JSON.stringify(filter.value) : String(filter.value ?? "")} onChange={(event) => { const raw = event.target.value; try { onChange(JSON.parse(raw) as JsonValue); } catch { onChange(raw); } }} />;
  if (filter.operator === "contains" && field.field_type === "relation") return <NoteDatabaseValueEditor client={client} field={{ ...field, name: t("filterValue"), relation_cardinality: "one_to_one" }} value={typeof filter.value === "string" && filter.value ? [filter.value] : []} disabled={disabled} onChange={(value) => onChange(Array.isArray(value) ? value[0] ?? "" : "")} />;
  const fieldType = filter.operator === "contains" && ["url", "email"].includes(field.field_type) ? "text" : filter.operator === "contains" && field.field_type === "multi_select" ? "single_select" : field.field_type;
  return <NoteDatabaseValueEditor client={client} field={{ ...field, name: t("filterValue"), field_type: fieldType }} value={filter.value} disabled={disabled} onChange={onChange} />;
}
