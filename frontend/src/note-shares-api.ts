import { apiBaseUrl, managementApiRequest, type OperatorAuth } from "./api";
import type { Palette, Theme } from "./admin/preference-options";

export interface NoteShareSettings {
  theme_palette: Palette | null;
  theme_mode: Theme | null;
  allow_theme_change: boolean;
  include_descendants: boolean;
  included_note_ids: string[] | null;
  can_edit: boolean;
}

export interface NoteShare extends NoteShareSettings {
  id: string;
  note_id: string | null;
  custom_slug: string | null;
  has_password: boolean;
  version: number;
  created_at: string;
}

export interface CreatedNoteShare extends NoteShare { token: string }

export interface NoteShareInput extends Partial<NoteShareSettings> {
  note_id: string | null;
  password?: string | null;
  custom_slug?: string | null;
  can_edit: boolean;
}

export interface NoteShareUpdate extends NoteShareSettings { expected_version: number }

export interface PublicNoteSummary {
  id: string;
  parent_id: string | null;
  sort_order: number;
  title: string;
  icon: string;
  version: number;
  created_at: string;
  updated_at: string;
}

export interface PublicNote extends PublicNoteSummary { body: string }

export interface PublicNotes {
  can_edit: boolean;
  root_note_id: string | null;
  theme_palette: Palette | null;
  theme_mode: Theme | null;
  allow_theme_change: boolean;
  include_descendants: boolean;
  items: PublicNoteSummary[];
  note: PublicNote | null;
}

export interface PublicNoteUpdate {
  title: string;
  body: string;
  icon: string;
  expected_version: number;
}

export class PublicNoteShareError extends Error {
  constructor(message: string, readonly status: number, readonly code: string) {
    super(message);
    this.name = "PublicNoteShareError";
  }
}

export async function listNoteShares(auth: OperatorAuth, signal?: AbortSignal): Promise<NoteShare[]> {
  const response = await managementApiRequest<{ items: NoteShare[] }>(auth, "/api/v1/notes/shares", { signal, cache: "no-store" });
  return response.items;
}

export function createNoteShare(auth: OperatorAuth, input: NoteShareInput): Promise<CreatedNoteShare> {
  return managementApiRequest(auth, "/api/v1/notes/shares", { method: "POST", body: JSON.stringify(input) });
}

export function updateNoteShare(auth: OperatorAuth, id: string, input: NoteShareUpdate): Promise<NoteShare> {
  return managementApiRequest(auth, `/api/v1/notes/shares/${encodeURIComponent(id)}`, { method: "PATCH", body: JSON.stringify(input) });
}

export function revokeNoteShare(auth: OperatorAuth, id: string): Promise<void> {
  return managementApiRequest(auth, `/api/v1/notes/shares/${encodeURIComponent(id)}`, { method: "DELETE" });
}

export function publicNoteSharePath(token: string, noteId?: string): string {
  const path = `/notes/share/${encodeURIComponent(token)}`;
  return noteId ? `${path}?note=${encodeURIComponent(noteId)}` : path;
}

export async function publicNoteRequest<T>(path: string, method: "POST" | "PATCH", input: unknown, signal?: AbortSignal): Promise<T> {
  const response = await fetch(`${apiBaseUrl}/api/v1/public/notes/${path}`, {
    method,
    credentials: "omit",
    cache: "no-store",
    referrerPolicy: "no-referrer",
    redirect: "error",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(input),
    signal,
  });
  const payload = await response.json();
  if (!response.ok) {
    throw new PublicNoteShareError(
      typeof payload?.error?.message === "string" ? payload.error.message : "Could not open the shared notes.",
      response.status,
      typeof payload?.error?.code === "string" ? payload.error.code : "request_failed",
    );
  }
  return payload as T;
}

export function resolvePublicNotes(token: string, accessToken?: string, noteId?: string, signal?: AbortSignal): Promise<PublicNotes> {
  return publicNoteRequest("resolve", "POST", { token, access_token: accessToken, note_id: noteId }, signal);
}

export function unlockPublicNotes(token: string, password: string): Promise<{ access_token: string }> {
  return publicNoteRequest("unlock", "POST", { token, password });
}

export function updatePublicNote(token: string, accessToken: string | undefined, noteId: string, input: PublicNoteUpdate): Promise<PublicNote> {
  return publicNoteRequest("page", "PATCH", { token, access_token: accessToken, note_id: noteId, ...input });
}
