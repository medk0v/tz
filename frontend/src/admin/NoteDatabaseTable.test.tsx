import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createI18n, I18nContext, type Locale } from "../i18n";
import type { NoteDatabaseClient, NoteDatabaseDetail, NoteDatabaseField, NoteDatabaseRecord, NoteDatabaseView } from "../note-databases-api";
import { NoteDatabasePicker, NoteDatabaseTable } from "./NoteDatabaseTable";
import { NoteDatabaseRecordDialog } from "./NoteDatabaseRecordDialog";
import { NoteDatabaseFieldsDialog } from "./NoteDatabaseFieldsDialog";
import { NoteDatabaseViewsDialog } from "./NoteDatabaseViewsDialog";
import { NoteDatabaseSuspendedContext } from "./note-database-state";

vi.mock("./NoteEditor", () => ({ default: ({ value, onChange, readOnly }: { value: string; onChange: (value: string) => void; readOnly: boolean }) => <textarea aria-label="Record Markdown" value={value} readOnly={readOnly} onChange={(event) => onChange(event.target.value)} /> }));
const database = { id: "db-a", project_id: "project", name: "Launch tasks", description: "Release tracking", icon: "", version: 8, created_at: "2026-09-21", updated_at: "2026-09-21" };
function field(id: string, name: string, field_type: NoteDatabaseField["field_type"], extra: Partial<NoteDatabaseField> = {}): NoteDatabaseField {
  return { id, name, field_type, database_id: database.id, position: 0, version: 3, config: {}, relation_target_database_id: null, relation_cardinality: null, relation_owner_field_id: null, inverse_field_id: null, rollup_relation_field_id: null, rollup_target_field_id: null, rollup_operation: null, created_at: "2026-09-21", updated_at: "2026-09-21", ...extra };
}
const titleField = field("title", "Title", "text"); const numberField = field("effort", "Effort", "number", { position: 1 });
const tagsField = field("tags", "Tags", "multi_select", { position: 2, config: { options: [{ id: "option-a", label: "Ready" }, { id: "option-b", label: "Blocked" }] } });
const rollupField = field("total", "Total", "rollup", { position: 3, rollup_operation: "sum" });
const relationField = field("related", "Related", "relation", { relation_target_database_id: "db-b", relation_cardinality: "many_to_one", relation_owner_field_id: "owner" });
const row: NoteDatabaseRecord = { id: "row-a", database_id: database.id, position: 0, version: 4, values: { title: "Publish app", effort: 3, tags: ["option-a"], total: 12 }, content_markdown: "## Acceptance\n- [ ] Checked", created_at: "2026-09-21", updated_at: "2026-09-21" };
const rowB: NoteDatabaseRecord = { ...row, id: "row-b", position: 1, values: { title: "Tell users", effort: 1 } };
const view: NoteDatabaseView = { id: "view-a", database_id: database.id, name: "Ready work", position: 0, version: 5, created_at: "2026-09-21", updated_at: "2026-09-21", config: { filters: [], sorts: [], hidden_fields: ["tags"], pinned_fields: ["effort"], column_order: ["title", "effort", "tags", "total"], column_widths: { effort: 140 } } };
const detail: NoteDatabaseDetail = { database, fields: [titleField, numberField, tagsField, rollupField], views: [view] };
function makeClient(data = detail): NoteDatabaseClient {
  return {
    listDatabases: vi.fn().mockResolvedValue([data.database]), getDatabase: vi.fn().mockResolvedValue(data), createDatabase: vi.fn().mockResolvedValue({ ...database, id: "new-db" }), updateDatabase: vi.fn().mockResolvedValue(database), deleteDatabase: vi.fn().mockResolvedValue(undefined),
    listRecords: vi.fn().mockResolvedValue({ items: [row, rowB], total: 2, page: 1, per_page: 50 }), getRecord: vi.fn().mockResolvedValue(row), createRecord: vi.fn().mockResolvedValue({ ...row, id: "new-row" }), updateRecord: vi.fn().mockImplementation(async (_db, id, input) => ({ ...row, ...input, id, version: 5 })), deleteRecord: vi.fn().mockResolvedValue(undefined), reorderRecords: vi.fn().mockResolvedValue(undefined),
    createField: vi.fn().mockResolvedValue(titleField), updateField: vi.fn().mockResolvedValue(titleField), deleteField: vi.fn().mockResolvedValue(undefined), reorderFields: vi.fn().mockResolvedValue(undefined),
    createView: vi.fn().mockResolvedValue({ ...view, id: "new-view" }), updateView: vi.fn().mockResolvedValue(view), deleteView: vi.fn().mockResolvedValue(undefined), reorderViews: vi.fn().mockResolvedValue(undefined),
    listEmbeds: vi.fn().mockResolvedValue([]), createEmbed: vi.fn(), updateEmbed: vi.fn(), deleteEmbed: vi.fn(),
  };
}
function wrapper(locale: Locale = "en") { return ({ children }: { children: React.ReactNode }) => <I18nContext.Provider value={createI18n(locale, vi.fn())}>{children}</I18nContext.Provider>; }
function table(client: NoteDatabaseClient, props: Partial<React.ComponentProps<typeof NoteDatabaseTable>> = {}) { return render(<NoteDatabaseTable client={client} databaseId={database.id} canWrite {...props} />, { wrapper: wrapper() }); }
async function loaded() { await waitFor(() => expect(screen.getByRole("button", { name: "Refresh table" })).toBeEnabled()); }
function recordDialog(client: NoteDatabaseClient, props: Partial<React.ComponentProps<typeof NoteDatabaseRecordDialog>> = {}) { return render(<NoteDatabaseRecordDialog client={client} databaseId={database.id} recordId={row.id} fields={detail.fields} canWrite onClose={vi.fn()} onSaved={vi.fn()} {...props} />, { wrapper: wrapper() }); }

beforeEach(() => { vi.spyOn(window, "confirm").mockReturnValue(true); });
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe("note database table", () => {
  it("applies a saved view's hidden, ordered, sized and pinned columns", async () => {
    const client = makeClient(); table(client, { initialViewId: view.id }); await loaded();
    expect(client.listRecords).toHaveBeenCalledWith(database.id, expect.objectContaining({ view_id: view.id, per_page: 50 }), expect.any(AbortSignal));
    const headings = screen.getAllByRole("columnheader"); expect(headings[0]).toHaveTextContent("Effort"); expect(headings[0]).toHaveStyle({ width: "140px", position: "sticky", left: "0px" });
    expect(headings.map((heading) => heading.textContent).join(" ")).not.toContain("Tags");
    expect(screen.queryByRole("button", { name: "Move up: Tell users" })).not.toBeInTheDocument();
  });
  it("recovers a deleted initial view and still loads all records", async () => {
    const client = makeClient({ ...detail, views: [] }); vi.mocked(client.listRecords).mockRejectedValueOnce({ status: 404 });
    table(client, { initialViewId: "removed-view" }); await screen.findByRole("button", { name: "Open record: Publish app" });
    expect(screen.getByRole("combobox", { name: "View" })).toHaveValue("");
    expect(client.listRecords).toHaveBeenLastCalledWith(database.id, expect.objectContaining({ view_id: undefined }), expect.any(AbortSignal));
  });
  it("edits one cell with the full saved body and version, excluding computed rollups", async () => {
    const client = makeClient(); const busy = vi.fn(); let resolve!: (value: NoteDatabaseRecord) => void;
    vi.mocked(client.updateRecord).mockImplementation(() => new Promise((done) => { resolve = done; }));
    table(client, { onBusyChange: busy }); await loaded(); fireEvent.click(screen.getAllByRole("button", { name: "Edit Effort" })[0]);
    fireEvent.change(await screen.findByRole("spinbutton", { name: "Effort" }), { target: { value: "7" } }); fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(client.updateRecord).toHaveBeenCalledWith(database.id, row.id, { values: { title: "Publish app", effort: 7, tags: ["option-a"] }, content_markdown: row.content_markdown, expected_version: 4 }));
    expect(busy).toHaveBeenLastCalledWith(true); expect(screen.getByRole("button", { name: "Close" })).toBeDisabled();
    resolve({ ...row, values: { ...row.values, effort: 7 }, version: 5 }); await waitFor(() => expect(busy).toHaveBeenLastCalledWith(false));
  });
  it("keeps a conflict draft, guards close/unload and preserves it across a renewed public client", async () => {
    const client = makeClient(); vi.mocked(client.updateRecord).mockRejectedValue({ status: 409 }); const onDirtyChange = vi.fn(); const onClose = vi.fn();
    const props = { databaseId: database.id, recordId: row.id, fields: detail.fields, canWrite: true, onClose, onSaved: vi.fn(), onDirtyChange };
    const result = render(<NoteDatabaseRecordDialog client={client} {...props} />, { wrapper: wrapper() });
    fireEvent.change(await screen.findByRole("textbox", { name: "Title" }), { target: { value: "My unsaved title" } }); fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText(/Your draft is preserved/); const fresh = makeClient(); vi.mocked(fresh.getRecord).mockResolvedValue({ ...row, values: { title: "Other edit" }, version: 9 });
    result.rerender(<NoteDatabaseRecordDialog client={fresh} {...props} />); await waitFor(() => expect(fresh.getRecord).toHaveBeenCalled());
    expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("My unsaved title"); expect(onDirtyChange).toHaveBeenLastCalledWith(true);
    const event = new Event("beforeunload", { cancelable: true }); window.dispatchEvent(event); expect(event.defaultPrevented).toBe(true);
    vi.mocked(window.confirm).mockReturnValue(false); fireEvent.click(screen.getByRole("button", { name: "Close" })); expect(onClose).not.toHaveBeenCalled();
  });
  it("shows public read-only rows and Markdown without write controls or unsafe links", async () => {
    const url = field("url", "Link", "url"); const client = makeClient({ ...detail, fields: [titleField, url] });
    vi.mocked(client.listRecords).mockResolvedValue({ items: [{ ...row, values: { title: "Document", url: "javascript:alert(1)" } }], total: 1, page: 1, per_page: 50 });
    table(client, { canWrite: false, canManage: false }); await loaded(); expect(screen.queryByRole("button", { name: "Fields" })).not.toBeInTheDocument(); expect(screen.queryByRole("button", { name: "Add record" })).not.toBeInTheDocument(); expect(screen.queryByRole("link")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Open record: Document" })); expect(await screen.findByRole("textbox", { name: "Record Markdown" })).toHaveAttribute("readonly"); expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
  });
  it("temporarily closes a dialog for public unlock and restores its draft afterwards", async () => {
    const client = makeClient(); const props = { databaseId: database.id, recordId: row.id, fields: detail.fields, canWrite: true, onClose: vi.fn(), onSaved: vi.fn() };
    const result = render(<NoteDatabaseSuspendedContext.Provider value={false}><NoteDatabaseRecordDialog client={client} {...props} /></NoteDatabaseSuspendedContext.Provider>, { wrapper: wrapper() });
    fireEvent.change(await screen.findByRole("textbox", { name: "Title" }), { target: { value: "Draft during reauth" } });
    result.rerender(<NoteDatabaseSuspendedContext.Provider value><NoteDatabaseRecordDialog client={client} {...props} /></NoteDatabaseSuspendedContext.Provider>);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    const fresh = makeClient(); result.rerender(<NoteDatabaseSuspendedContext.Provider value={false}><NoteDatabaseRecordDialog client={fresh} {...props} /></NoteDatabaseSuspendedContext.Provider>);
    await waitFor(() => expect(fresh.getRecord).toHaveBeenCalled()); expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Draft during reauth");
  });
  it("fetches the complete order before moving a row beyond a page boundary", async () => {
    const client = makeClient(); const all = Array.from({ length: 501 }, (_, index) => ({ ...row, id: `row-${index}`, position: index, values: { title: `Task ${index}` } }));
    vi.mocked(client.listRecords).mockImplementation(async (_db, query) => query?.per_page === 500 ? { items: query.page === 1 ? all.slice(0, 500) : all.slice(500), total: 501, page: query.page ?? 1, per_page: 500 } : { items: all.slice(0, 50), total: 501, page: 1, per_page: 50 });
    table(client); await loaded(); fireEvent.click(screen.getByRole("button", { name: "Move down: Task 0" }));
    await waitFor(() => expect(client.reorderRecords).toHaveBeenCalled()); const ids = vi.mocked(client.reorderRecords).mock.calls[0][1]; expect(ids).toHaveLength(501); expect(ids.slice(0, 3)).toEqual(["row-1", "row-0", "row-2"]); expect(vi.mocked(client.reorderRecords).mock.calls[0][2]).toBe(8);
  });
  it("searches records on the server and resets pagination", async () => {
    const client = makeClient(); table(client); await loaded(); fireEvent.change(screen.getByRole("searchbox", { name: "Search records" }), { target: { value: " release " } });
    await waitFor(() => expect(client.listRecords).toHaveBeenLastCalledWith(database.id, expect.objectContaining({ q: "release", page: 1 }), expect.any(AbortSignal)));
  });
  it("saves record Markdown with the keyboard and deletes using its refreshed version", async () => {
    const client = makeClient(); const close = vi.fn(); recordDialog(client, { onClose: close }); const body = await screen.findByRole("textbox", { name: "Record Markdown" });
    fireEvent.change(body, { target: { value: "## Updated\n- [x] Ready" } }); fireEvent.keyDown(body, { key: "s", metaKey: true });
    await waitFor(() => expect(client.updateRecord).toHaveBeenCalledWith(database.id, row.id, expect.objectContaining({ content_markdown: "## Updated\n- [x] Ready", expected_version: 4 })));
    await waitFor(() => expect(screen.getByRole("button", { name: "Delete record" })).toBeEnabled()); fireEvent.click(screen.getByRole("button", { name: "Delete record" })); await waitFor(() => expect(client.deleteRecord).toHaveBeenCalledWith(database.id, row.id, 5)); expect(close).toHaveBeenCalled();
  });
});

describe("schema, views and relationships", () => {
  it("creates inverse relationships and lets public editors manage fields without ordering", async () => {
    const client = makeClient(); table(client, { canManage: false, canEditFields: true }); await loaded(); fireEvent.click(screen.getByRole("button", { name: "Fields" }));
    const dialog = screen.getByRole("dialog", { name: "Fields" }); expect(within(dialog).queryByRole("button", { name: "Move up: Effort" })).not.toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Add field" })); fireEvent.change(within(dialog).getByRole("textbox", { name: "Name" }), { target: { value: "Subtasks" } }); fireEvent.change(within(dialog).getByRole("combobox", { name: "Field type" }), { target: { value: "relation" } });
    fireEvent.change(within(dialog).getByRole("combobox", { name: "Relationship" }), { target: { value: "one_to_many" } }); fireEvent.change(within(dialog).getByRole("textbox", { name: "Inverse field name" }), { target: { value: "Parent task" } }); fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(client.createField).toHaveBeenCalledWith(database.id, expect.objectContaining({ name: "Subtasks", field_type: "relation", relation: { target_database_id: database.id, cardinality: "one_to_many", inverse_name: "Parent task" } })));
  });
  it("keeps stable option IDs when editing labels and orders fields with the database version", async () => {
    const client = makeClient(); render(<NoteDatabaseFieldsDialog client={client} detail={detail} onClose={vi.fn()} onChange={vi.fn()} />, { wrapper: wrapper() });
    fireEvent.click(screen.getByRole("button", { name: "Field settings: Tags" })); fireEvent.change(screen.getByRole("textbox", { name: "Option label 1" }), { target: { value: "Shipped" } }); fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(client.updateField).toHaveBeenCalledWith(database.id, "tags", expect.objectContaining({ config: { options: [{ id: "option-a", label: "Shipped" }, { id: "option-b", label: "Blocked" }] }, expected_version: 3 })));
    fireEvent.click(screen.getByRole("button", { name: "Move up: Effort" })); await waitFor(() => expect(client.reorderFields).toHaveBeenCalledWith(database.id, ["effort", "title", "tags", "total"], 8));
  });
  it("creates minimum rollups over date fields", async () => {
    const withRelation = { ...detail, fields: [titleField, relationField] }; const client = makeClient(withRelation);
    vi.mocked(client.getDatabase).mockResolvedValue({ ...detail, database: { ...database, id: "db-b" }, fields: [field("due", "Due date", "date"), numberField] });
    render(<NoteDatabaseFieldsDialog client={client} detail={withRelation} onClose={vi.fn()} onChange={vi.fn()} />, { wrapper: wrapper() });
    fireEvent.click(screen.getByRole("button", { name: "Add field" })); fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Earliest date" } }); fireEvent.change(screen.getByRole("combobox", { name: "Field type" }), { target: { value: "rollup" } }); fireEvent.change(screen.getByRole("combobox", { name: "Operation" }), { target: { value: "min" } });
    await screen.findByRole("option", { name: "Due date" }); fireEvent.change(screen.getByRole("combobox", { name: "Target field" }), { target: { value: "due" } }); fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(client.createField).toHaveBeenCalledWith(database.id, expect.objectContaining({ rollup: { relation_field_id: "related", operation: "min", target_field_id: "due" } })));
  });
  it("enforces inverse single selection, excludes self and opens related record cards", async () => {
    const related = { ...rowB, database_id: "db-b", id: "related-b", values: { title: "Parent B" } }; const client = makeClient();
    vi.mocked(client.getDatabase).mockResolvedValue({ ...detail, database: { ...database, id: "db-b" }, fields: [titleField] });
    vi.mocked(client.listRecords).mockResolvedValue({ items: [row, { ...related, id: "related-a", values: { title: "Parent A" } }, related], total: 3, page: 1, per_page: 50 });
    vi.mocked(client.getRecord).mockImplementation(async (_db, id) => id === "related-b" ? related : { ...row, values: { ...row.values, related: ["related-a"] } });
    recordDialog(client, { fields: [titleField, relationField] }); const relation = await screen.findByRole("group", { name: "Related" }); await within(relation).findByRole("checkbox", { name: "Parent B" });
    expect(within(relation).queryByRole("checkbox", { name: "Publish app" })).not.toBeInTheDocument(); fireEvent.click(within(relation).getByRole("checkbox", { name: "Parent B" })); expect(within(relation).getByRole("checkbox", { name: "Parent A" })).not.toBeChecked();
    fireEvent.click(within(relation).getByRole("button", { name: "Open record: Parent B" })); const relatedDialog = await screen.findByRole("dialog", { name: "Parent B" }); expect(within(relatedDialog).getByRole("textbox", { name: "Title" })).toHaveValue("Parent B");
  });
  it("saves array filters, scalar contains, sorting and column layout in one view", async () => {
    const client = makeClient(); render(<NoteDatabaseViewsDialog client={client} detail={detail} onClose={vi.fn()} onChange={vi.fn()} onSelect={vi.fn()} />, { wrapper: wrapper() });
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Ready tasks" } }); fireEvent.click(screen.getByRole("button", { name: "Add filter" })); fireEvent.change(screen.getByRole("combobox", { name: "Fields 1" }), { target: { value: "tags" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Ready" })); fireEvent.click(screen.getByRole("button", { name: "Add filter" })); fireEvent.change(screen.getByRole("combobox", { name: "Fields 2" }), { target: { value: "tags" } }); fireEvent.change(screen.getByRole("combobox", { name: "Operation 2" }), { target: { value: "contains" } }); fireEvent.change(screen.getByRole("combobox", { name: "Filter value" }), { target: { value: "option-b" } });
    fireEvent.click(screen.getByRole("button", { name: "Add sort" })); fireEvent.change(screen.getByRole("combobox", { name: "Sorting 1" }), { target: { value: "effort" } }); fireEvent.change(screen.getByRole("spinbutton", { name: "Width: Effort" }), { target: { value: "240" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" })); await waitFor(() => expect(client.createView).toHaveBeenCalledWith(database.id, expect.objectContaining({ name: "Ready tasks", config: expect.objectContaining({ filters: [{ field_id: "tags", operator: "equals", value: ["option-a"] }, { field_id: "tags", operator: "contains", value: "option-b" }], sorts: [{ field_id: "effort", direction: "asc" }], column_widths: { effort: 240 } }) })));
  });
  it("resets a selected view when it is deleted", async () => {
    const client = makeClient(); const select = vi.fn(); render(<NoteDatabaseViewsDialog client={client} detail={detail} initialViewId={view.id} onClose={vi.fn()} onChange={vi.fn()} onSelect={select} />, { wrapper: wrapper() });
    fireEvent.click(screen.getByRole("button", { name: "Delete view: Ready work" })); await waitFor(() => expect(client.deleteView).toHaveBeenCalledWith(database.id, view.id, 5)); expect(select).toHaveBeenCalledWith("");
  });
  it("accepts URL substrings as filter values without requiring a complete URL", async () => {
    const client = makeClient(); const data = { ...detail, fields: [field("link", "Website", "url")] };
    render(<NoteDatabaseViewsDialog client={client} detail={data} onClose={vi.fn()} onChange={vi.fn()} onSelect={vi.fn()} />, { wrapper: wrapper() });
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Documentation" } }); fireEvent.click(screen.getByRole("button", { name: "Add filter" })); fireEvent.change(screen.getByRole("combobox", { name: "Operation 1" }), { target: { value: "contains" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Filter value" }), { target: { value: "/docs/" } }); fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(client.createView).toHaveBeenCalledWith(database.id, expect.objectContaining({ config: expect.objectContaining({ filters: [{ field_id: "link", operator: "contains", value: "/docs/" }] }) })));
  });
});

describe("database picker", () => {
  it("inserts an existing table's selected saved view", async () => {
    const client = makeClient(); const select = vi.fn(); render(<NoteDatabasePicker client={client} canWrite onSelect={select} onCancel={vi.fn()} />, { wrapper: wrapper() });
    fireEvent.change(await screen.findByRole("combobox", { name: "Select table" }), { target: { value: database.id } }); fireEvent.change(await screen.findByRole("combobox", { name: "View" }), { target: { value: view.id } }); fireEvent.click(screen.getByRole("button", { name: "Insert" })); await waitFor(() => expect(select).toHaveBeenCalledWith(database.id, view.id));
  });
  it("retains a newly created table when insertion fails, so retry does not create a duplicate", async () => {
    const client = makeClient(); const select = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValue(undefined); render(<NoteDatabasePicker client={client} canWrite onSelect={select} onCancel={vi.fn()} />, { wrapper: wrapper() });
    fireEvent.click(await screen.findByRole("button", { name: "Create table" })); fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: " Roadmap " } }); fireEvent.click(screen.getAllByRole("button", { name: "Create table" })[1]); await screen.findByRole("alert");
    expect(client.createDatabase).toHaveBeenCalledTimes(1); fireEvent.click(screen.getByRole("button", { name: "Insert" })); await waitFor(() => expect(select).toHaveBeenCalledTimes(2)); expect(client.createDatabase).toHaveBeenCalledTimes(1); expect(select).toHaveBeenLastCalledWith("new-db", null);
  });
  it.each([ ["ru", "Вставить таблицу"], ["ro", "Inserează un tabel"] ] as const)("has localized controls in %s", async (locale, label) => {
    render(<NoteDatabasePicker client={makeClient()} canWrite={false} onSelect={vi.fn()} onCancel={vi.fn()} />, { wrapper: wrapper(locale) }); expect(await screen.findByRole("dialog", { name: label })).toBeInTheDocument();
  });
});
