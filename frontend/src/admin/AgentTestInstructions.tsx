import { AnimatedDetails } from "../AnimatedDetails";
import { DemoActionButton } from "./DemoReadOnly";
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { BookOpen, FileText, Save, Wrench } from "lucide-react";
import { aiTestRequest, type AiProfile, type OperatorAuth } from "../api";
import "./AgentTestInstructions.css";

type SourceTarget = { target: "agent_instructions" | "tool_instructions" | "knowledge_article"; articleId?: string };
type Source = { key: string; title: string; kind: "instructions" | "tool_instructions" | "article"; version?: number; testArticle?: boolean; chunks: { text: string; step?: number; offset?: number }[] };
type CurrentSource = { text: string; revision: string; title?: string; status?: "draft" | "published" };
export type AppliedSource = CurrentSource & { source_key: string };
type Props = {
  auth: OperatorAuth;
  profile: AiProfile;
  runId: string;
  snapshot: Record<string, unknown>;
  trace: unknown[];
  suggestedText?: string;
  initialSource?: SourceTarget;
  savedSources?: AppliedSource[];
  disabled?: boolean;
  onProfileSaved?: (profile: AiProfile, field: "instructions" | "tool_instructions") => void;
  onBusyChange?: (busy: boolean) => void;
  onDirtyChange?: (dirty: boolean) => void;
};

function object(value: unknown): Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function usedSources(snapshot: Record<string, unknown>, trace: unknown[]): Source[] {
  const profile = object(snapshot.profile);
  const testArticles = object(snapshot.scenario).knowledge_articles;
  const testArticleIds = new Set(Array.isArray(testArticles) ? testArticles.map((article) => object(article).article_id) : []);
  const sources: Source[] = [];
  for (const [key, title] of [["instructions", "Инструкции агента"], ["tool_instructions", "Инструкции инструментов"]] as const) {
    if (typeof profile[key] === "string") sources.push({ key, title, kind: key, chunks: [{ text: profile[key] }] });
  }
  for (const entry of trace) {
    const event = object(entry);
    const call = object(event.call);
    const parameters = object(call.parameters);
    const response = object(event.response);
    if (call.tool !== "read_article" || typeof parameters.article_id !== "string" || typeof response.content !== "string" || !response.content || response.article_id !== parameters.article_id) continue;
    const key = `article:${parameters.article_id}`;
    let source = sources.find((candidate) => candidate.key === key);
    if (!source) {
      source = { key, title: typeof response.title === "string" ? response.title : "Статья базы знаний", kind: "article", version: typeof response.version === "number" ? response.version : undefined, testArticle: testArticleIds.has(parameters.article_id), chunks: [] };
      sources.push(source);
    }
    const chunk = { text: response.content, step: typeof event.step === "number" ? event.step + 1 : undefined, offset: typeof parameters.offset === "number" ? parameters.offset : undefined };
    if (!source.chunks.some((existing) => existing.text === chunk.text && existing.step === chunk.step && existing.offset === chunk.offset)) source.chunks.push(chunk);
  }
  return sources;
}

function sourceKey(target?: SourceTarget) {
  return target?.target === "knowledge_article" ? `article:${target.articleId}` : target?.target === "tool_instructions" ? "tool_instructions" : "instructions";
}

function sourceError(cause: unknown, kind: Source["kind"]) {
  if (object(cause).status === 403) return kind === "article" ? "Для редактирования статьи нужны права управления базой знаний." : "Недостаточно прав для редактирования инструкций агента.";
  if (object(cause).status === 404) return "Источник больше не доступен этому агенту. Сохранённый текст запуска остаётся доступен для просмотра.";
  return null;
}

function SourceEditor({ auth, profile, runId, source, suggestedText, savedSources, disabled = false, onProfileSaved, onBusyChange, onDirtyChange }: Omit<Props, "snapshot" | "trace" | "initialSource" | "onBusyChange" | "onDirtyChange"> & { source: Source; onBusyChange: (busy: boolean) => void; onDirtyChange: (key: string, dirty: boolean) => void }) {
  const [current, setCurrent] = useState<CurrentSource | null>(null);
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [conflict, setConflict] = useState(false);
  const [comparison, setComparison] = useState(false);
  const [unavailable, setUnavailable] = useState(false);
  const [appliedSource, setAppliedSource] = useState<AppliedSource>();
  const request = useRef<AbortController | null>(null);
  const savingRequest = useRef(false);
  const busyCallback = useRef(onBusyChange);
  const dirtyCallback = useRef(onDirtyChange);
  const dirty = current !== null && draft !== current.text;
  const savedSource = savedSources?.find((value) => value.source_key === source.key);
  const path = `test-runs/${encodeURIComponent(runId)}/sources/${encodeURIComponent(source.key)}`;

  useEffect(() => { busyCallback.current = onBusyChange; }, [onBusyChange]);
  useEffect(() => { dirtyCallback.current = onDirtyChange; }, [onDirtyChange]);
  useEffect(() => { dirtyCallback.current(source.key, dirty); }, [source.key, dirty]);
  useEffect(() => () => { dirtyCallback.current(source.key, false); }, [source.key]);
  useEffect(() => () => {
    if (savingRequest.current) { savingRequest.current = false; busyCallback.current(false); }
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    request.current = controller;
    void aiTestRequest<CurrentSource>(auth, profile.id, path, { signal: controller.signal }).then((value) => {
      if (!controller.signal.aborted) { setCurrent(value); setDraft(value.text); setLoading(false); }
    }).catch((cause) => {
      if (!controller.signal.aborted) { const denied = sourceError(cause, source.kind); setUnavailable(Boolean(denied)); setError(denied ?? "Не удалось загрузить текущую версию. Попробуйте снова."); setLoading(false); }
    });
    return () => { controller.abort(); request.current?.abort(); };
  }, [auth, profile.id, path, source.kind]);

  const savedText = savedSource?.text;
  const savedRevision = savedSource?.revision;
  const savedTitle = savedSource?.title;
  const savedStatus = savedSource?.status;
  useEffect(() => {
    if (savedText === undefined || savedRevision === undefined) return;
    request.current?.abort();
    if (savingRequest.current) { savingRequest.current = false; busyCallback.current(false); }
  }, [savedText, savedRevision, savedTitle, savedStatus]);
  if (savedSource && (appliedSource?.text !== savedText || appliedSource?.revision !== savedRevision || appliedSource?.title !== savedTitle || appliedSource?.status !== savedStatus)) {
    setAppliedSource(savedSource);
    setCurrent(savedSource);
    setDraft(savedSource.text);
    setLoading(false); setSaving(false); setConflict(false); setUnavailable(false); setComparison(false);
    setError(""); setNotice("");
  }

  async function reload() {
    if (disabled || savingRequest.current) return;
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    setLoading(true); setError(""); setNotice("");
    try {
      const value = await aiTestRequest<CurrentSource>(auth, profile.id, path, { signal: controller.signal });
      if (controller.signal.aborted) return;
      setCurrent(value);
      if (current) {
        setComparison(true);
        setNotice("Ваша правка осталась в поле. Сравните её с текущей сохранённой версией перед сохранением.");
      } else setDraft(value.text);
      setConflict(false); setUnavailable(false);
    } catch (cause) {
      if (!controller.signal.aborted) { const denied = sourceError(cause, source.kind); setUnavailable(Boolean(denied)); setError(denied ?? "Не удалось загрузить текущую версию. Попробуйте снова."); }
    } finally {
      if (!controller.signal.aborted) setLoading(false);
    }
  }

  async function save() {
    if (!current || disabled || savingRequest.current || loading || draft === current.text || conflict || unavailable) return;
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    savingRequest.current = true;
    setSaving(true); busyCallback.current(true); setError(""); setNotice("");
    try {
      const value = await aiTestRequest<CurrentSource>(auth, profile.id, path, { method: "PATCH", signal: controller.signal, body: JSON.stringify({ text: draft, expected_revision: current.revision }) });
      if (controller.signal.aborted) return;
      setCurrent(value); setDraft(value.text); setComparison(false);
      setNotice(source.kind === "article" && value.status === "draft" ? "Черновик статьи сохранён. Опубликуйте статью в базе знаний, чтобы использовать её в новых запусках." : "Изменения сохранены. Повторите тест, чтобы проверить новую версию.");
      if (source.kind !== "article") onProfileSaved?.({ ...profile, [source.kind]: value.text }, source.kind);
    } catch (cause) {
      if (controller.signal.aborted) return;
      const isConflict = object(cause).status === 409;
      const denied = sourceError(cause, source.kind);
      setUnavailable(Boolean(denied));
      setConflict(isConflict);
      setError(denied ?? (isConflict ? "Источник изменился во время редактирования. Загрузите текущую версию и сравните её со своей правкой." : "Не удалось сохранить изменения. Ваша правка осталась в поле; попробуйте снова."));
    } finally {
      if (!controller.signal.aborted) { savingRequest.current = false; setSaving(false); busyCallback.current(false); }
    }
  }

  const title = current?.title || source.title;
  return <div className="agent-test-sources-editor">
    <div className="agent-test-sources-heading"><h4>{title}</h4>{source.kind === "article" && current && <span>Текущая версия {current.revision} · {current.status === "draft" ? "Черновик" : "Опубликована"}</span>}</div>
    <div className="agent-test-sources-columns">
      <section className="agent-test-sources-recorded" aria-label="Текст в запуске">
        <h5>{source.kind === "article" ? "Прочитано в запуске" : "Инструкции в запуске"}{source.version ? ` · v${source.version}` : ""}</h5>
        {source.kind === "article" && <p className="agent-test-sources-hint">Только фрагменты, которые агент получил при чтении статьи.</p>}
        {source.chunks.map((chunk, index) => <div className="agent-test-sources-chunk" key={index}>
          {source.kind === "article" && <span>{chunk.step ? `Шаг ${chunk.step}` : "Прочитанный фрагмент"}{chunk.offset !== undefined ? ` · с позиции ${chunk.offset}` : ""}</span>}
          <pre>{chunk.text || "Инструкции не заданы"}</pre>
        </div>)}
      </section>
      <section className="agent-test-sources-current" aria-label="Редактирование текущего источника">
        <h5>Текущая версия</h5>
        {loading && <p role="status">Загружаем текущую версию…</p>}
        {current && <>
          {source.kind !== "article" && current.text !== source.chunks[0]?.text && <p className="agent-test-sources-hint">Инструкции изменились после этого запуска.</p>}
          {source.kind === "article" && source.version !== undefined && current.revision !== String(source.version) && <p className="agent-test-sources-hint">Статья изменилась после этого запуска.</p>}
          {suggestedText && <AnimatedDetails className="agent-test-sources-suggestion" open><summary>Рекомендация к этому источнику</summary><pre>{suggestedText}</pre></AnimatedDetails>}
          {comparison && <AnimatedDetails open><summary>Текущая сохранённая версия</summary><pre>{current.text || "Текст не задан"}</pre></AnimatedDetails>}
          <label><span>Текст для следующих запусков</span><textarea rows={12} value={draft} maxLength={source.kind === "article" ? 200000 : 50000} disabled={disabled || saving || loading} readOnly={unavailable} onChange={(event) => { setDraft(event.target.value); setNotice(""); }} /></label>
          <div className="agent-test-sources-actions"><DemoActionButton type="button" className="primary-button" disabled={disabled || saving || loading || conflict || unavailable || draft === current.text} onClick={() => void save()}><Save size={15} />{saving ? "Сохраняем…" : "Сохранить изменения"}</DemoActionButton><button type="button" className="secondary-button" disabled={disabled || saving || loading || draft === current.text} onClick={() => { setDraft(current.text); if (!unavailable) setError(""); setNotice(""); setComparison(false); }}>Отмена</button></div>
        </>}
        {error && <p role="alert" className="form-error">{error}</p>}
        {(!current || conflict || unavailable) && !loading && <button type="button" className="secondary-button" disabled={disabled || saving} onClick={() => void reload()}>{unavailable ? "Проверить доступ снова" : conflict ? "Сравнить с текущей версией" : "Повторить загрузку"}</button>}
        {notice && <p role="status" className="agent-test-sources-notice">{notice}</p>}
      </section>
    </div>
  </div>;
}

function VisitedSourceEditor({ selected, ...props }: Parameters<typeof SourceEditor>[0] & { selected: boolean }) {
  const [visited, setVisited] = useState(selected);
  if (selected && !visited) setVisited(true);
  if (props.source.testArticle) return visited || selected ? <section className="agent-test-sources-editor" aria-label="Статья внутри теста">
    <div className="agent-test-sources-heading"><h4>{props.source.title}</h4><span>Статья внутри теста{props.source.version ? ` · v${props.source.version}` : ""}</span></div>
    <p className="agent-test-sources-hint">Статья хранится в сценарии. Чтобы изменить заголовок или ответ для следующих запусков, откройте редактор теста и раздел «Статьи внутри теста».</p>
    <section className="agent-test-sources-recorded" aria-label="Текст в запуске">
      <h5>Прочитано в запуске</h5>
      {props.source.chunks.map((chunk, index) => <div className="agent-test-sources-chunk" key={index}><span>{chunk.step ? `Шаг ${chunk.step}` : "Прочитанный фрагмент"}{chunk.offset !== undefined ? ` · с позиции ${chunk.offset}` : ""}</span><pre>{chunk.text}</pre></div>)}
    </section>
    {props.suggestedText && <AnimatedDetails className="agent-test-sources-suggestion" open><summary>Рекомендация к статье теста</summary><pre>{props.suggestedText}</pre></AnimatedDetails>}
  </section> : null;
  return visited || selected ? <SourceEditor {...props} /> : null;
}

function InstructionSources(props: Props) {
  const sources = usedSources(props.snapshot, props.trace);
  const requestedKey = sourceKey(props.initialSource);
  const requestedSourceAvailable = sources.some((source) => source.key === requestedKey);
  const defaultKey = requestedSourceAvailable ? requestedKey : props.initialSource ? undefined : sources[0]?.key;
  const selectionRequest = `${requestedKey}:${props.suggestedText ?? ""}`;
  const [selection, setSelection] = useState({ key: defaultKey, request: selectionRequest });
  const selected = selection.request === selectionRequest && sources.some((source) => source.key === selection.key) ? selection.key : defaultKey;
  const [busy, setBusy] = useState(false);
  const dirtySources = useRef(new Set<string>());
  const dirtyCallback = useRef(props.onDirtyChange);
  useEffect(() => { dirtyCallback.current = props.onDirtyChange; }, [props.onDirtyChange]);
  const reportDirty = useCallback((key: string, dirty: boolean) => {
    const previouslyDirty = dirtySources.current.size > 0;
    if (dirty) dirtySources.current.add(key);
    else dirtySources.current.delete(key);
    const currentlyDirty = dirtySources.current.size > 0;
    if (currentlyDirty !== previouslyDirty) dirtyCallback.current?.(currentlyDirty);
  }, []);
  const id = useId();
  if (sources.length === 0) return <p className="agent-test-sources-hint">В этом запуске нет сохранённого текста инструкций и прочитанных статей.</p>;
  return <section className="agent-test-sources" aria-label="Использованные инструкции">
    {props.initialSource && !requestedSourceAvailable && <p role="status" className="agent-test-sources-unavailable">Источник из рекомендации не был прочитан в этом запуске или отсутствует в его снимке. Выберите источник из списка.</p>}
    <div className="agent-test-sources-navigation" role="tablist" aria-label="Источники инструкций" aria-orientation="vertical">
      {sources.map((source, index) => {
        const Icon = source.kind === "article" ? BookOpen : source.kind === "tool_instructions" ? Wrench : FileText;
        return <button key={source.key} type="button" role="tab" id={`${id}-tab-${index}`} aria-controls={`${id}-panel-${index}`} aria-selected={selected === source.key} tabIndex={selected === source.key || (!selected && index === 0) ? 0 : -1} disabled={props.disabled || busy} onClick={() => { setSelection({ key: source.key, request: selectionRequest }); }} onKeyDown={(event) => {
          const tabs = [...(event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? [])];
          const target = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : event.key === "ArrowDown" ? (index + 1) % tabs.length : event.key === "ArrowUp" ? (index - 1 + tabs.length) % tabs.length : null;
          if (target !== null) { event.preventDefault(); tabs[target]?.focus(); tabs[target]?.click(); }
        }}><Icon size={16} /><span>{source.title}{source.kind === "article" && <small>{source.testArticle ? "Статья внутри теста" : "Статья"}{source.version ? ` · v${source.version}` : ""}</small>}</span></button>;
      })}
    </div>
    {sources.map((source, index) => <div key={`${props.profile.id}:${props.runId}:${source.key}`} id={`${id}-panel-${index}`} role="tabpanel" aria-labelledby={`${id}-tab-${index}`} hidden={selected !== source.key} className="agent-test-sources-panel">
      <VisitedSourceEditor {...props} selected={selected === source.key} source={source} suggestedText={source.key === requestedKey ? props.suggestedText : undefined} onBusyChange={(value) => { setBusy(value); props.onBusyChange?.(value); }} onDirtyChange={reportDirty} />
    </div>)}
  </section>;
}

export function AgentTestInstructions(props: Props) {
  const [scope, setScope] = useState({ auth: props.auth, revision: 0 });
  if (scope.auth !== props.auth) setScope({ auth: props.auth, revision: scope.revision + 1 });
  return <InstructionSources key={`${props.profile.id}:${props.runId}:${scope.revision}`} {...props} />;
}
