import { managementApiRequest, type OperatorAuth } from "./api";

export interface NoteInput {
  title: string;
  body: string;
  parent_id: string | null;
  icon: string;
  is_favorite: boolean;
}

export interface NoteSummary extends Omit<NoteInput, "body"> {
  id: string;
  version: number;
  sort_order: number;
  created_at: string;
  updated_at: string;
}

export interface Note extends NoteSummary { body: string }
export interface NoteMoveInput { parent_id: string | null; before_id: string | null; expected_version: number }
export interface NoteMoveResult { note: Note; items: NoteSummary[] }

const notePath = (id: string) => `/api/v1/notes/${encodeURIComponent(id)}`;

export async function listNotes(auth: OperatorAuth, signal?: AbortSignal): Promise<NoteSummary[]> {
  const response = await managementApiRequest<{ items: NoteSummary[] }>(auth, "/api/v1/notes", { signal });
  return response.items;
}

export function getNote(auth: OperatorAuth, id: string, signal?: AbortSignal): Promise<Note> {
  return managementApiRequest(auth, notePath(id), { signal });
}

export function createNote(auth: OperatorAuth, input: NoteInput): Promise<Note> {
  return managementApiRequest(auth, "/api/v1/notes", { method: "POST", body: JSON.stringify(input) });
}

export function updateNote(auth: OperatorAuth, id: string, input: NoteInput, expectedVersion: number): Promise<Note> {
  return managementApiRequest(auth, notePath(id), {
    method: "PATCH", body: JSON.stringify({ ...input, expected_version: expectedVersion }),
  });
}

export function deleteNote(auth: OperatorAuth, id: string, expectedVersion: number): Promise<void> {
  return managementApiRequest(auth, `${notePath(id)}?expected_version=${expectedVersion}`, { method: "DELETE" });
}

export function moveNote(auth: OperatorAuth, id: string, input: NoteMoveInput): Promise<NoteMoveResult> {
  return managementApiRequest(auth, `${notePath(id)}/move`, { method: "POST", body: JSON.stringify(input) });
}
