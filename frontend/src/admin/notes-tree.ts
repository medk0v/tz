import type { NoteMoveInput, NoteSummary } from "../notes-api";

export type NoteDropPosition = "before" | "inside" | "after" | "root";
export interface NoteDropTarget { id: string | null; position: NoteDropPosition }

export function compareNoteOrder(a: NoteSummary, b: NoteSummary) {
  return a.sort_order - b.sort_order || a.id.localeCompare(b.id);
}

/** Derive the insertion anchor from the saved tree, excluding the moving page. */
export function noteMoveInput(notes: NoteSummary[], sourceId: string, target: NoteDropTarget, allowUnchanged = false): NoteMoveInput | null {
  const source = notes.find((note) => note.id === sourceId);
  if (!source) return null;
  const descendants = new Set([sourceId]);
  for (let previous = -1; previous !== descendants.size;) {
    previous = descendants.size;
    notes.forEach((note) => { if (note.parent_id && descendants.has(note.parent_id)) descendants.add(note.id); });
  }
  const destination = notes.find((note) => note.id === target.id);
  if (target.position !== "root" && (!destination || descendants.has(destination.id))) return null;
  const parent_id = target.position === "root" ? null : target.position === "inside" ? destination!.id : destination!.parent_id;
  if (parent_id && descendants.has(parent_id)) return null;
  const siblings = notes.filter((note) => note.parent_id === parent_id && note.id !== sourceId).sort(compareNoteOrder);
  const before_id = target.position === "before" ? destination!.id : target.position === "after" ? siblings[siblings.findIndex((note) => note.id === destination!.id) + 1]?.id ?? null : null;
  if (!allowUnchanged && source.parent_id === parent_id) {
    const current = notes.filter((note) => note.parent_id === parent_id).sort(compareNoteOrder);
    if ((current[current.findIndex((note) => note.id === sourceId) + 1]?.id ?? null) === before_id) return null;
  }
  return { parent_id, before_id, expected_version: source.version };
}

export function noteDropPosition(clientY: number, bounds: Pick<DOMRect, "top" | "height">): Exclude<NoteDropPosition, "root"> {
  const relative = (clientY - bounds.top) / Math.max(bounds.height, 1);
  return relative < .25 ? "before" : relative > .75 ? "after" : "inside";
}
