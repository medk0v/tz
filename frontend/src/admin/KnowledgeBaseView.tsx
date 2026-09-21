import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { visibilityEqual } from "../resource-visibility";
import { useContentMotion } from "./useContentMotion";
import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { BookOpen, ChevronDown, FilePlus2, Plus, Save, Search, Settings2, Trash2 } from "lucide-react";
import {
  createKnowledgeArticle,
  createKnowledgeBase,
  deleteKnowledgeArticle,
  deleteKnowledgeBase,
  listKnowledgeArticles,
  listKnowledgeBases,
  updateKnowledgeArticle,
  updateKnowledgeBase,
  type KnowledgeArticle,
  type KnowledgeArticleInput,
  type KnowledgeArticleStatus,
  type KnowledgeBase,
  type KnowledgeBaseInput,
  type OperatorAuth,
} from "../api";
import { useI18n, type MessageKey } from "../i18n";
import { AiReplySuggestionPicker } from "./AiReplySuggestionPicker";
import { knowledgeArticleDraftSource } from "./ai-draft-sources";
import { sameSegments, useCanonicalPageRoute, usePageRoute } from "./page-route";
import "./KnowledgeBaseView.css";

interface Props {
  auth: OperatorAuth;
  projectId: string;
  onDirtyChange?: (dirty: boolean) => void;
}

interface Notice {
  kind: "success" | "error";
  message: MessageKey;
}

type ArticleFilter = "all" | KnowledgeArticleStatus;

const emptyBase: KnowledgeBaseInput = { name: "", description: "", status: "active" };
const emptyArticle: KnowledgeArticleInput = { title: "", body: "", status: "draft", source_url: null };
const maxArticleBodyLength = 200_000;

function baseInput(base: KnowledgeBase): KnowledgeBaseInput {
  return { visibility: base.visibility, name: base.name, description: base.description, status: base.status };
}

function articleInput(article: KnowledgeArticle): KnowledgeArticleInput {
  return { title: article.title, body: article.body, status: article.status, source_url: article.source_url };
}

function baseInputsEqual(left: KnowledgeBaseInput, right: KnowledgeBaseInput) {
  return left.name === right.name && left.description === right.description && left.status === right.status && visibilityEqual(left.visibility, right.visibility);
}

function articleInputsEqual(left: KnowledgeArticleInput, right: KnowledgeArticleInput) {
  return left.title === right.title && left.body === right.body && left.status === right.status && left.source_url === right.source_url;
}

/**
 * Knowledge base URLs: `/knowledge-base`, `/knowledge-base/new` and `/knowledge-base/<base>[/<material>|/new]`.
 * Without a base the first one opens, and without a material the base's first material.
 */
function knowledgeRoute(segments: readonly string[]): { baseId: string | null; articleId: string | null } {
  const [baseId = null, articleId = null] = segments;
  return { baseId, articleId: baseId === "new" ? null : articleId };
}

function knowledgeSegments(baseId: string | null, articleId: string | null = null): string[] {
  if (!baseId) return [];
  return articleId ? [baseId, articleId] : [baseId];
}

function KnowledgeDeleteAction({ label, message, busy, onConfirm }: {
  label: string;
  message: string;
  busy: boolean;
  onConfirm: () => Promise<void>;
}) {
  const { t } = useI18n();
  const [confirming, setConfirming] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);

  useEffect(() => {
    if (!confirming && restoreFocus.current) {
      trigger.current?.focus();
      restoreFocus.current = false;
    }
  }, [confirming]);

  function cancel() {
    if (busy) return;
    restoreFocus.current = true;
    setConfirming(false);
  }

  if (!confirming) return <DemoActionButton ref={trigger} className="secondary-button table-danger-link" type="button" disabled={busy} onClick={() => setConfirming(true)}><Trash2 size={16} aria-hidden="true" />{label}</DemoActionButton>;

  return <div className="knowledge-delete-confirmation" role="alert" onKeyDown={(event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    }
  }}>
    <p>{message}</p>
    <div>
      <DemoActionButton className="secondary-button table-danger-link" type="button" disabled={busy} onClick={() => void onConfirm()}><Trash2 size={16} aria-hidden="true" />{label}</DemoActionButton>
      <button autoFocus className="secondary-button" type="button" disabled={busy} onClick={cancel}>{t("knowledge.cancel")}</button>
    </div>
  </div>;
}

export function KnowledgeBaseView({ auth, projectId, onDirtyChange }: Props) {
  return <ProjectKnowledgeBaseView key={projectId} auth={auth} projectId={projectId} onDirtyChange={onDirtyChange} />;
}

function ProjectKnowledgeBaseView({ auth, onDirtyChange }: Props) {
  const { t, formatDate } = useI18n();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const routeSegmentsRef = useRef(route.segments);
  const routed = knowledgeRoute(route.segments);
  const [bases, setBases] = useState<KnowledgeBase[]>([]);
  const selectedBaseId: string | "new" | null = routed.baseId === "new" ? "new" : bases.find((base) => base.id === routed.baseId)?.id ?? bases[0]?.id ?? null;
  const [baseDraft, setBaseDraft] = useState<KnowledgeBaseInput>(emptyBase);
  const [baseReturnId, setBaseReturnId] = useState<string | null>(null);
  const [baseSettingsOpen, setBaseSettingsOpen] = useState(false);
  const [articles, setArticles] = useState<KnowledgeArticle[]>([]);
  const [loadingArticles, setLoadingArticles] = useState(false);
  // A material in the URL belongs to the base the URL names.
  const routedArticleId = routed.baseId === selectedBaseId ? routed.articleId : null;
  const selectedArticleId: string | "new" | null = !selectedBaseId || selectedBaseId === "new" ? null : routedArticleId === "new" ? "new" : loadingArticles ? null : articles.find((article) => article.id === routedArticleId)?.id ?? articles[0]?.id ?? null;
  const [articleDraft, setArticleDraft] = useState<KnowledgeArticleInput>(emptyArticle);
  const [articleQuery, setArticleQuery] = useState("");
  const [articleFilter, setArticleFilter] = useState<ArticleFilter>("all");
  const editorMotion = useContentMotion<HTMLDivElement>(`${selectedBaseId}:${selectedArticleId}`);
  const articleBodyId = useId();
  const articleBodyRef = useRef<HTMLTextAreaElement>(null);
  const [loading, setLoading] = useState(true);
  const [basesFailed, setBasesFailed] = useState(false);
  const [articlesFailed, setArticlesFailed] = useState(false);
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);

  // Links and Back/Forward open bases and materials through the URL; the drafts follow what is shown.
  const articleKey = selectedArticleId && `${selectedBaseId}/${selectedArticleId}`;
  const [shownBaseId, setShownBaseId] = useState(selectedBaseId);
  const [shownArticleKey, setShownArticleKey] = useState(articleKey);
  if (selectedBaseId !== shownBaseId) {
    setShownBaseId(selectedBaseId);
    const base = bases.find((item) => item.id === selectedBaseId);
    setBaseDraft(base ? baseInput(base) : emptyBase);
    setBaseSettingsOpen(false);
    setArticles([]);
    setLoadingArticles(Boolean(base));
    setArticlesFailed(false);
    setArticleQuery("");
    setArticleFilter("all");
  } else if (articleKey !== shownArticleKey) {
    setShownArticleKey(articleKey);
    const article = articles.find((item) => item.id === selectedArticleId);
    setArticleDraft(article ? articleInput(article) : emptyArticle);
  }

  const selectedBase = useMemo(() => bases.find((base) => base.id === selectedBaseId) ?? null, [bases, selectedBaseId]);
  const selectedArticle = useMemo(() => articles.find((article) => article.id === selectedArticleId) ?? null, [articles, selectedArticleId]);
  const baseDirty = useMemo(() => {
    if (selectedBaseId === "new") return !baseInputsEqual(baseDraft, emptyBase);
    return selectedBase ? !baseInputsEqual(baseDraft, baseInput(selectedBase)) : false;
  }, [baseDraft, selectedBase, selectedBaseId]);
  const articleDirty = useMemo(() => {
    if (selectedArticleId === "new") return !articleInputsEqual(articleDraft, emptyArticle);
    return selectedArticle ? !articleInputsEqual(articleDraft, articleInput(selectedArticle)) : false;
  }, [articleDraft, selectedArticle, selectedArticleId]);
  const filteredArticles = useMemo(() => {
    const query = articleQuery.trim().toLocaleLowerCase();
    return articles.filter((article) => {
      if (articleFilter !== "all" && article.status !== articleFilter) return false;
      if (!query) return true;
      return [article.title, article.body, article.source_url ?? ""].some((value) => value.toLocaleLowerCase().includes(query));
    });
  }, [articleFilter, articleQuery, articles]);

  useEffect(() => { onDirtyChange?.(baseDirty || articleDirty); }, [articleDirty, baseDirty, onDirtyChange]);
  useEffect(() => () => { onDirtyChange?.(false); }, [onDirtyChange]);
  useEffect(() => {
    if (!baseDirty && !articleDirty) return undefined;
    const preventUnload = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    window.addEventListener("beforeunload", preventUnload);
    return () => window.removeEventListener("beforeunload", preventUnload);
  }, [articleDirty, baseDirty]);

  async function reloadBases() {
    setBases(await listKnowledgeBases(auth));
  }

  useEffect(() => {
    let active = true;
    listKnowledgeBases(auth)
      .then((items) => { if (active) setBases(items); })
      .catch(() => { if (active) { setBasesFailed(true); setNotice({ kind: "error", message: "knowledge.loadError" }); } })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [auth]);

  useEffect(() => {
    if (!selectedBaseId || selectedBaseId === "new") return undefined;
    let active = true;
    listKnowledgeArticles(auth, selectedBaseId)
      .then((items) => { if (active) setArticles(items); })
      .catch(() => { if (active) { setArticlesFailed(true); setNotice({ kind: "error", message: "knowledge.articleLoadError" }); } })
      .finally(() => { if (active) setLoadingArticles(false); });
    return () => { active = false; };
  }, [auth, selectedBaseId]);

  // The URL keeps a base or material only while it names what is shown; loading failures keep the link for a retry.
  const baseSegment = selectedBaseId === "new" || (selectedBaseId !== null && selectedBaseId === routed.baseId) ? selectedBaseId : null;
  const articleSegment = baseSegment && baseSegment !== "new" && (selectedArticleId === "new" || (selectedArticleId !== null && selectedArticleId === routedArticleId)) ? selectedArticleId : null;
  useEffect(() => { routeSegmentsRef.current = route.segments; }, [route.segments]);
  useCanonicalPageRoute(route, knowledgeSegments(baseSegment, articleSegment), !loading && !basesFailed && (!selectedBase || (!loadingArticles && !articlesFailed)));

  function confirmDiscardChanges() {
    return (!baseDirty && !articleDirty) || window.confirm(t("knowledge.discardChangesConfirm"));
  }

  function selectBase(base: KnowledgeBase) {
    if (base.id === selectedBaseId || !confirmDiscardChanges()) return;
    setNotice(null);
    void navigateRoute(knowledgeSegments(base.id));
  }

  function startBase() {
    if (!confirmDiscardChanges()) return;
    setNotice(null);
    if (selectedBaseId === "new") {
      setBaseDraft(emptyBase);
      return;
    }
    // Canceling returns to the base the URL named, or else to the first base.
    setBaseReturnId(routed.baseId);
    void navigateRoute(["new"]);
  }

  function cancelBaseCreation() {
    if (baseDirty && !window.confirm(t("knowledge.discardChangesConfirm"))) return;
    const base = bases.find((item) => item.id === baseReturnId);
    setBaseReturnId(null);
    setNotice(null);
    void navigateRoute(knowledgeSegments(base?.id ?? null));
  }

  async function saveBase(event: React.FormEvent) {
    event.preventDefault();
    if (!baseDraft.name.trim() || saving || selectedBaseId === null) return;
    setSaving(true);
    setNotice(null);
    const opened = route.segments;
    try {
      const payload = { ...baseDraft, name: baseDraft.name.trim() };
      const saved = selectedBaseId === "new" ? await createKnowledgeBase(auth, payload) : await updateKnowledgeBase(auth, selectedBaseId, payload);
      await reloadBases();
      // Back/Forward during the request opened another base, which keeps its own draft.
      if (!sameSegments(routeSegmentsRef.current, opened)) return;
      setBaseDraft(baseInput(saved));
      setBaseReturnId(null);
      setBaseSettingsOpen(false);
      if (selectedBaseId === "new") void navigateRoute(knowledgeSegments(saved.id), { replace: true });
      setNotice({ kind: "success", message: "knowledge.saved" });
    } catch {
      setNotice({ kind: "error", message: "knowledge.saveError" });
    } finally {
      setSaving(false);
    }
  }

  async function removeBase() {
    if (!selectedBase || saving) return;
    setSaving(true);
    setNotice(null);
    const opened = route.segments;
    try {
      await deleteKnowledgeBase(auth, selectedBase.id);
      setBaseSettingsOpen(false);
      await reloadBases();
      // The first remaining base opens once the deleted one leaves the URL.
      if (opened.length && sameSegments(routeSegmentsRef.current, opened)) void navigateRoute([], { replace: true });
      setNotice({ kind: "success", message: "knowledge.deleted" });
    } catch {
      setNotice({ kind: "error", message: "knowledge.deleteError" });
    } finally {
      setSaving(false);
    }
  }

  function startArticle() {
    if (!selectedBase || !confirmDiscardChanges()) return;
    setNotice(null);
    if (selectedArticleId !== "new") {
      void navigateRoute(knowledgeSegments(selectedBase.id, "new"));
      return;
    }
    setArticleDraft(emptyArticle);
  }

  function selectArticle(article: KnowledgeArticle) {
    if (!selectedBase || article.id === selectedArticleId || !confirmDiscardChanges()) return;
    setNotice(null);
    void navigateRoute(knowledgeSegments(selectedBase.id, article.id));
  }

  async function saveArticle(event: React.FormEvent) {
    event.preventDefault();
    if (!selectedBase || !selectedArticleId || !articleDraft.title.trim() || saving) return;
    setSaving(true);
    setNotice(null);
    const opened = route.segments;
    try {
      const payload = { ...articleDraft, title: articleDraft.title.trim(), source_url: articleDraft.source_url?.trim() || null };
      const saved = selectedArticleId === "new"
        ? await createKnowledgeArticle(auth, selectedBase.id, payload)
        : await updateKnowledgeArticle(auth, selectedBase.id, selectedArticleId, payload);
      // Back/Forward during the request opened another material, which keeps its own draft.
      if (sameSegments(routeSegmentsRef.current, opened)) {
        setArticles((current) => [saved, ...current.filter((article) => article.id !== saved.id)]);
        setArticleDraft(articleInput(saved));
        if (selectedArticleId === "new") void navigateRoute(knowledgeSegments(selectedBase.id, saved.id), { replace: true });
      }
      await reloadBases();
      setNotice({ kind: "success", message: "knowledge.articleSaved" });
    } catch {
      setNotice({ kind: "error", message: "knowledge.articleSaveError" });
    } finally {
      setSaving(false);
    }
  }

  async function removeArticle() {
    if (!selectedBase || !selectedArticle || saving) return;
    setSaving(true);
    setNotice(null);
    const opened = route.segments;
    try {
      await deleteKnowledgeArticle(auth, selectedBase.id, selectedArticle.id);
      setArticles((current) => current.filter((article) => article.id !== selectedArticle.id));
      // The first remaining material opens once the deleted one leaves the URL.
      if (routed.articleId && sameSegments(routeSegmentsRef.current, opened)) void navigateRoute(knowledgeSegments(selectedBase.id), { replace: true });
      await reloadBases();
      setNotice({ kind: "success", message: "knowledge.articleDeleted" });
    } catch {
      setNotice({ kind: "error", message: "knowledge.articleDeleteError" });
    } finally {
      setSaving(false);
    }
  }

  /** Appends generated Markdown after the current content, as the chat editor does. */
  function insertGeneratedBody(body: string): boolean {
    const current = articleDraft.body.trimEnd();
    const next = current ? `${current}\n\n${body}` : body;
    if (next.length > maxArticleBodyLength) return false;
    setArticleDraft((draft) => ({ ...draft, body: next }));
    requestAnimationFrame(() => {
      const textarea = articleBodyRef.current;
      if (!textarea) return;
      textarea.focus();
      textarea.setSelectionRange(textarea.value.length, textarea.value.length);
      textarea.scrollTop = textarea.scrollHeight;
    });
    return true;
  }

  function clearArticleFilters() {
    setArticleQuery("");
    setArticleFilter("all");
  }

  return (
    <section className="page knowledge-page">
      <header className="page-toolbar knowledge-page-toolbar">
        <div><h1>{t("knowledge.title")}</h1><p>{t("knowledge.description")}</p></div>
        <div className="knowledge-page-actions">
          <button className={selectedBase ? "secondary-button" : "primary-button"} type="button" onClick={startBase}><Plus size={16} aria-hidden="true" />{t("knowledge.newBase")}</button>
          {selectedBase && <button className="primary-button" type="button" onClick={startArticle}><FilePlus2 size={16} aria-hidden="true" />{t("knowledge.newArticle")}</button>}
        </div>
      </header>

      {notice && <div className={`admin-notice admin-notice--${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>{t(notice.message)}</div>}

      {loading ? <div className="settings-panel empty-state" role="status">{t("knowledge.loading")}</div> : selectedBaseId === "new" ? (
        <form className="settings-panel knowledge-base-settings" onSubmit={saveBase}>
          <header><h2>{t("knowledge.newBaseTitle")}</h2><span>{t("knowledge.statusNew")}</span></header>
          <div className="settings-form knowledge-base-fields">
            <label><span>{t("knowledge.name")}</span><input autoFocus value={baseDraft.name} maxLength={200} onChange={(event) => setBaseDraft((current) => ({ ...current, name: event.target.value }))} /></label>
            <label><span>{t("knowledge.status")}</span><select value={baseDraft.status} onChange={(event) => setBaseDraft((current) => ({ ...current, status: event.target.value as KnowledgeBaseInput["status"] }))}><option value="active">{t("knowledge.statusActive")}</option><option value="archived">{t("knowledge.statusArchived")}</option></select></label>
            <label className="knowledge-description-field"><span>{t("knowledge.baseDescription")}</span><textarea value={baseDraft.description} maxLength={2000} rows={3} onChange={(event) => setBaseDraft((current) => ({ ...current, description: event.target.value }))} /></label>
            <div className="knowledge-actions"><DemoActionButton className="primary-button" type="submit" disabled={saving || !baseDraft.name.trim()}><Save size={16} aria-hidden="true" />{saving ? t("knowledge.saving") : t("knowledge.saveBase")}</DemoActionButton><button className="secondary-button" type="button" disabled={saving} onClick={cancelBaseCreation}>{t("knowledge.cancel")}</button></div>
          </div>
        </form>
      ) : selectedBase ? (
        <>
          <section className="knowledge-base-bar" aria-label={t("knowledge.baseList")}>
            <div className="knowledge-base-picker">
              <BookOpen size={18} aria-hidden="true" />
              <label><span className="visually-hidden">{t("knowledge.baseList")}</span><select value={selectedBase.id} onChange={(event) => { const base = bases.find((item) => item.id === event.target.value); if (base) selectBase(base); }}>{bases.map((base) => <option key={base.id} value={base.id}>{base.name}</option>)}</select></label>
              <span>{t("knowledge.baseSummary", { published: selectedBase.published_article_count, total: selectedBase.article_count })}</span>
            </div>
            <button className="secondary-button knowledge-settings-toggle" type="button" aria-expanded={baseSettingsOpen} aria-controls={`${articleBodyId}-knowledge-base-settings`} onClick={() => setBaseSettingsOpen((open) => !open)}><Settings2 size={16} aria-hidden="true" />{t("knowledge.baseSettings")}<ChevronDown size={15} aria-hidden="true" /></button>
          </section>

          <AnimatedDisclosure open={baseSettingsOpen}><form id={`${articleBodyId}-knowledge-base-settings`} className="settings-panel knowledge-base-settings" onSubmit={saveBase}>
            <header><h2>{t("knowledge.baseSettings")}</h2><span>{t(baseDraft.status === "active" ? "knowledge.statusActive" : "knowledge.statusArchived")}</span></header>
            <div className="settings-form knowledge-base-fields">
              <label><span>{t("knowledge.name")}</span><input value={baseDraft.name} maxLength={200} onChange={(event) => setBaseDraft((current) => ({ ...current, name: event.target.value }))} /></label>
              <label><span>{t("knowledge.status")}</span><select value={baseDraft.status} onChange={(event) => setBaseDraft((current) => ({ ...current, status: event.target.value as KnowledgeBaseInput["status"] }))}><option value="active">{t("knowledge.statusActive")}</option><option value="archived">{t("knowledge.statusArchived")}</option></select></label>
              <label className="knowledge-description-field"><span>{t("knowledge.baseDescription")}</span><textarea value={baseDraft.description} maxLength={2000} rows={3} onChange={(event) => setBaseDraft((current) => ({ ...current, description: event.target.value }))} /></label>
              <div className="knowledge-actions knowledge-base-actions"><DemoActionButton className="primary-button" type="submit" disabled={saving || !baseDirty || !baseDraft.name.trim()}><Save size={16} aria-hidden="true" />{saving ? t("knowledge.saving") : t("knowledge.saveBase")}</DemoActionButton><KnowledgeDeleteAction key={selectedBase.id} label={t("knowledge.deleteBase")} message={t("knowledge.deleteConfirm", { name: selectedBase.name })} busy={saving} onConfirm={removeBase} /></div>
            </div>
          </form></AnimatedDisclosure>

          <section className="knowledge-workspace">
            <aside className="knowledge-library" aria-label={t("knowledge.articleList")}>
              <header className="knowledge-library-header">
                <div><div><h2>{t("knowledge.materials")}</h2><p>{t("knowledge.materialsDescription")}</p></div><span>{t("knowledge.materialCount", { visible: filteredArticles.length, total: articles.length })}</span></div>
                <label className="knowledge-search"><Search size={16} aria-hidden="true" /><input type="search" aria-label={t("knowledge.searchArticles")} placeholder={t("knowledge.searchArticlesPlaceholder")} value={articleQuery} onChange={(event) => setArticleQuery(event.target.value)} /></label>
                <div className="knowledge-filters" role="group" aria-label={t("knowledge.filterArticles")}>
                  {(["all", "published", "draft"] as const).map((filter) => <button key={filter} type="button" aria-pressed={articleFilter === filter} onClick={() => setArticleFilter(filter)}>{t(filter === "all" ? "knowledge.filterAll" : filter === "published" ? "knowledge.filterPublished" : "knowledge.filterDrafts")}</button>)}
                </div>
              </header>

              <div className="knowledge-article-list">
                {loadingArticles ? <div className="empty-state" role="status">{t("knowledge.articleLoading")}</div> : filteredArticles.length > 0 ? (
                  <ul>{filteredArticles.map((article) => <li key={article.id}><button className={`knowledge-article-row ${selectedArticleId === article.id ? "knowledge-article-row--active" : ""}`} type="button" aria-current={selectedArticleId === article.id ? "true" : undefined} title={article.title} onClick={() => selectArticle(article)}><strong>{article.title}</strong><code className="knowledge-article-id">ID: {article.id}</code><span className="knowledge-article-meta"><span className={`knowledge-article-status knowledge-article-status--${article.status}`}>{t(article.status === "published" ? "knowledge.articlePublished" : "knowledge.articleDraft")}</span><time dateTime={article.updated_at}>{formatDate(article.updated_at, { dateStyle: "medium" })}</time></span></button></li>)}</ul>
                ) : <div className="empty-state knowledge-library-empty">
                  <BookOpen size={22} aria-hidden="true" /><p>{t(articles.length === 0 ? "knowledge.articleEmpty" : "knowledge.noArticleResults")}</p>
                  {articles.length > 0 && <button className="secondary-button" type="button" onClick={clearArticleFilters}>{t("knowledge.clearArticleFilters")}</button>}
                </div>}
              </div>
            </aside>

            <div ref={editorMotion} className="knowledge-article-editor">
              {selectedArticleId ? (
                <form className="knowledge-article-form" onSubmit={saveArticle} aria-busy={saving}>
                  <header className="knowledge-editor-header">
                    <div><h2>{selectedArticleId === "new" ? t("knowledge.newArticleTitle") : selectedArticle?.title}</h2>{selectedArticle && <code className="knowledge-article-id">ID: {selectedArticle.id}</code>}<span role={articleDirty ? "status" : undefined}>{articleDirty ? t("knowledge.unsavedChanges") : selectedArticle ? t("knowledge.version", { version: selectedArticle.version }) : t("knowledge.articleDraft")}</span></div>
                  </header>

                  <div className="knowledge-editor-panel">
                    <div className="knowledge-article-fields">
                      <label className="knowledge-title-field"><span>{t("knowledge.articleTitle")}</span><input autoFocus={selectedArticleId === "new"} value={articleDraft.title} maxLength={300} onChange={(event) => setArticleDraft((current) => ({ ...current, title: event.target.value }))} /></label>
                      <label><span>{t("knowledge.articleStatus")}</span><select value={articleDraft.status} onChange={(event) => setArticleDraft((current) => ({ ...current, status: event.target.value as KnowledgeArticleInput["status"] }))}><option value="draft">{t("knowledge.articleDraft")}</option><option value="published">{t("knowledge.articlePublished")}</option></select></label>
                      <label className="knowledge-source-field"><span>{t("knowledge.sourceUrl")}</span><input type="url" value={articleDraft.source_url ?? ""} maxLength={2000} placeholder="https://docs.example/…" onChange={(event) => setArticleDraft((current) => ({ ...current, source_url: event.target.value || null }))} /></label>
                      <div className="knowledge-body-field">
                        <div className="knowledge-body-label">
                          <label htmlFor={articleBodyId}>{t("knowledge.articleBody")}</label>
                          {articleDraft.title.trim() && <AiReplySuggestionPicker key={articleKey} source={knowledgeArticleDraftSource(auth, selectedBase.id, articleDraft.title)} draftBody={articleDraft.body} disabled={saving} triggerClassName="secondary-button" onInsert={insertGeneratedBody} />}
                        </div>
                        <textarea ref={articleBodyRef} id={articleBodyId} value={articleDraft.body} maxLength={maxArticleBodyLength} rows={18} placeholder={t("knowledge.articleBodyPlaceholder")} aria-describedby={`${articleBodyId}-help`} onChange={(event) => setArticleDraft((current) => ({ ...current, body: event.target.value }))} />
                        <small id={`${articleBodyId}-help`}>{t("knowledge.markdownHelp")}</small>
                      </div>
                    </div>
                  </div>

                  <footer className="knowledge-editor-actions"><DemoActionButton className="primary-button" type="submit" disabled={saving || !articleDirty || !articleDraft.title.trim()}><Save size={16} aria-hidden="true" />{saving ? t("knowledge.saving") : t("knowledge.saveArticle")}</DemoActionButton>{selectedArticle && <KnowledgeDeleteAction key={selectedArticle.id} label={t("knowledge.deleteArticle")} message={t("knowledge.articleDeleteConfirm", { title: selectedArticle.title })} busy={saving} onConfirm={removeArticle} />}</footer>
                </form>
              ) : <div className="empty-state knowledge-editor-empty"><BookOpen size={28} aria-hidden="true" /><h2>{t("knowledge.selectArticle")}</h2><p>{t("knowledge.selectArticleDescription")}</p><button className="primary-button" type="button" onClick={startArticle}><FilePlus2 size={16} aria-hidden="true" />{t("knowledge.newArticle")}</button></div>}
            </div>
          </section>
        </>
      ) : <div className="settings-panel empty-state knowledge-base-empty"><BookOpen size={28} aria-hidden="true" /><h2>{t("knowledge.empty")}</h2><button className="primary-button" type="button" onClick={startBase}><Plus size={16} aria-hidden="true" />{t("knowledge.newBase")}</button></div>}
    </section>
  );
}
