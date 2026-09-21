import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { BookOpen, Check, Lightbulb, RefreshCw, X } from "lucide-react";
import { aiTestRequest, type OperatorAuth } from "../api";
import type { AppliedSource } from "./AgentTestInstructions";

export type Recommendation = {
  target: "agent_instructions" | "knowledge_article" | "scenario" | "runtime";
  reason: string;
  suggested_text: string;
  step?: number;
  article_id?: string;
  article_version?: number;
  article_title?: string;
  change?: { source_key: string; expected_revision: string; original_text: string };
};
type Recommendations = { summary: string; recommendations: Recommendation[]; stale: boolean };
type Analysis =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ready"; result: Recommendations; application?: "saving" | "saved" | "conflict" | "error"; message?: string };
const targets: Record<Recommendation["target"], string> = {
  agent_instructions: "Инструкции агента",
  knowledge_article: "Статья базы знаний",
  scenario: "Сценарий теста",
  runtime: "Выполнение запроса",
};

export function AgentTestRecommendations({ auth, profileId, runId, stale, disabled, onEditInstructions, onOpenInstructions, onEditSource, instructions, testArticleIds = [], hasUnsavedChanges = false, onApplied, onBusyChange }: {
  auth: OperatorAuth;
  profileId: string;
  runId: string;
  stale: boolean;
  disabled: boolean;
  onEditInstructions?: (suggestedText?: string) => void;
  onOpenInstructions?: () => void;
  onEditSource?: (recommendation: Recommendation) => void;
  instructions?: ReactNode;
  testArticleIds?: string[];
  hasUnsavedChanges?: boolean;
  onApplied?: (sources: AppliedSource[]) => void;
  onBusyChange?: (busy: boolean) => void;
}) {
  const [state, setState] = useState<{ auth: OperatorAuth; profileId: string; runId: string; analysis: Analysis } | null>(null);
  const analysis = state?.auth === auth && state.profileId === profileId && state.runId === runId ? state.analysis : null;
  const [workspaceHidden, setWorkspaceHidden] = useState(false);
  const workspaceId = useId();
  const instructionsButton = useRef<HTMLButtonElement>(null);
  const recommendationsButton = useRef<HTMLButtonElement>(null);
  const request = useRef<AbortController | null>(null);
  const applying = useRef(false);
  const busyCallback = useRef(onBusyChange);
  useEffect(() => { busyCallback.current = onBusyChange; }, [onBusyChange]);
  useEffect(() => () => {
    request.current?.abort();
    if (applying.current) { applying.current = false; busyCallback.current?.(false); }
  }, [auth, profileId, runId]);
  const saving = analysis?.status === "ready" && analysis.application === "saving";
  const applied = analysis?.status === "ready" && analysis.application === "saved";
  const changes = analysis?.status === "ready" ? analysis.result.recommendations.flatMap((item) => {
    if (!item.change || (item.target !== "agent_instructions" && item.target !== "knowledge_article") || testArticleIds.includes(item.article_id ?? "")) return [];
    return [{ ...item.change, suggested_text: item.suggested_text }];
  }) : [];

  function openInstructions() {
    setWorkspaceHidden(false);
    onOpenInstructions?.();
  }

  function closeWorkspace() {
    setWorkspaceHidden(true);
    (instructionsButton.current ?? recommendationsButton.current)?.focus();
  }

  async function analyze() {
    if (applying.current) return;
    openInstructions();
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    const setAnalysis = (analysis: Analysis) => setState({ auth, profileId, runId, analysis });
    setAnalysis({ status: "loading" });
    try {
      const result = await aiTestRequest<Recommendations>(auth, profileId, `test-runs/${encodeURIComponent(runId)}/recommendations`, { method: "POST", signal: controller.signal });
      if (!controller.signal.aborted) setAnalysis({ status: "ready", result });
    } catch {
      if (!controller.signal.aborted) setAnalysis({ status: "error", message: "Не удалось получить рекомендации. Попробуйте снова." });
    }
  }

  async function apply() {
    if (analysis?.status !== "ready" || applying.current || disabled || hasUnsavedChanges || !changes.length || applied || analysis.application === "conflict") return;
    request.current?.abort();
    const controller = new AbortController();
    request.current = controller;
    const setAnalysis = (next: Analysis) => setState({ auth, profileId, runId, analysis: next });
    applying.current = true;
    busyCallback.current?.(true);
    setAnalysis({ ...analysis, application: "saving", message: undefined });
    try {
      const result = await aiTestRequest<{ sources: AppliedSource[] }>(auth, profileId, `test-runs/${encodeURIComponent(runId)}/recommendations/apply`, { method: "POST", signal: controller.signal, body: JSON.stringify({ changes }) });
      if (controller.signal.aborted) return;
      setAnalysis({ ...analysis, application: "saved", message: result.sources.some((source) => source.status === "draft")
        ? "Рекомендации применены. Изменённые черновики статей нужно опубликовать в базе знаний. Затем повторите тест."
        : "Рекомендации применены. Повторите тест, чтобы проверить результат." });
      onApplied?.(result.sources);
    } catch (cause) {
      if (controller.signal.aborted) return;
      const status = cause !== null && typeof cause === "object" && "status" in cause ? cause.status : undefined;
      setAnalysis({ ...analysis, application: status === 409 ? "conflict" : "error", message: status === 409
        ? "Источник изменился после подготовки рекомендаций. Изменения не применены. Обновите рекомендации и проверьте новые замены."
        : status === 403 ? "Недостаточно прав для изменения инструкций или статей. Изменения не применены."
        : status === 404 ? "Один из источников больше не доступен агенту. Изменения не применены. Обновите рекомендации."
        : "Не удалось подтвердить применение рекомендаций. Обновите рекомендации, чтобы проверить текущий текст перед повторной попыткой." });
    } finally {
      if (!controller.signal.aborted) { applying.current = false; busyCallback.current?.(false); }
    }
  }

  return <div className="agent-test-recommendations">
    <div className="agent-test-toolbar">
      <DemoActionButton ref={recommendationsButton} type="button" className="secondary-button" aria-controls={workspaceId} aria-expanded={Boolean(analysis || instructions) && !workspaceHidden} disabled={disabled || saving || (!workspaceHidden && analysis?.status === "loading")} onClick={() => workspaceHidden && analysis ? setWorkspaceHidden(false) : void analyze()}>{analysis && !workspaceHidden ? <RefreshCw size={15} /> : <Lightbulb size={15} />}{workspaceHidden && analysis ? "Показать рекомендации" : analysis?.status === "loading" ? "Готовим рекомендации…" : analysis ? "Обновить рекомендации" : "Рекомендации по инструкциям"}</DemoActionButton>
      {onOpenInstructions ? <button ref={instructionsButton} type="button" className="secondary-button" aria-controls={workspaceId} aria-expanded={Boolean(instructions) && !workspaceHidden} disabled={disabled} onClick={openInstructions}><BookOpen size={15} />Использованные инструкции</button>
        : onEditInstructions && <button type="button" className="secondary-button" disabled={disabled} onClick={() => onEditInstructions()}>Редактировать инструкции</button>}
    </div>
    {(analysis || instructions) && <section id={workspaceId} className="agent-test-instructions-workspace" aria-label="Инструкции и рекомендации" hidden={workspaceHidden}>
      <div className="agent-test-instructions-workspace-heading"><h4>Инструкции и рекомендации</h4><button type="button" className="icon-button" aria-label="Закрыть инструкции и рекомендации" title="Закрыть инструкции и рекомендации" onClick={closeWorkspace}><X size={18} aria-hidden="true" /></button></div>
      <div className={`agent-test-instructions-layout${instructions ? " agent-test-instructions-layout--with-sources" : ""}`}>
      {instructions && <div className="agent-test-instructions-editor">{instructions}</div>}
      <aside className="agent-test-advice" aria-label="Рекомендации">
      <h4>Рекомендации</h4>
      {!analysis && <p>Получите рекомендации по ответу агента и использованным инструкциям этого запуска.</p>}
    {analysis?.status === "loading" && <p role="status">Анализируем ответ, действия и настройки этого запуска.</p>}
    {analysis?.status === "error" && <p role="alert" className="form-error">{analysis.message}</p>}
    {analysis?.status === "ready" && <>
      <p role="status">{analysis.result.summary}</p>
      {(stale || analysis.result.stale) && <p className="agent-test-context-note">Настройки или сценарий изменились после запуска. Анализ относится к сохранённому запуску; точные замены подготовлены по текущему тексту источников.</p>}
      {analysis.result.recommendations.map((recommendation, index) => {
        const testArticle = recommendation.target === "knowledge_article" && testArticleIds.includes(recommendation.article_id ?? "");
        const change = testArticle ? undefined : recommendation.change;
        const target = testArticle ? "Статья внутри теста" : change?.source_key === "tool_instructions" ? "Инструкции инструментов" : targets[recommendation.target];
        return <section className="agent-test-recommendation" key={index} aria-label={`Рекомендация ${index + 1}: ${target}`}>
        <strong>{target}{recommendation.step ? ` · Шаг ${recommendation.step}` : ""}</strong>
        {recommendation.article_id && <p>{recommendation.article_title || "Статья"} · ID: {recommendation.article_id}{recommendation.article_version != null ? ` · Версия ${recommendation.article_version}` : ""}</p>}
        <p>{recommendation.reason}</p>
        {change && <div className="agent-test-replacement" aria-label="Точная замена">
          <div className="agent-test-suggested-text"><span>Было</span><p>{change.original_text}</p></div>
          <div className="agent-test-suggested-text"><span>Станет</span><p>{recommendation.suggested_text}</p></div>
        </div>}
        {!change && recommendation.suggested_text && <div className="agent-test-suggested-text"><span>Предлагаемый текст</span><p>{recommendation.suggested_text}</p></div>}
        {!change && <p className="agent-test-context-note">Точная замена не подготовлена. Эту рекомендацию нужно внести вручную.</p>}
        {onEditSource && (recommendation.target === "agent_instructions" || recommendation.target === "knowledge_article")
          ? <button type="button" className="secondary-button" disabled={disabled || saving} onClick={() => onEditSource(recommendation)}>{testArticle ? "Посмотреть статью теста" : "Редактировать источник"}</button>
          : recommendation.target === "agent_instructions" && recommendation.suggested_text && onEditInstructions && <button type="button" className="secondary-button" disabled={disabled} onClick={() => onEditInstructions(recommendation.suggested_text)}>Открыть инструкции с рекомендацией</button>}
      </section>; })}
      <div className="agent-test-apply-recommendations">
        {changes.length > 0 ? <p className="agent-test-context-note">{applied ? "Сохранены" : "Будут сохранены"} только показанные замены «Было → Станет»: {changes.length}. Остальной текст источников {applied ? "сохранён" : "сохранится"}.</p>
          : <p className="agent-test-context-note">Нет точных замен для применения. Обновите рекомендации или отредактируйте источник вручную.</p>}
        {hasUnsavedChanges && !applied && <p className="agent-test-context-note">Сохраните или отмените ручные правки в редакторе перед применением рекомендаций.</p>}
        <DemoActionButton type="button" className="primary-button" disabled={disabled || saving || applied || hasUnsavedChanges || !changes.length || analysis.application === "conflict" || analysis.application === "error"} onClick={() => void apply()}><Check size={15} />{saving ? "Применяем рекомендации…" : applied ? "Рекомендации применены" : "Применить рекомендации"}</DemoActionButton>
        {analysis.message && <p role={analysis.application === "error" || analysis.application === "conflict" ? "alert" : "status"} className={analysis.application === "saved" ? "agent-test-apply-notice" : "form-error"}>{analysis.message}</p>}
      </div>
    </>}
      </aside>
      </div>
    </section>}
  </div>;
}
