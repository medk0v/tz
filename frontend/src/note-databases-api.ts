import { managementApiRequest, type OperatorAuth } from "./api";
import { publicNoteRequest, PublicNoteShareError } from "./note-shares-api";

export type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue };
export type FieldType = "text" | "long_text" | "number" | "date" | "boolean" | "single_select" | "multi_select" | "url" | "email" | "relation" | "rollup";
export type RelationCardinality = "one_to_one" | "one_to_many" | "many_to_one" | "many_to_many";
export type RollupOperation = "count" | "sum" | "min" | "max" | "show_values";
export interface SelectOption { id: string; label: string }
export interface NoteDatabase {
  id: string; project_id: string; name: string; description: string; icon: string;
  version: number; created_at: string; updated_at: string;
}
export interface NoteDatabaseField {
  id: string; database_id: string; name: string; field_type: FieldType; position: number; version: number;
  config: { options?: SelectOption[]; [key: string]: JsonValue | SelectOption[] | undefined };
  relation_target_database_id: string | null; relation_cardinality: RelationCardinality | null;
  relation_owner_field_id: string | null; inverse_field_id: string | null;
  rollup_relation_field_id: string | null; rollup_target_field_id: string | null; rollup_operation: RollupOperation | null;
  created_at: string; updated_at: string;
}
export interface NoteDatabaseRecord {
  id: string; database_id: string; position: number; version: number; values: Record<string, JsonValue>;
  content_markdown?: string; created_at: string; updated_at: string;
}
export interface ViewFilter { field_id: string; operator: string; value: JsonValue }
export interface ViewSort { field_id: string; direction: "asc" | "desc" }
export interface ViewConfig {
  filters: ViewFilter[]; sorts: ViewSort[]; hidden_fields: string[]; column_order: string[];
  column_widths: Record<string, number>; pinned_fields: string[];
}
export interface NoteDatabaseView {
  id: string; database_id: string; name: string; position: number; version: number;
  config: ViewConfig; created_at: string; updated_at: string;
}
export interface NoteDatabaseDetail { database: NoteDatabase; fields: NoteDatabaseField[]; views: NoteDatabaseView[] }
export interface NoteDatabaseRecords { items: NoteDatabaseRecord[]; total: number; page: number; per_page: number }
export interface NoteDatabaseEmbed {
  id: string; note_id: string; database_id: string; view_id: string | null; position: number;
  version: number; created_at: string; updated_at: string;
}
export interface DatabaseInput { name: string; description: string; icon: string }
export interface CreateField {
  name: string; field_type: FieldType; config?: NoteDatabaseField["config"];
  relation?: { target_database_id: string; cardinality: Exclude<RelationCardinality, "many_to_one">; inverse_name: string };
  rollup?: { relation_field_id: string; target_field_id: string | null; operation: RollupOperation };
}
export interface UpdateField { name: string; field_type: FieldType; config: NoteDatabaseField["config"]; expected_version: number }
export interface RecordInput { values: Record<string, JsonValue>; content_markdown: string }
export interface ViewInput { name: string; config: ViewConfig }
export interface RecordQuery { page?: number; per_page?: number; q?: string; view_id?: string }
export interface EmbedInput { database_id: string; view_id?: string | null; position?: number }
export type NoteDatabaseClient = NoteDatabasesClient;
export type NoteDatabaseFieldType = FieldType;
export type NoteDatabaseViewConfig = ViewConfig;
export type NoteDatabaseFieldInput = CreateField;
export type NoteDatabaseViewInput = ViewInput;
export type NoteDatabaseValue = JsonValue;
export interface NoteDatabasesClient {
  listDatabases(signal?: AbortSignal): Promise<NoteDatabase[]>;
  getDatabase(id: string, signal?: AbortSignal): Promise<NoteDatabaseDetail>;
  createDatabase(input: DatabaseInput): Promise<NoteDatabase>;
  updateDatabase(id: string, input: DatabaseInput & { expected_version: number }): Promise<NoteDatabase>;
  deleteDatabase(id: string, version: number): Promise<void>;
  createField(db: string, input: CreateField): Promise<NoteDatabaseField>;
  updateField(db: string, id: string, input: UpdateField): Promise<NoteDatabaseField>;
  deleteField(db: string, id: string, version: number): Promise<void>;
  reorderFields(db: string, ids: string[], version: number): Promise<void>;
  listRecords(db: string, query?: RecordQuery, signal?: AbortSignal): Promise<NoteDatabaseRecords>;
  getRecord(db: string, id: string, signal?: AbortSignal): Promise<NoteDatabaseRecord>;
  createRecord(db: string, input: RecordInput): Promise<NoteDatabaseRecord>;
  updateRecord(db: string, id: string, input: RecordInput & { expected_version: number }): Promise<NoteDatabaseRecord>;
  deleteRecord(db: string, id: string, version: number): Promise<void>;
  reorderRecords(db: string, ids: string[], version: number): Promise<void>;
  createView(db: string, input: ViewInput): Promise<NoteDatabaseView>;
  updateView(db: string, id: string, input: ViewInput & { expected_version: number }): Promise<NoteDatabaseView>;
  deleteView(db: string, id: string, version: number): Promise<void>;
  reorderViews(db: string, ids: string[], version: number): Promise<void>;
  listEmbeds(note: string, signal?: AbortSignal): Promise<NoteDatabaseEmbed[]>;
  createEmbed(note: string, input: EmbedInput): Promise<NoteDatabaseEmbed>;
  updateEmbed(note: string, id: string, input: Required<EmbedInput> & { expected_version: number }): Promise<NoteDatabaseEmbed>;
  deleteEmbed(note: string, id: string, version: number): Promise<void>;
}

const base = "/api/v1/notes/databases";
const segment = encodeURIComponent;
function queryString(query: RecordQuery = {}): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) if (value !== undefined && value !== "") params.set(key, String(value));
  return params.size ? `?${params}` : "";
}

export function createNoteDatabaseClient(auth: OperatorAuth): NoteDatabasesClient {
  const request = <T>(path: string, method = "GET", body?: unknown, signal?: AbortSignal) => managementApiRequest<T>(auth, path, {
    method, ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal,
  });
  const dbPath = (db: string) => `${base}/${segment(db)}`;
  const embedsPath = (note: string) => `/api/v1/notes/${segment(note)}/databases`;
  return {
    listDatabases: async (signal) => (await request<{ items: NoteDatabase[] }>(base, "GET", undefined, signal)).items,
    getDatabase: (id, signal) => request(dbPath(id), "GET", undefined, signal),
    createDatabase: (input) => request(base, "POST", input),
    updateDatabase: (id, input) => request(dbPath(id), "PATCH", input),
    deleteDatabase: (id, version) => request(`${dbPath(id)}?expected_version=${version}`, "DELETE"),
    createField: (db, input) => request(`${dbPath(db)}/fields`, "POST", input),
    updateField: (db, id, input) => request(`${dbPath(db)}/fields/${segment(id)}`, "PATCH", input),
    deleteField: (db, id, version) => request(`${dbPath(db)}/fields/${segment(id)}?expected_version=${version}`, "DELETE"),
    reorderFields: (db, ids, version) => request(`${dbPath(db)}/fields/order`, "PUT", { ids, expected_version: version }),
    listRecords: (db, query, signal) => request(`${dbPath(db)}/records${queryString(query)}`, "GET", undefined, signal),
    getRecord: (db, id, signal) => request(`${dbPath(db)}/records/${segment(id)}`, "GET", undefined, signal),
    createRecord: (db, input) => request(`${dbPath(db)}/records`, "POST", input),
    updateRecord: (db, id, input) => request(`${dbPath(db)}/records/${segment(id)}`, "PATCH", input),
    deleteRecord: (db, id, version) => request(`${dbPath(db)}/records/${segment(id)}?expected_version=${version}`, "DELETE"),
    reorderRecords: (db, ids, version) => request(`${dbPath(db)}/records/order`, "PUT", { ids, expected_version: version }),
    createView: (db, input) => request(`${dbPath(db)}/views`, "POST", input),
    updateView: (db, id, input) => request(`${dbPath(db)}/views/${segment(id)}`, "PATCH", input),
    deleteView: (db, id, version) => request(`${dbPath(db)}/views/${segment(id)}?expected_version=${version}`, "DELETE"),
    reorderViews: (db, ids, version) => request(`${dbPath(db)}/views/order`, "PUT", { ids, expected_version: version }),
    listEmbeds: async (note, signal) => (await request<{ items: NoteDatabaseEmbed[] }>(embedsPath(note), "GET", undefined, signal)).items,
    createEmbed: (note, input) => request(embedsPath(note), "POST", input),
    updateEmbed: (note, id, input) => request(`${embedsPath(note)}/${segment(id)}`, "PATCH", input),
    deleteEmbed: (note, id, version) => request(`${embedsPath(note)}/${segment(id)}?expected_version=${version}`, "DELETE"),
  };
}

/** Public capability credentials stay in JSON bodies and never use the cabinet session. */
export function createPublicNoteDatabaseClient(token: string, accessToken?: string, onUnauthorized?: () => void): NoteDatabasesClient {
  const request = <T>(action: string, input: object = {}, signal?: AbortSignal) => publicNoteRequest<T>("databases", "POST", {
    token, access_token: accessToken, action, ...input,
  }, signal).catch((failure: unknown) => {
    if (failure instanceof PublicNoteShareError && failure.status === 401) onUnauthorized?.();
    throw failure;
  });
  const unavailable = () => Promise.reject(new Error("This operation is available in the workspace."));
  return {
    listDatabases: async (signal) => (await request<{ items: NoteDatabase[] }>("list", {}, signal)).items,
    getDatabase: (database_id, signal) => request("detail", { database_id }, signal),
    listRecords: (database_id, query, signal) => request("records", { database_id, ...query }, signal),
    getRecord: (database_id, record_id, signal) => request("record", { database_id, record_id }, signal),
    createRecord: (database_id, input) => request("create_record", { database_id, ...input }),
    updateRecord: (database_id, record_id, input) => request("update_record", { database_id, record_id, ...input }),
    deleteRecord: (database_id, record_id, expected_version) => request("delete_record", { database_id, record_id, expected_version }),
    listEmbeds: async (note_id, signal) => (await request<{ items: NoteDatabaseEmbed[] }>("embeds", { note_id }, signal)).items,
    createField: (database_id, field) => request("create_field", { database_id, field }),
    updateField: (database_id, field_id, field) => request("update_field", { database_id, field_id, field }),
    deleteField: (database_id, field_id, expected_version) => request("delete_field", { database_id, field_id, expected_version }),
    createDatabase: unavailable, updateDatabase: unavailable, deleteDatabase: unavailable,
    reorderFields: unavailable,
    reorderRecords: unavailable, createView: unavailable, updateView: unavailable, deleteView: unavailable, reorderViews: unavailable,
    createEmbed: unavailable, updateEmbed: unavailable, deleteEmbed: unavailable,
  };
}
