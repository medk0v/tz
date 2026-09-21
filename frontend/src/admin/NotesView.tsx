import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { AnimatedDetails } from "../AnimatedDetails";
import { lazy, Suspense, useEffect, useId, useMemo, useRef, useState, type DragEvent, type FormEvent, type KeyboardEvent, type ReactNode } from "react";
import { ArrowLeft, ChevronDown, ChevronRight, Copy, FileText, FolderOpen, GripVertical, PanelLeftClose, PanelLeftOpen, Plus, RefreshCw, Save, Search, Share2, Star, Trash2 } from "lucide-react";
import { ApiRequestError, type OperatorAuth } from "../api";
import { createNote, deleteNote, getNote, listNotes, moveNote, updateNote, type Note, type NoteInput, type NoteSummary } from "../notes-api";
import { useI18n } from "../i18n";
import { notesText } from "./notes-i18n";
import { noteSharesText } from "../note-shares-i18n";
import { NoteShareDialog } from "./NoteShareDialog";
import { createNoteDatabaseClient } from "../note-databases-api";
import { NoteDatabaseEmbeds } from "./NoteDatabaseEmbeds";
import { PreferenceDropdown } from "./PreferenceDropdown";
import { useCanonicalPageRoute, usePageRoute } from "./page-route";
import { compareNoteOrder, noteDropPosition, noteMoveInput, type NoteDropTarget } from "./notes-tree";
import "./NotesView.css";

const NoteEditor = lazy(() => import("./NoteEditor"));
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const emptyDraft = (): NoteInput => ({ title: "", body: "", parent_id: null, icon: "", is_favorite: false });
const toInput = ({ title, body, parent_id, icon, is_favorite }: NoteInput): NoteInput => ({ title, body, parent_id, icon, is_favorite });
const ICONS = ["📝", "📌", "💡", "📚", "🎯", "🗂️", "✅", "🚀"];
function copyTitle(title: string, t: ReturnType<typeof notesText>) {
  const suffix = t("copyTitle", { title: "" });
  return [...title.trim()].slice(0, 200 - [...suffix].length).join("") + suffix;
}

interface Props {
  auth: OperatorAuth;
  projectId: string;
  canWrite: boolean;
  onDirtyChange?: (dirty: boolean) => void;
}

/** Project changes discard the previous project's component state after the cabinet's navigation guard. */
export function NotesView(props: Props) {
  const auth = useMemo(() => ({ ...props.auth, projectId: props.projectId }), [props.auth, props.projectId]);
  return <NotesWorkspace key={props.projectId} {...props} auth={auth} />;
}

function NotesWorkspace({ auth, canWrite, onDirtyChange }: Props) {
  const { locale } = useI18n();
  const t = notesText(locale);
  const shareText = noteSharesText(locale);
  const route = usePageRoute();
  const selectedId = route.segments[0] === "new" && canWrite ? "new" : UUID.test(route.segments[0] ?? "") ? route.segments[0] : null;
  useCanonicalPageRoute(route, selectedId ? [selectedId] : []);
  const [notes, setNotes] = useState<NoteSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [reload, setReload] = useState(0);
  const [search, setSearch] = useState("");
  const [favorites, setFavorites] = useState(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [showPages, setShowPages] = useState(true);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [treeBusy, setTreeBusy] = useState(false);
  const [pageLoading, setPageLoading] = useState(!!selectedId && selectedId !== "new");
  const [detailVersion, setDetailVersion] = useState<{ id: string; version: number } | null>(null);
  const [treeError, setTreeError] = useState<"moveError" | "moveConflict" | null>(null);
  const [relocation, setRelocation] = useState<Note | null>(null);
  const [draggedId, setDraggedId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<NoteDropTarget | null>(null);
  const dragSource = useRef<string | null>(null);
  const movePending = useRef(false);
  const mounted = useRef(true);
  const treeRef = useRef<HTMLElement>(null);
  const moveHelpId = useId();
  const [seed, setSeed] = useState({ input: emptyDraft(), revision: 0 });
  const [notice, setNotice] = useState("");
  const [shareScope, setShareScope] = useState<{ noteId: string | null; title?: string } | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    listNotes(auth, controller.signal).then((items) => {
      if (controller.signal.aborted) return;
      setNotes(items); setFailed(false);
    }).catch(() => { if (!controller.signal.aborted) setFailed(true); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [auth, reload]);

  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => { onDirtyChange?.(dirty || busy || treeBusy); }, [dirty, busy, treeBusy, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);
  useEffect(() => {
    if (!dirty && !busy && !treeBusy) return;
    const beforeUnload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ""; };
    window.addEventListener("beforeunload", beforeUnload);
    return () => window.removeEventListener("beforeunload", beforeUnload);
  }, [dirty, busy, treeBusy]);

  function open(id: string | null) {
    if (busy || treeBusy || id === selectedId || (dirty && !window.confirm(t("discardConfirm")))) return;
    setNotice(""); setDirty(false);
    setPageLoading(!!id && id !== "new");
    if (id) setShowPages(false);
    void route.navigate(id ? [id] : []);
  }

  function newPage(input: NoteInput = emptyDraft()) {
    if (!canWrite || busy || treeBusy || (dirty && !window.confirm(t("discardConfirm")))) return;
    setSeed((previous) => ({ input, revision: previous.revision + 1 }));
    setNotice(""); setDirty(false); setShowPages(false);
    void route.navigate(["new"]);
  }

  function saved(note: Note, wasNew: boolean) {
    setNotes((items) => [...items.filter((item) => item.id !== note.id), note]);
    setDirty(false);
    if (note.parent_id) setCollapsed((current) => { const next = new Set(current); next.delete(note.parent_id!); return next; });
    if (wasNew) void route.navigate([note.id], { replace: true });
  }

  const query = search.trim().toLocaleLowerCase(locale);
  const sortedNotes = [...notes].sort(compareNoteOrder);
  const filtered = query !== "" || favorites;
  const canMove = canWrite && !filtered && !loading && !failed && !busy && !treeBusy && !pageLoading;
  const visibleNotes = sortedNotes.filter((note) => (!favorites || note.is_favorite) && note.title.toLocaleLowerCase(locale).includes(query));
  const byId = new Map(notes.map((note) => [note.id, note]));
  function endDrag() { dragSource.current = null; setDraggedId(null); setDropTarget(null); }
  async function relocate(id: string, target: NoteDropTarget, restoreFocus = false) {
    const input = noteMoveInput(notes, id, target);
    if (!canMove || movePending.current || !input) { endDrag(); return; }
    if (id === selectedId) {
      if (detailVersion?.id !== id) { endDrag(); return; }
      input.expected_version = detailVersion.version;
    }
    movePending.current = true; setTreeBusy(true); setTreeError(null); setNotice(""); endDrag();
    try {
      const result = await moveNote(auth, id, input);
      if (!mounted.current) return;
      setNotes(result.items); setRelocation(result.note); setNotice(t("moved"));
      if (target.position === "inside" && target.id) setCollapsed((current) => { const next = new Set(current); next.delete(target.id!); return next; });
      if (restoreFocus) window.requestAnimationFrame(() => treeRef.current?.querySelector<HTMLButtonElement>(`[data-note-id="${id}"] .notes-tree-grip`)?.focus());
    } catch (failure) {
      if (mounted.current) setTreeError(failure instanceof ApiRequestError && failure.status === 409 ? "moveConflict" : "moveError");
    } finally { movePending.current = false; if (mounted.current) setTreeBusy(false); }
  }
  function dragOver(event: DragEvent<HTMLElement>, target: NoteDropTarget) {
    const source = dragSource.current;
    // Accept valid hover positions even if unchanged: the pointer may cross a row edge before the next dragover.
    if (!canMove || !source || !noteMoveInput(notes, source, target, true)) { event.dataTransfer.dropEffect = "none"; setDropTarget(null); return; }
    event.preventDefault(); event.stopPropagation(); event.dataTransfer.dropEffect = "move"; setDropTarget(target);
  }
  function drop(event: DragEvent<HTMLElement>, target: NoteDropTarget) {
    event.preventDefault(); event.stopPropagation();
    const source = dragSource.current;
    if (source) void relocate(source, target); else endDrag();
  }
  function keyboardMove(event: KeyboardEvent<HTMLButtonElement>, note: NoteSummary) {
    if (!event.altKey || !["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault(); event.stopPropagation(); if (!canMove) return;
    const siblings = sortedNotes.filter((item) => item.parent_id === note.parent_id);
    const index = siblings.findIndex((item) => item.id === note.id);
    const previous = siblings[index - 1]; const next = siblings[index + 1];
    const target: NoteDropTarget | null = event.key === "ArrowUp" && previous ? { id: previous.id, position: "before" }
      : event.key === "ArrowDown" && next ? { id: next.id, position: "after" }
      : event.key === "ArrowRight" && previous ? { id: previous.id, position: "inside" }
      : event.key === "ArrowLeft" && note.parent_id ? { id: note.parent_id, position: "after" } : null;
    if (target) void relocate(note.id, target, true);
  }
  const dropLabel = dropTarget ? t(dropTarget.position === "root" ? "moveRoot" : dropTarget.position === "inside" ? "moveInside" : dropTarget.position === "before" ? "moveBefore" : "moveAfter", { title: byId.get(dropTarget.id ?? "")?.title ?? "" }) : "";
  function rows(parentId: string | null, depth = 0, ancestors = new Set<string>()): ReactNode {
    const children = filtered ? visibleNotes : sortedNotes.filter((note) => note.parent_id === parentId || (parentId === null && note.parent_id && !byId.has(note.parent_id)));
    return children.filter((note) => !ancestors.has(note.id)).map((note) => {
      const hasChildren = !filtered && notes.some((item) => item.parent_id === note.id);
      const isCollapsed = collapsed.has(note.id);
      return <li key={note.id}>
        <div className="notes-tree-row" style={{ paddingInlineStart: `${Math.min(depth, 8) * 14}px` }} data-note-id={note.id} data-selected={selectedId === note.id || undefined}
          data-dragging={draggedId === note.id || undefined} data-drop={dropTarget?.id === note.id ? dropTarget.position : undefined}
          onDragEnter={(event) => dragOver(event, { id: note.id, position: noteDropPosition(event.clientY, event.currentTarget.getBoundingClientRect()) })}
          onDragOver={(event) => dragOver(event, { id: note.id, position: noteDropPosition(event.clientY, event.currentTarget.getBoundingClientRect()) })}
          onDragLeave={(event) => { if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setDropTarget(null); }}
          onDrop={(event) => drop(event, { id: note.id, position: noteDropPosition(event.clientY, event.currentTarget.getBoundingClientRect()) })}>
          {canWrite && <button type="button" className="notes-tree-grip" aria-label={t("movePage", { title: note.title })} aria-describedby={moveHelpId}
            title={t("movePage", { title: note.title })} draggable={canMove} disabled={!canMove} onClick={(event) => event.stopPropagation()} onKeyDown={(event) => keyboardMove(event, note)}
            onDragStart={(event) => { if (!canMove) { event.preventDefault(); return; } dragSource.current = note.id; setDraggedId(note.id); event.dataTransfer.effectAllowed = "move"; event.dataTransfer.setData("application/x-tz-note-id", note.id); }} onDragEnd={endDrag}><GripVertical size={13} aria-hidden="true" /></button>}
          {hasChildren ? <button type="button" className="notes-tree-toggle" aria-label={t(isCollapsed ? "expand" : "collapse", { title: note.title })}
            aria-expanded={!isCollapsed} onClick={() => setCollapsed((current) => {
              const next = new Set(current); if (next.has(note.id)) next.delete(note.id); else next.add(note.id); return next;
            })}>{isCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}</button> : <span className="notes-tree-spacer" />}
          <button type="button" className="notes-tree-page" aria-current={selectedId === note.id ? "page" : undefined} disabled={busy || treeBusy} onClick={() => open(note.id)}>
            {note.icon ? <span className="notes-page-icon" aria-hidden="true">{note.icon}</span> : <FileText size={16} aria-hidden="true" />}
            <span className="notes-tree-title">{note.title}{filtered && note.parent_id && <small>{byId.get(note.parent_id)?.title}</small>}</span>
            {note.is_favorite && <Star size={12} className="notes-favorite-mark" aria-label={t("favorites")} />}
          </button>
        </div>
        {hasChildren && <AnimatedDisclosure open={!isCollapsed}><ul>{rows(note.id, depth + 1, new Set([...ancestors, note.id]))}</ul></AnimatedDisclosure>}
      </li>;
    });
  }

  return <section className="page notes-workspace" data-sidebar-open={showPages || !selectedId} aria-label={t("title")}
    onDropCapture={(event) => {
      if (dragSource.current && (!(event.target instanceof Element) || !event.target.closest(".notes-tree-row, .notes-root-drop"))) { event.preventDefault(); event.stopPropagation(); endDrag(); }
    }}>
    <header className="notes-header">
      <div><button type="button" className="notes-icon-button notes-sidebar-trigger" aria-label={t(showPages ? "hidePages" : "showPages")}
        aria-expanded={showPages} onClick={() => setShowPages((value) => !value)}>{showPages ? <PanelLeftClose size={18} /> : <PanelLeftOpen size={18} />}</button>
        <h1>{t("title")}</h1>{!canWrite && <span className="notes-readonly-label">{t("readOnly")}</span>}</div>
      {canWrite && <div><button type="button" className="notes-icon-button" disabled={busy || treeBusy} aria-label={shareText("shareProject")} title={shareText("shareProject")}
        onClick={() => setShareScope({ noteId: null })}><Share2 size={17} /></button>
        <button type="button" className="notes-button" disabled={busy || treeBusy} onClick={() => newPage()}><Plus size={16} />{t("newPage")}</button></div>}
    </header>
    <div className="notes-layout">
      <aside className="notes-sidebar" aria-label={t("pages")}>
        <label className="notes-search"><Search size={16} aria-hidden="true" /><input type="search" value={search} aria-label={t("search")} placeholder={t("search")} disabled={treeBusy}
          onChange={(event) => { endDrag(); setSearch(event.target.value); }} /></label>
        <div className="notes-list-controls">
          <button type="button" aria-pressed={!favorites} disabled={treeBusy} onClick={() => { endDrag(); setFavorites(false); }}><FolderOpen size={15} />{t("allPages")}</button>
          <button type="button" aria-pressed={favorites} disabled={treeBusy} onClick={() => { endDrag(); setFavorites(true); }}><Star size={15} />{t("favorites")}</button>
          <button type="button" className="notes-icon-button" aria-label={t("refresh")} disabled={loading || busy || treeBusy} onClick={() => { endDrag(); setLoading(true); setReload((value) => value + 1); }}><RefreshCw size={14} /></button>
        </div>
        {loading && <p className="notes-sidebar-message" role="status">{t("loading")}</p>}
        {failed && <div className="notes-sidebar-message" role="alert"><p>{t("loadError")}</p><button type="button" className="notes-button" disabled={loading}
          onClick={() => { setLoading(true); setReload((value) => value + 1); }}>{t("retry")}</button></div>}
        {!failed && !loading && visibleNotes.length === 0 && <p className="notes-sidebar-message">{t(query ? "noResults" : favorites ? "noFavorites" : "empty")}</p>}
        {canWrite && <p id={moveHelpId} className={filtered ? "notes-sidebar-message" : "visually-hidden"}>{t(filtered ? "moveFiltered" : "moveHelp")}</p>}
        {treeError && <p role="alert" className="notes-sidebar-message notes-tree-error">{t(treeError)}</p>}
        <nav ref={treeRef} aria-label={t("pages")}><ul className="notes-tree">{rows(null)}</ul></nav>
        {canWrite && !filtered && notes.length > 0 && <div className="notes-root-drop" data-drop={dropTarget?.position === "root" || undefined} aria-label={t("moveRoot")}
          onDragEnter={(event) => dragOver(event, { id: null, position: "root" })}
          onDragOver={(event) => dragOver(event, { id: null, position: "root" })} onDragLeave={() => setDropTarget(null)} onDrop={(event) => drop(event, { id: null, position: "root" })}>{t("moveRoot")}</div>}
        <p className="notes-tree-drop-label" role="status" aria-live="polite">{treeBusy ? t("moving") : dropLabel}</p>
      </aside>
      <main className="notes-document">
        {notice && <p className="notes-notice" role="status">{notice}</p>}
        {selectedId ? <NotePage key={`${selectedId}:${selectedId === "new" ? seed.revision : ""}`} auth={auth} id={selectedId} initial={seed.input}
          notes={notes} canWrite={canWrite} treeBusy={treeBusy} relocation={relocation} onVersionChange={setDetailVersion} onLoadingChange={setPageLoading} onDirtyChange={setDirty} onBusyChange={setBusy} onSaved={saved}
          onNew={(input) => newPage(input)} onBack={() => open(null)} onShare={(note) => setShareScope({ noteId: note.id, title: note.title })} onDeleted={(id) => {
            setNotes((items) => items.filter((item) => item.id !== id)); setNotice(t("deleted")); setDirty(false);
            void route.navigate([], { replace: true });
          }} /> : <div className="notes-empty">
          <FileText size={30} strokeWidth={1.4} aria-hidden="true" /><h2>{t(notes.length ? "selectPage" : "empty")}</h2>
          <p>{t(notes.length ? "selectHelp" : "emptyHelp")}</p>
          {canWrite && <button type="button" className="notes-button notes-button-primary" onClick={() => newPage()}><Plus size={16} />{t("newPage")}</button>}
        </div>}
      </main>
    </div>
    {shareScope && <NoteShareDialog auth={auth} noteId={shareScope.noteId} title={shareScope.title} onClose={() => setShareScope(null)} />}
  </section>;
}

interface NotePageProps {
  auth: OperatorAuth;
  id: string;
  initial: NoteInput;
  notes: NoteSummary[];
  canWrite: boolean;
  treeBusy: boolean;
  relocation: Note | null;
  onLoadingChange: (loading: boolean) => void;
  onVersionChange: (value: { id: string; version: number } | null) => void;
  onDirtyChange: (dirty: boolean) => void;
  onBusyChange: (busy: boolean) => void;
  onSaved: (note: Note, wasNew: boolean) => void;
  onNew: (input: NoteInput) => void;
  onBack: () => void;
  onDeleted: (id: string) => void;
  onShare: (note: Note) => void;
}

function NotePage({ auth, id, initial, notes, canWrite, treeBusy, relocation, onLoadingChange, onVersionChange, onDirtyChange, onBusyChange, onSaved, onNew, onBack, onDeleted, onShare }: NotePageProps) {
  const { locale } = useI18n();
  const t = notesText(locale);
  const shareText = noteSharesText(locale);
  const isNew = id === "new";
  const [note, setNote] = useState<Note | null>(null);
  const [draft, setDraft] = useState<NoteInput | null>(isNew ? initial : null);
  const [baseline, setBaseline] = useState<NoteInput>(emptyDraft);
  const [loading, setLoading] = useState(!isNew);
  const [loadError, setLoadError] = useState<"detailError" | "notFound" | null>(null);
  const [error, setError] = useState<"saveError" | "deleteError" | "hasChildren" | "conflict" | null>(null);
  const [busy, setBusy] = useState(false);
  const [tablesDirty, setTablesDirty] = useState(false);
  const [tablesBusy, setTablesBusy] = useState(false);
  const [appliedRelocation, setAppliedRelocation] = useState<Note | null>(null);
  const databaseClient = useMemo(() => createNoteDatabaseClient(auth), [auth]);
  const [reload, setReload] = useState(0);
  const active = useRef(true);
  const childWarningId = useId();
  const dirty = !!draft && JSON.stringify(draft) !== JSON.stringify(baseline);
  const canEdit = canWrite && !busy && !tablesBusy && !treeBusy && !loading;
  const bodyTooLarge = !!draft && [...draft.body].length > 200_000;

  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  useEffect(() => { onLoadingChange(loading); return () => onLoadingChange(false); }, [loading, onLoadingChange]);
  useEffect(() => { onVersionChange(note ? { id: note.id, version: note.version } : null); return () => onVersionChange(null); }, [note, onVersionChange]);
  if (relocation && relocation.id === id && appliedRelocation !== relocation) {
    setAppliedRelocation(relocation);
    const { parent_id, version, sort_order, updated_at } = relocation;
    setNote((current) => current ? { ...current, parent_id, version, sort_order, updated_at } : current);
    setDraft((current) => current ? { ...current, parent_id } : current);
    setBaseline((current) => ({ ...current, parent_id }));
  }
  useEffect(() => { onDirtyChange(dirty || tablesDirty); return () => onDirtyChange(false); }, [dirty, tablesDirty, onDirtyChange]);
  useEffect(() => { onBusyChange(busy || tablesBusy); return () => onBusyChange(false); }, [busy, tablesBusy, onBusyChange]);
  async function embedsChanged() {
    const latest = await getNote(auth, id);
    if (!active.current) return;
    setNote(latest); setDraft(toInput(latest)); setBaseline(toInput(latest));
    onSaved(latest, false);
  }
  useEffect(() => {
    if (isNew) return;
    const controller = new AbortController();
    getNote(auth, id, controller.signal).then((item) => {
      if (controller.signal.aborted) return;
      setNote(item); setDraft(toInput(item)); setBaseline(toInput(item)); setLoadError(null); setError(null);
    }).catch((failure: unknown) => {
      if (!controller.signal.aborted) setLoadError(failure instanceof ApiRequestError && failure.status === 404 ? "notFound" : "detailError");
    }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [auth, id, isNew, reload]);

  function edit(changes: Partial<NoteInput>) { if (canEdit) setDraft((value) => value ? { ...value, ...changes } : value); }
  function reloadLatest() {
    if (busy || treeBusy || (dirty && !window.confirm(t("discardConfirm")))) return;
    setLoading(true); setReload((value) => value + 1);
  }

  async function save(event?: FormEvent, asCopy = false) {
    event?.preventDefault();
    if (!draft || !canEdit || !draft.title.trim() || [...draft.title.trim()].length > 200 || bodyTooLarge) return;
    setBusy(true); setError(null);
    try {
      const input = { ...draft, title: asCopy ? copyTitle(draft.title, t) : draft.title.trim() };
      const saved = isNew || asCopy ? await createNote(auth, input) : await updateNote(auth, id, input, note!.version);
      if (!active.current) return;
      setNote(saved); setDraft(toInput(saved)); setBaseline(toInput(saved));
      onSaved(saved, isNew || asCopy);
    } catch (failure) {
      if (active.current) setError(failure instanceof ApiRequestError && failure.status === 409 ? "conflict" : "saveError");
    } finally { if (active.current) setBusy(false); }
  }

  async function remove() {
    if (!note || !canEdit) return;
    if (notes.some((item) => item.parent_id === id)) { setError("hasChildren"); return; }
    if (!window.confirm(t("deleteConfirm", { title: note.title }))) return;
    setBusy(true); setError(null);
    try {
      await deleteNote(auth, id, note.version);
      if (active.current) onDeleted(id);
    } catch (failure) {
      if (active.current) setError(failure instanceof ApiRequestError && failure.status === 409
        ? failure.message.includes("child notes") ? "hasChildren" : "conflict" : "deleteError");
    } finally { if (active.current) setBusy(false); }
  }

  if (loading) return <div className="notes-document-status" role="status">{t("loading")}</div>;
  if (loadError || !draft) return <div className="notes-document-status" role="alert"><p>{t(loadError ?? "detailError")}</p>
    <button type="button" className="notes-button" onClick={reloadLatest}>{t("retry")}</button></div>;

  const excluded = new Set([id]);
  for (let size = -1; size !== excluded.size;) {
    size = excluded.size;
    notes.forEach((item) => { if (item.parent_id && excluded.has(item.parent_id)) excluded.add(item.id); });
  }
  const parentOptions = notes.filter((item) => !excluded.has(item.id)).sort((a, b) => a.title.localeCompare(b.title, locale));
  const hasChildren = notes.some((item) => item.parent_id === id);

  return <><form className="notes-page" inert={treeBusy} aria-busy={treeBusy} onSubmit={(event) => void save(event)} onKeyDown={(event) => {
    if ((event.metaKey || event.ctrlKey) && event.key === "s") { event.preventDefault(); if (dirty || isNew) void save(); }
  }}>
    <div className="notes-page-toolbar">
      <button type="button" className="notes-icon-button notes-back" aria-label={t("back")} disabled={busy || treeBusy} onClick={onBack}><ArrowLeft size={17} /></button>
      <span className="notes-save-state" role="status">{t(busy ? "saving" : dirty ? "unsaved" : canWrite ? "saved" : "readOnly")}</span>
      {note && <time className="notes-updated" dateTime={note.updated_at}>{t("updated", { date: new Date(note.updated_at).toLocaleDateString(locale, { day: "numeric", month: "short" }) })}</time>}
      {canWrite && <div className="notes-page-actions">
        {note && <button type="button" className="notes-icon-button" disabled={!canEdit || dirty} aria-label={shareText("share")} title={shareText(dirty ? "savedOnly" : "share")} onClick={() => onShare(note)}><Share2 size={16} /></button>}
        <button type="button" className="notes-icon-button" disabled={!canEdit} aria-label={t(draft.is_favorite ? "unfavorite" : "favorite")}
          aria-pressed={draft.is_favorite} onClick={() => edit({ is_favorite: !draft.is_favorite })}><Star size={17} fill={draft.is_favorite ? "currentColor" : "none"} /></button>
        {!isNew && <button type="button" className="notes-icon-button" disabled={!canEdit} aria-label={t("duplicate")} onClick={() => onNew({ ...draft, title: copyTitle(draft.title, t), is_favorite: false })}><Copy size={17} /></button>}
        {!isNew && <button type="button" className="notes-icon-button" disabled={!canEdit} aria-label={t("newChild")} onClick={() => onNew({ ...emptyDraft(), parent_id: id })}><Plus size={17} /></button>}
        <button type="submit" className="notes-button notes-button-primary" disabled={!canEdit || !draft.title.trim() || bodyTooLarge || (!dirty && !isNew)}><Save size={15} />{t("save")}</button>
      </div>}
    </div>
    {error && <div className="notes-inline-error" role="alert"><p>{t(error)}</p>
      {error === "conflict" && <div className="notes-error-actions"><button type="button" className="notes-button" disabled={busy || treeBusy} onClick={reloadLatest}>{t("reloadLatest")}</button>
        <button type="button" className="notes-button" disabled={!canEdit || !draft.title.trim() || bodyTooLarge} onClick={() => void save(undefined, true)}>{t("saveCopy")}</button></div>}
    </div>}
    {bodyTooLarge && <p className="notes-inline-error" role="alert">{t("tooLarge")}</p>}
    <div className="notes-page-content">
      <div className="notes-title-row">
        {draft.icon && <span className="notes-document-icon" aria-hidden="true">{draft.icon}</span>}
        <input className="notes-title-input" aria-label={t("pageTitle")} placeholder={t("untitled")} value={draft.title} maxLength={200} readOnly={!canEdit} required
          onChange={(event) => edit({ title: event.target.value })} />
      </div>
      {canWrite && <AnimatedDetails className="notes-properties"><summary>{t("properties")}</summary><div>
        <div className="notes-properties-field"><span>{t("parent")}</span><PreferenceDropdown
          label={t("parent")} value={draft.parent_id ?? ""} disabled={!canEdit} onChange={(value) => edit({ parent_id: value || null })}
          search={{ placeholder: t("search"), emptyLabel: t("noResults") }}
          options={[{ value: "", label: t("root") }, ...parentOptions.map((item) => ({ value: item.id, label: `${item.icon ? `${item.icon} ` : ""}${item.title}` }))]}
        /></div>
        <label>{t("icon")}<select value={draft.icon} disabled={!canEdit} onChange={(event) => edit({ icon: event.target.value })}>
          <option value="">{t("noIcon")}</option>{draft.icon && !ICONS.includes(draft.icon) && <option>{draft.icon}</option>}{ICONS.map((icon) => <option key={icon} value={icon}>{icon}</option>)}
        </select></label>
      </div></AnimatedDetails>}
      <Suspense fallback={<p className="notes-document-status" role="status">{t("loading")}</p>}><NoteEditor value={draft.body} onChange={(body) => edit({ body })} readOnly={!canWrite || busy || tablesBusy || loading} /></Suspense>
      {canWrite && <footer className="notes-page-footer">
        <button type="button" className="notes-text-button" disabled={!canEdit || !dirty} onClick={() => { if (window.confirm(t("discardConfirm"))) { setDraft(baseline); setError(null); } }}>{t("discard")}</button>
        {!isNew && <button type="button" className="notes-text-button notes-delete" disabled={!canEdit} aria-describedby={hasChildren ? childWarningId : undefined} onClick={() => void remove()}><Trash2 size={14} />{t("delete")}</button>}
        {hasChildren && <p id={childWarningId}>{t("hasChildren")}</p>}
      </footer>}
    </div>
  </form>{note && <NoteDatabaseEmbeds key={note.id} client={databaseClient} noteId={note.id} canWrite={canWrite && !busy} suspended={treeBusy}
    canManage={canWrite && !dirty && !busy} onNoteChanged={embedsChanged} onDirtyChange={setTablesDirty} onBusyChange={setTablesBusy} />}</>;
}
