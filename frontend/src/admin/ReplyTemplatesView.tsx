import { visibilityEqual } from "../resource-visibility";
import { useContentMotion } from "./useContentMotion";
import { DemoActionButton } from "./DemoReadOnly";
import { lazy, Suspense, useEffect, useRef, useState, type FormEvent } from "react";
import { Plus, Save, Search, Trash2 } from "lucide-react";
import {
  createReplyTemplate,
  deleteReplyTemplate,
  listReplyTemplates,
  updateReplyTemplate,
  type Inbox,
  type OperatorAuth,
  type ReplyTemplate,
  type ReplyTemplateInput,
} from "../api";
import { MessageBody } from "../MessageBody";
import { useI18n, type MessageKey } from "../i18n";
import { replyTemplateDraftSource } from "./ai-draft-sources";
import { sameSegments, useCanonicalPageRoute, usePageRoute } from "./page-route";
import "./ReplyTemplatesView.css";

const ChatMessageEditor = lazy(() => import("./ChatMessageEditor"));
const emptyDraft: ReplyTemplateInput = { title: "", body: "" };

interface Props {
  auth: OperatorAuth;
  inboxes: Inbox[];
  activeInboxId: string | null;
  onInboxChange: (id: string) => void;
  canManage: boolean;
  onDirtyChange?: (dirty: boolean) => void;
}

type Navigation = { kind: "inbox"; id: string } | { kind: "template"; id: string | "new" };
type Notice = { kind: "success" | "error"; message: MessageKey };

function templateInput(template: ReplyTemplate): ReplyTemplateInput {
  return { visibility: template.visibility, title: template.title, body: template.body };
}

function plainPreview(body: string): string {
  return body.replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(^|\n)\s*(?:#{1,6}\s+|>\s*|[-*+]\s+|\d+\.\s+)/g, "$1")
    .replace(/[*_~`]/g, "").replace(/\s+/g, " ").trim();
}

export function ReplyTemplatesView(props: Props) {
  return <ReplyTemplatesWorkspace key={props.activeInboxId ?? "no-inbox"} {...props} />;
}

/** Reply template URLs: `/reply-templates` opens the Inbox's first template, `/reply-templates/<id>` and `/reply-templates/new`. */
function ReplyTemplatesWorkspace({ auth, inboxes, activeInboxId, onInboxChange, canManage, onDirtyChange }: Props) {
  const { t, formatDate } = useI18n();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const routeSegmentsRef = useRef(route.segments);
  const routedId = route.segments[0] ?? null;
  const [templates, setTemplates] = useState<ReplyTemplate[]>([]);
  const selectedId = routedId === "new" && canManage && activeInboxId ? "new" : templates.find((template) => template.id === routedId)?.id ?? templates[0]?.id ?? null;
  // Unsaved edits belong to what the URL shows, so every other URL starts from the saved template.
  const selectionKey = `${routedId ?? ""}/${selectedId ?? ""}`;
  const editorMotion = useContentMotion<HTMLDivElement>(`${activeInboxId}:${selectedId}`);
  const [draft, setDraft] = useState<ReplyTemplateInput>(emptyDraft);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(Boolean(activeInboxId));
  const [loadError, setLoadError] = useState(false);
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteConfirmation, setDeleteConfirmation] = useState(false);
  const [pendingNavigation, setPendingNavigation] = useState<Navigation | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const mounted = useRef(true);
  const titleInput = useRef<HTMLInputElement>(null);
  const discardButton = useRef<HTMLButtonElement>(null);
  const deleteButton = useRef<HTMLButtonElement>(null);
  const selected = templates.find((template) => template.id === selectedId);
  const [shownKey, setShownKey] = useState(selectionKey);
  if (selectionKey !== shownKey) {
    setShownKey(selectionKey);
    setDraft(selected ? templateInput(selected) : emptyDraft);
    setPendingNavigation(null);
    setDeleteConfirmation(false);
  }
  const dirty = selectedId !== null && (draft.title !== (selected?.title ?? "") || draft.body !== (selected?.body ?? "") || !visibilityEqual(draft.visibility, selected?.visibility));
  const busy = saving || deleting;
  const titleTooLong = draft.title.length > 120;
  const bodyTooLong = draft.body.length > 10_000;
  const query = search.trim().toLocaleLowerCase();
  const filtered = templates.filter((template) => !query || `${template.title} ${plainPreview(template.body)}`.toLocaleLowerCase().includes(query));

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  useEffect(() => {
    if (!activeInboxId) return;
    const controller = new AbortController();
    listReplyTemplates(auth, activeInboxId, controller.signal)
      .then((items) => {
        if (controller.signal.aborted) return;
        setTemplates(items);
        setLoadError(false);
      })
      .catch(() => {
        if (!controller.signal.aborted) setLoadError(true);
      })
      .finally(() => {
        if (!controller.signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [activeInboxId, auth, loadAttempt]);

  useEffect(() => { routeSegmentsRef.current = route.segments; }, [route.segments]);
  useCanonicalPageRoute(route, selectedId === "new" ? ["new"] : selectedId && selectedId === routedId ? [selectedId] : [], !loading && !loadError);

  useEffect(() => {
    onDirtyChange?.(dirty);
    return () => onDirtyChange?.(false);
  }, [dirty, onDirtyChange]);

  useEffect(() => {
    if (!dirty) return;
    const beforeUnload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = ""; };
    window.addEventListener("beforeunload", beforeUnload);
    return () => window.removeEventListener("beforeunload", beforeUnload);
  }, [dirty]);

  useEffect(() => {
    if (pendingNavigation) discardButton.current?.focus();
    else if (deleteConfirmation) deleteButton.current?.focus();
  }, [deleteConfirmation, pendingNavigation]);

  function navigate(target: Navigation) {
    setPendingNavigation(null);
    setDeleteConfirmation(false);
    setNotice(null);
    if (target.kind === "inbox") {
      // The open template belongs to the current Inbox.
      if (route.segments.length) void navigateRoute([]);
      onInboxChange(target.id);
      return;
    }
    void navigateRoute([target.id]);
    if (canManage) requestAnimationFrame(() => titleInput.current?.focus());
  }

  function requestNavigation(target: Navigation) {
    if (busy || (target.kind === "template" && target.id === selectedId)) return;
    if (dirty) {
      setDeleteConfirmation(false);
      setPendingNavigation(target);
    } else navigate(target);
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!canManage || !activeInboxId || !selectedId || busy || !draft.title.trim() || !draft.body.trim()
      || titleTooLong || bodyTooLong || pendingNavigation || deleteConfirmation) return;
    setSaving(true);
    setNotice(null);
    const opened = route.segments;
    const input = { visibility: draft.visibility, title: draft.title.trim(), body: draft.body.trim() };
    try {
      const saved = selectedId === "new"
        ? await createReplyTemplate(auth, activeInboxId, input)
        : await updateReplyTemplate(auth, activeInboxId, selectedId, input);
      if (!mounted.current) return;
      setTemplates((current) => [saved, ...current.filter((template) => template.id !== saved.id)]);
      // Back/Forward during the request opened another template, which keeps its own draft.
      if (!sameSegments(routeSegmentsRef.current, opened)) return;
      setDraft(templateInput(saved));
      setSearch("");
      setNotice({ kind: "success", message: "replyTemplates.saved" });
      if (selectedId === "new") void navigateRoute([saved.id], { replace: true });
    } catch {
      if (mounted.current) setNotice({ kind: "error", message: "replyTemplates.saveError" });
    } finally {
      if (mounted.current) setSaving(false);
    }
  }

  async function remove() {
    if (!canManage || !activeInboxId || !selected || busy || !deleteConfirmation) return;
    setDeleting(true);
    setNotice(null);
    const opened = route.segments;
    try {
      await deleteReplyTemplate(auth, activeInboxId, selected.id);
      if (!mounted.current) return;
      setTemplates(templates.filter((template) => template.id !== selected.id));
      setDeleteConfirmation(false);
      // The first remaining template opens once the deleted one leaves the URL.
      if (opened.length && sameSegments(routeSegmentsRef.current, opened)) void navigateRoute([], { replace: true });
      setNotice({ kind: "success", message: "replyTemplates.deleted" });
    } catch {
      if (mounted.current) setNotice({ kind: "error", message: "replyTemplates.deleteError" });
    } finally {
      if (mounted.current) setDeleting(false);
    }
  }

  return (
    <section className="page reply-templates-page">
      <header className="page-toolbar reply-templates-toolbar">
        <div><h1>{t("replyTemplates.title")}</h1><p>{t("replyTemplates.description")}</p></div>
        <div className="reply-templates-toolbar-actions">
          <label><span>{t("common.inbox")}</span>
            <select value={activeInboxId ?? ""} disabled={busy || inboxes.length === 0} onChange={(event) => requestNavigation({ kind: "inbox", id: event.target.value })}>
              {!activeInboxId && <option value="" disabled>{t("common.inbox")}</option>}
              {inboxes.map((inbox) => <option key={inbox.id} value={inbox.id}>{inbox.name}</option>)}
            </select>
          </label>
          {canManage && <button className="primary-button" type="button" disabled={!activeInboxId || loading || loadError || busy} onClick={() => requestNavigation({ kind: "template", id: "new" })}><Plus size={16} />{t("replyTemplates.create")}</button>}
        </div>
      </header>

      {notice && <div className={`admin-notice admin-notice--${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>{t(notice.message)}</div>}
      {!canManage && <p className="reply-templates-readonly">{t("replyTemplates.readonly")}</p>}
      {!activeInboxId ? <div className="empty-state">{t("replyTemplates.noInbox")}</div> : (
        <div className="reply-templates-layout">
          <aside className="reply-templates-list" aria-label={t("replyTemplates.list")}>
            <label className="reply-templates-search"><Search size={16} aria-hidden="true" /><input type="search" aria-label={t("replyTemplates.search")} placeholder={t("replyTemplates.searchPlaceholder")} value={search} onChange={(event) => setSearch(event.target.value)} /></label>
            {loading ? <div className="empty-state" role="status">{t("replyTemplates.loading")}</div> : loadError ? (
              <div className="empty-state reply-templates-load-error" role="alert"><p>{t("replyTemplates.loadError")}</p><button className="secondary-button" type="button" onClick={() => { setLoading(true); setLoadError(false); setLoadAttempt((current) => current + 1); }}>{t("replyTemplates.retry")}</button></div>
            ) : filtered.length === 0 ? <div className="empty-state">{t(templates.length === 0 ? "replyTemplates.empty" : "replyTemplates.noResults")}</div> : (
              <ul>{filtered.map((template) => <li key={template.id}><button className={`reply-template-row${selectedId === template.id ? " reply-template-row--active" : ""}`} type="button" aria-label={template.title} aria-describedby={`reply-template-preview-${template.id}`} aria-current={selectedId === template.id ? "true" : undefined} disabled={busy} onClick={() => requestNavigation({ kind: "template", id: template.id })}><strong>{template.title}</strong><span id={`reply-template-preview-${template.id}`}>{plainPreview(template.body)}</span></button></li>)}</ul>
            )}
          </aside>

          <div ref={editorMotion} className="reply-templates-main">
            {pendingNavigation && <div className="reply-templates-confirmation" role="alert"><p>{t("replyTemplates.discardConfirm")}</p><div><button ref={discardButton} className="secondary-button" type="button" onClick={() => navigate(pendingNavigation)}>{t("replyTemplates.discard")}</button><button className="secondary-button" type="button" onClick={() => { setPendingNavigation(null); titleInput.current?.focus(); }}>{t("replyTemplates.keepEditing")}</button></div></div>}
            {selectedId ? (
              <form className="reply-template-form" onSubmit={save} aria-busy={busy}>
                <header className="reply-template-heading"><h2>{selectedId === "new" ? t("replyTemplates.newTitle") : selected?.title}</h2>{dirty && <span role="status">{t("replyTemplates.unsaved")}</span>}</header>
                {canManage ? <>
                  <label className="reply-template-name"><span>{t("replyTemplates.name")}</span><input ref={titleInput} value={draft.title} maxLength={120} required disabled={busy} placeholder={t("replyTemplates.namePlaceholder")} onChange={(event) => setDraft((current) => ({ ...current, title: event.target.value }))} /></label>
                  {titleTooLong && <p className="message-editor-error" role="alert">{t("replyTemplates.titleTooLong")}</p>}
                  <div className="reply-template-body"><span>{t("replyTemplates.body")}</span><Suspense fallback={<div className="message-editor-loading" role="status">{t("conversations.editorLoading")}</div>}><ChatMessageEditor key={selectedId} value={draft.body} label={t("replyTemplates.body")} placeholder={t("replyTemplates.bodyPlaceholder")} disabled={busy} aiDraftSource={draft.title.trim() ? replyTemplateDraftSource(auth, activeInboxId, draft.title) : undefined} onChange={(body) => setDraft((current) => ({ ...current, body }))} /></Suspense></div>
                  <div className="reply-template-length"><span>{bodyTooLong ? t("replyTemplates.bodyTooLong") : ""}</span><span>{draft.body.length.toLocaleString()} / 10 000</span></div>
                  {bodyTooLong && <p className="visually-hidden" role="alert">{t("replyTemplates.bodyTooLong")}</p>}
                  {deleteConfirmation ? <div className="reply-templates-confirmation" role="alert"><p>{t("replyTemplates.deleteConfirm", { title: selected?.title ?? draft.title })}</p><div><DemoActionButton ref={deleteButton} className="secondary-button table-danger-link" type="button" disabled={busy} onClick={() => void remove()}>{deleting ? t("replyTemplates.deleting") : t("replyTemplates.confirmDelete")}</DemoActionButton><button className="secondary-button" type="button" disabled={busy} onClick={() => { setDeleteConfirmation(false); titleInput.current?.focus(); }}>{t("replyTemplates.cancel")}</button></div></div> : <footer className="reply-template-actions"><DemoActionButton className="primary-button" type="submit" disabled={busy || !dirty || !draft.title.trim() || !draft.body.trim() || titleTooLong || bodyTooLong || Boolean(pendingNavigation)}><Save size={16} />{saving ? t("replyTemplates.saving") : t("replyTemplates.save")}</DemoActionButton>{selected && <DemoActionButton className="secondary-button table-danger-link" type="button" disabled={busy || Boolean(pendingNavigation)} onClick={() => setDeleteConfirmation(true)}><Trash2 size={16} />{t("replyTemplates.delete")}</DemoActionButton>}</footer>}
                </> : <div className="reply-template-preview" aria-label={t("replyTemplates.preview")}><MessageBody body={draft.body} format="markdown" /></div>}
                {selected && <p className="reply-template-updated">{t("replyTemplates.updated", { date: formatDate(selected.updated_at, { dateStyle: "medium", timeStyle: "short" }) })}</p>}
              </form>
            ) : <div className="empty-state">{!loading && !loadError && t("replyTemplates.select")}</div>}
          </div>
        </div>
      )}
    </section>
  );
}
