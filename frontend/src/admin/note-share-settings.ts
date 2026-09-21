import type { NoteShareSettings } from "../note-shares-api";
import type { NoteSummary } from "../notes-api";

export interface SharePageRow {
  note: NoteSummary;
  parentId: string | null;
  ancestors: string[];
  path: string;
}

export type ShareSettingsDraft = Omit<NoteShareSettings, "included_note_ids"> & { included_note_ids: string[] };

/** The publication root is implicit; every returned ancestor is selectable. */
export function sharePageRows(notes: NoteSummary[], rootId: string | null): SharePageRow[] {
  const ids = new Set(notes.map((note) => note.id));
  const children = new Map<string | null, NoteSummary[]>();
  for (const note of [...notes].sort((a, b) => a.sort_order - b.sort_order || a.id.localeCompare(b.id))) {
    const parent = note.parent_id && ids.has(note.parent_id) ? note.parent_id : null;
    children.set(parent, [...(children.get(parent) ?? []), note]);
  }
  const rows: SharePageRow[] = [];
  const visited = new Set(rootId ? [rootId] : []);
  function append(parent: string | null, ancestors: string[], names: string[]) {
    for (const note of children.get(parent) ?? []) {
      if (visited.has(note.id)) continue;
      visited.add(note.id);
      const path = [...names, note.title];
      rows.push({ note, parentId: ancestors.at(-1) ?? null, ancestors, path: path.join(" / ") });
      append(note.id, [...ancestors, note.id], path);
    }
  }
  append(rootId, [], []);
  return rows;
}

export function materializeShareSettings(settings: NoteShareSettings, rows: SharePageRow[]): ShareSettingsDraft {
  const selected = new Set(settings.included_note_ids ?? rows.map((row) => row.note.id));
  return {
    theme_palette: settings.theme_palette, theme_mode: settings.theme_mode,
    allow_theme_change: settings.allow_theme_change, include_descendants: settings.include_descendants, can_edit: settings.can_edit,
    included_note_ids: rows.filter((row) => selected.has(row.note.id) && row.ancestors.every((id) => selected.has(id))).map((row) => row.note.id),
  };
}

export function toggleSharedPage(selectedIds: string[], rows: SharePageRow[], id: string, checked: boolean): string[] {
  const selected = new Set(selectedIds);
  const row = rows.find((item) => item.note.id === id);
  if (!row) return selectedIds;
  if (checked) {
    selected.add(id);
    row.ancestors.forEach((ancestor) => selected.add(ancestor));
  } else {
    rows.filter((item) => item.note.id === id || item.ancestors.includes(id)).forEach((item) => selected.delete(item.note.id));
  }
  return rows.filter((item) => selected.has(item.note.id)).map((item) => item.note.id);
}

/** Hidden descendants remain in the local draft, but never in a root-only request. */
export function shareSettingsInput(settings: ShareSettingsDraft, rows: SharePageRow[], rootId: string | null): NoteShareSettings {
  const normalized = materializeShareSettings(settings, rows);
  const visible = new Set(rows.filter((row) => settings.include_descendants || (rootId === null && row.ancestors.length === 0)).map((row) => row.note.id));
  return { ...normalized, included_note_ids: normalized.included_note_ids.filter((id) => visible.has(id)) };
}
