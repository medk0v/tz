import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";
import { useLocation, useNavigate } from "react-router";
import { Copy, FileText, LockKeyhole, Menu, Save, Search } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useI18n } from "./i18n";
import { noteSharesText } from "./note-shares-i18n";
import { PublicNoteShareError, publicNoteSharePath, resolvePublicNotes, unlockPublicNotes, updatePublicNote, type PublicNote, type PublicNotes } from "./note-shares-api";
import { createPublicNoteDatabaseClient } from "./note-databases-api";
import { NoteDatabaseEmbeds } from "./admin/NoteDatabaseEmbeds";
import {  } from "./admin/PreferenceDropdown";
import {  } from "./admin/preference-options";
import { usePublicNotesTheme } from "./public-notes-theme";
import "./admin/NotesView.css";
import "./PublicNotesPage.css";

const NoteEditor = lazy(() => import("./admin/NoteEditor"));
type Draft = Pick<PublicNote, "title" | "body" | "icon">;
type DocumentState = { note: PublicNote; draft: Draft };
type Message = Parameters<ReturnType<typeof noteSharesText>>[0];
const draftOf = ({ title, body, icon }: PublicNote): Draft => ({ title, body, icon });
const isDirty = (page: DocumentState | null) => !!page && JSON.stringify(page.draft) !== JSON.stringify(draftOf(page.note));
const ICONS = ["📝", "📌", "💡", "📚", "🎯", "🗂️", "✅", "🚀"];

function safeDocumentLink(href: string | undefined) {
  try { return href && ["https:", "http:", "mailto:"].includes(new URL(href).protocol) ? href : undefined; }
  catch { return undefined; }
}

interface Props { token: string; onDirtyChange?: (dirty: boolean) => void }

export function PublicNotesPage(props: Props) {
  return <PublicNotesWorkspace key={props.token} {...props} />;
}

function PublicNotesWorkspace({ token, onDirtyChange }: Props) {
  const { locale } = useI18n();
  const t = noteSharesText(locale);
  const location = useLocation();
  const navigate = useNavigate();
  const selectedId = new URLSearchParams(location.search).get("note") || undefined;
  const [accessToken, setAccessToken] = useState<string>();
  const [data, setData] = useState<PublicNotes | null>(null);
  usePublicNotesTheme();
  const [page, setPage] = useState<DocumentState | null>(null);
  const [resolvedId, setResolvedId] = useState<string | undefined>(selectedId);
  const [loading, setLoading] = useState(true);
  const [reload, setReload] = useState(0);
  const [gate, setGate] = useState<"password" | "unavailable" | null>(null);
  const [loadError, setLoadError] = useState<Message | null>(null);
  const [saveError, setSaveError] = useState<Message | null>(null);
  const [notice, setNotice] = useState<Message | null>(null);
  const [permissionDenied, setPermissionDenied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [tablesDirty, setTablesDirty] = useState(false);
  const [tablesBusy, setTablesBusy] = useState(false);
  const [password, setPassword] = useState("");
  const [unlocking, setUnlocking] = useState(false);
  const [passwordError, setPasswordError] = useState<Message | null>(null);
  const databaseUnauthorized = useCallback(() => { setGate("password"); setPasswordError("accessExpired"); }, []);
  const databaseClient = useMemo(() => createPublicNoteDatabaseClient(token, accessToken, databaseUnauthorized), [token, accessToken, databaseUnauthorized]);
  const [search, setSearch] = useState("");
  const [showPages, setShowPages] = useState(false);
  const active = useRef(true);
  const currentSelection = useRef(selectedId);
  const defaultNoteId = useRef<string | undefined>(undefined);
  const replaceDraft = useRef(false);
  const dirty = isDirty(page);
  const pageLoading = loading || selectedId !== resolvedId;
  const canEdit = !!data?.can_edit && !permissionDenied && !gate && !pageLoading && !busy;
  const invalid = !!page && (!page.draft.title.trim() || [...page.draft.title.trim()].length > 200 || [...page.draft.body].length > 200_000 || [...page.draft.icon].length > 32);

  useEffect(() => { const previous = document.title; return () => { document.title = previous; }; }, []);
  useEffect(() => { document.title = page?.note.title ?? ""; }, [page?.note.title]);

  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  useEffect(() => { currentSelection.current = selectedId; }, [selectedId]);
  useEffect(() => { onDirtyChange?.(dirty || busy || tablesDirty || tablesBusy); }, [dirty, busy, tablesDirty, tablesBusy, onDirtyChange]);
  useEffect(() => () => onDirtyChange?.(false), [onDirtyChange]);
  useEffect(() => {
    if (!dirty && !busy && !tablesDirty && !tablesBusy) return;
    const beforeUnload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ""; };
    window.addEventListener("beforeunload", beforeUnload);
    return () => window.removeEventListener("beforeunload", beforeUnload);
  }, [dirty, busy, tablesDirty, tablesBusy]);
  useEffect(() => {
    const restored = ["referrer", "robots"].map((name) => {
      const existing = document.head.querySelector<HTMLMetaElement>(`meta[name="${name}"]`);
      const element = existing ?? document.createElement("meta");
      const previous = element.content;
      element.name = name; element.content = name === "referrer" ? "no-referrer" : "noindex,nofollow";
      if (!existing) document.head.append(element);
      return () => { if (existing) element.content = previous; else element.remove(); };
    });
    return () => restored.forEach((restore) => restore());
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    const requestedNoteId = selectedId ?? defaultNoteId.current;
    resolvePublicNotes(token, accessToken, requestedNoteId, controller.signal).then((result) => {
      if (controller.signal.aborted) return;
      const forceReplace = replaceDraft.current; replaceDraft.current = false;
      if (selectedId === undefined && result.note && !defaultNoteId.current) defaultNoteId.current = result.note.id;
      setData(result); setGate(null); setLoadError(null); setPermissionDenied(false);
      setPage((current) => {
        if (!forceReplace && current && isDirty(current) && requestedNoteId === current.note.id) return current;
        if (!result.note) return null;
        if (!forceReplace && current?.note.id === result.note.id && isDirty(current)) return current;
        return { note: result.note, draft: draftOf(result.note) };
      });
      setSaveError(null);
    }).catch((failure: unknown) => {
      if (controller.signal.aborted) return;
      if (failure instanceof PublicNoteShareError && failure.status === 401) { setGate("password"); setPasswordError(accessToken ? "accessExpired" : null); }
      else if (failure instanceof PublicNoteShareError && (failure.status === 404 || failure.status === 403)) { setGate("unavailable"); setLoadError(selectedId ? "selectedUnavailable" : "unavailable"); }
      else setLoadError(failure instanceof PublicNoteShareError && failure.status === 429 ? "tooMany" : "publicLoadError");
    }).finally(() => { if (!controller.signal.aborted) { setLoading(false); setResolvedId(selectedId); } });
    return () => controller.abort();
  }, [token, accessToken, selectedId, reload]);

  function openNote(id?: string) {
    if (busy || tablesBusy || ((dirty || tablesDirty) && !window.confirm(t("discardConfirm")))) return;
    onDirtyChange?.(false);
    if (!id) defaultNoteId.current = undefined;
    setPage(null); setSaveError(null); setNotice(null); setShowPages(false); setLoading(true);
    void navigate(publicNoteSharePath(token, id));
    if (id === selectedId) setReload((value) => value + 1);
  }

  function refresh(force = false) {
    if (busy || tablesBusy || (force && (dirty || tablesDirty) && !window.confirm(t("discardConfirm")))) return;
    replaceDraft.current = force;
    setLoading(true); setLoadError(null); setReload((value) => value + 1);
  }

  async function unlock(event: FormEvent) {
    event.preventDefault();
    if (!password || unlocking) return;
    setUnlocking(true); setPasswordError(null);
    try {
      const response = await unlockPublicNotes(token, password);
      if (!active.current) return;
      setPassword(""); setLoading(true);
      setAccessToken(response.access_token);
      // A repeated token still needs a fresh resolution after access was checked.
      if (response.access_token === accessToken) setReload((value) => value + 1);
    } catch (failure) {
      if (!active.current) return;
      if (failure instanceof PublicNoteShareError && failure.status === 404) setGate("unavailable");
      else setPasswordError(failure instanceof PublicNoteShareError && failure.status === 401 ? "wrongPassword"
        : failure instanceof PublicNoteShareError && failure.status === 429 ? "tooMany" : "unlockError");
    } finally { if (active.current) setUnlocking(false); }
  }

  function edit(change: Partial<Draft>) { if (canEdit) setPage((current) => current ? { ...current, draft: { ...current.draft, ...change } } : null); }

  async function save(event?: FormEvent) {
    event?.preventDefault();
    if (!page || !canEdit || !dirty || invalid) return;
    const selectionAtSave = selectedId;
    setBusy(true); setSaveError(null); setNotice(null);
    try {
      const saved = await updatePublicNote(token, accessToken, page.note.id, { ...page.draft, title: page.draft.title.trim(), expected_version: page.note.version });
      if (!active.current || currentSelection.current !== selectionAtSave) return;
      setPage({ note: saved, draft: draftOf(saved) });
      setData((current) => current ? { ...current, note: current.note?.id === saved.id ? saved : current.note, items: current.items.map((item) => item.id === saved.id ? saved : item) } : current);
    } catch (failure) {
      if (!active.current || currentSelection.current !== selectionAtSave) return;
      if (failure instanceof PublicNoteShareError && failure.status === 401) { setGate("password"); setPasswordError("accessExpired"); }
      else if (failure instanceof PublicNoteShareError && failure.status === 403) { setPermissionDenied(true); setSaveError("permissionChanged"); }
      else if (failure instanceof PublicNoteShareError && failure.status === 404) { setGate("unavailable"); setSaveError("draftUnavailable"); }
      else setSaveError(failure instanceof PublicNoteShareError && failure.status === 409 ? "conflict" : "saveError");
    } finally { if (active.current) setBusy(false); }
  }

  async function copyDraft() {
    if (!page) return;
    try { await navigator.clipboard.writeText(`# ${page.draft.title}\n\n${page.draft.body}`); if (active.current) setNotice("draftCopied"); }
    catch { if (active.current) setNotice("draftCopyFailed"); }
  }

  const items = data?.items ?? [];
  const hasNavigation = data?.root_note_id ? data.include_descendants && items.some((item) => item.id !== data.root_note_id) : items.length > 1;
  const showEditor = !!data?.can_edit || dirty;
  const query = search.trim().toLocaleLowerCase(locale);
  const ids = new Set(items.map((item) => item.id));
  function renderPages(parent: string | null, depth = 0, visited = new Set<string>()): ReactNode {
    const children = items.filter((item) => query ? item.title.toLocaleLowerCase(locale).includes(query)
      : item.parent_id === parent || (!parent && item.parent_id && !ids.has(item.parent_id)));
    return children.filter((item) => !visited.has(item.id)).map((item) => <li key={item.id}>
      <button type="button" aria-current={page?.note.id === item.id ? "page" : undefined} disabled={busy} style={{ paddingInlineStart: `${10 + Math.min(depth, 8) * 14}px` }}
        onClick={() => { if (page?.note.id !== item.id) openNote(item.id); }}>
        {item.icon ? <span aria-hidden="true">{item.icon}</span> : <FileText size={15} aria-hidden="true" />}<span>{item.title}</span></button>
      {!query && <ul>{renderPages(item.id, depth + 1, new Set([...visited, item.id]))}</ul>}
    </li>);
  }

  return <div className="public-notes">
    {gate === "password" && <section className="public-notes-gate" aria-labelledby="public-notes-password-title"><LockKeyhole size={25} aria-hidden="true" />
      <h1 id="public-notes-password-title">{t("passwordTitle")}</h1><p>{t("passwordPrompt")}</p>
      <form onSubmit={(event) => void unlock(event)}><label>{t("password")}<input type="password" value={password} autoComplete="current-password" required autoFocus
        disabled={unlocking} onChange={(event) => setPassword(event.target.value)} /></label>
        {passwordError && <p className="public-notes-error" role="alert">{t(passwordError)}</p>}
        <button className="primary-button" type="submit" disabled={unlocking || !password}>{t(unlocking ? "unlocking" : "unlock")}</button></form>
    </section>}
    {gate === "unavailable" && <section className="public-notes-gate" role="alert"><h1>{t(loadError ?? "unavailable")}</h1><p>{t("unavailableHelp")}</p>
      {selectedId && !dirty && <button type="button" className="secondary-button" onClick={() => openNote()}>{t("pages")}</button>}</section>}
    {pageLoading && <p className="public-notes-status" role="status">{t("publicLoading")}</p>}
    {!gate && loadError && <div className="public-notes-status" role="alert"><p>{t(loadError)}</p><button type="button" className="secondary-button" disabled={pageLoading} onClick={() => refresh()}>{t("retry")}</button></div>}
    {data && (!gate || dirty || tablesDirty || tablesBusy) && <div className="public-notes-layout" data-menu-open={showPages} data-navigation={hasNavigation && !gate}>
      {hasNavigation && !gate && <><button className="public-notes-menu" type="button" aria-expanded={showPages} onClick={() => setShowPages((value) => !value)}><Menu size={17} />{t(showPages ? "hidePages" : "showPages")}</button>
        <aside className="public-notes-sidebar"><label className="public-notes-search"><Search size={15} aria-hidden="true" /><input type="search" value={search} aria-label={t("search")} placeholder={t("search")} onChange={(event) => setSearch(event.target.value)} /></label>
          <nav aria-label={t("pages")}><ul>{renderPages(null)}</ul></nav>{query && !items.some((item) => item.title.toLocaleLowerCase(locale).includes(query)) && <p>{t("noResults")}</p>}
        </aside></>}
      <main className="public-notes-document">
        {page && (!pageLoading || dirty) ? <form onSubmit={(event) => void save(event)} onKeyDown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === "s") { event.preventDefault(); void save(); } }}>
          {showEditor && <div className="public-notes-document-toolbar"><span role="status">{t(busy ? "saving" : dirty ? "dirty" : "saved")}</span>
            {data.can_edit && !gate && !permissionDenied && <button className="primary-button" type="submit" disabled={!canEdit || !dirty || invalid}><Save size={15} />{t("save")}</button>}</div>}
          {saveError && <div className="public-notes-error" role="alert"><p>{t(saveError)}</p>
            {(saveError === "conflict" || saveError === "permissionChanged") && <button type="button" className="secondary-button" disabled={busy} onClick={() => refresh(true)}>{t("reload")}</button>}</div>}
          {notice && <p role="status" className="public-notes-notice">{t(notice)}</p>}
          {invalid && canEdit && <p className="public-notes-error" role="alert">{t("tooLarge")}</p>}
          <div className="public-notes-content">
            <div className="public-notes-title-row">{page.draft.icon && <span aria-hidden="true">{page.draft.icon}</span>}
              {showEditor ? <input className="public-notes-title" aria-label={t("titleLabel")} value={page.draft.title} readOnly={!canEdit} maxLength={200} required onChange={(event) => edit({ title: event.target.value })} />
                : <h1 className="public-notes-title">{page.draft.title}</h1>}</div>
            {canEdit && <label className="public-notes-icon-field">{t("icon")}<select value={page.draft.icon} onChange={(event) => edit({ icon: event.target.value })}>
              <option value="">{t("noIcon")}</option>{page.draft.icon && !ICONS.includes(page.draft.icon) && <option>{page.draft.icon}</option>}{ICONS.map((icon) => <option key={icon}>{icon}</option>)}
            </select></label>}
            {showEditor ? <Suspense fallback={<p role="status">{t("publicLoading")}</p>}><NoteEditor value={page.draft.body} readOnly={!canEdit} onChange={(body) => edit({ body })} /></Suspense>
              : <article className="notes-prose public-notes-prose"><ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml
                components={{ a: ({ href, children }) => <a href={safeDocumentLink(href)} target="_blank" rel="noopener noreferrer">{children}</a>, img: () => null,
                  table: ({ children }) => <div className="public-notes-table-scroll"><table>{children}</table></div> }}>{page.draft.body}</ReactMarkdown></article>}
            {dirty && <footer className="public-notes-draft-actions"><button type="button" className="secondary-button" onClick={() => void copyDraft()}><Copy size={14} />{t("copyDraft")}</button>
              {!gate && <button type="button" className="notes-text-button" disabled={busy} onClick={() => { if (window.confirm(t("discardConfirm"))) { setPage((current) => current ? { ...current, draft: draftOf(current.note) } : null); setSaveError(null); } }}>{t("discard")}</button>}</footer>}
          </div>
        </form> : !pageLoading && <p className="public-notes-status">{t(items.length ? "selectedUnavailable" : "noPages")}</p>}
        {page && <NoteDatabaseEmbeds key={page.note.id} client={databaseClient} noteId={page.note.id} suspended={pageLoading || !!gate}
          canWrite={!!data.can_edit && !permissionDenied} canManage={false} onDirtyChange={setTablesDirty} onBusyChange={setTablesBusy} />}
      </main>
    </div>}
  </div>;
}
