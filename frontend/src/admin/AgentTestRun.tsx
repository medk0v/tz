import { AnimatedDetails } from "../AnimatedDetails";
import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useRef, useState } from "react";
import { Check, ChevronRight, Clock, FileCode2, MoreHorizontal, Trash2, X } from "lucide-react";
import { aiTestRequest, type AiProfile, type OperatorAuth } from "../api";
import { useDateTimePreferences } from "../date-time-preferences";
import { AgentTestRecommendations, type Recommendation } from "./AgentTestRecommendations";
import { AgentTestInstructions, type AppliedSource } from "./AgentTestInstructions";
import { AgentTestTrace } from "./AgentTestTrace";

type SemanticReview = { expected_meaning: string; verdict: string; note: string; source?: "ai" | "automatic" | "human" };
export type AgentTestRunData = {
  id: string; scenario_id: string; scenario_revision: number; status: string; stale: boolean; created_at: string;
  snapshot: { scenario: { name: string; knowledge_articles?: { article_id: string }[] } } & Record<string, unknown>;
  result: {
    steps?: { reply: string; message: string; at: number; timer_remaining: number | null; operator_present: boolean; closed: boolean }[];
    trace?: unknown[]; failures?: string[]; execution_errors?: string[]; semantic_review?: SemanticReview[];
  };
};
type SourceSelection = { target: "agent_instructions" | "tool_instructions" | "knowledge_article"; articleId?: string };
const statuses: Record<string, string> = { running: "Выполняется", passed: "Пройдено", behavior_error: "Ошибка поведения", execution_error: "Ошибка выполнения", manual_review: "Нужна ручная оценка", pass: "Смысл верный", fail: "Смысл неверный" };

function TestStatus({ status }: { status: string }) {
  const label = statuses[status] ?? status;
  const passed = status === "passed" || status === "pass";
  const pending = status === "manual_review" || status === "running";
  const Icon = passed ? Check : pending ? Clock : X;
  return <span className={`agent-test-status agent-test-status--${passed ? "passed" : pending ? "pending" : "failed"}`}>
    <Icon size={17} role="img" aria-label={label} /><span aria-hidden="true">{label}</span>
  </span>;
}

function StepReview({ review, disabled, onReview }: { review: SemanticReview; disabled: boolean; onReview: (verdict: string, note: string) => void }) {
  const [note, setNote] = useState(review.note);
  return <div className="agent-test-review">
    <div className="agent-test-review-verdict"><span>{review.source === "ai" ? "Оценка ИИ:" : review.source === "human" ? "Ручная оценка:" : "Оценка:"}</span><TestStatus status={review.verdict} /></div>
    <div className="agent-test-review-copy"><div><h5>Ожидалось</h5><p>{review.expected_meaning || "Ожидаемый смысл не задан."}</p></div>
      <div><h5>Обоснование оценки</h5><p>{review.note || "Не указано"}</p></div></div>
    <AnimatedDetails><summary>Изменить оценку вручную</summary>
      <label><span>Обоснование ручной оценки</span><textarea rows={3} value={note} onChange={(event) => setNote(event.target.value)} /></label>
      <div className="agent-test-toolbar">{([['pass', 'Смысл верный'], ['fail', 'Смысл неверный'], ['manual_review', 'Оставить на проверке']] as const).map(([verdict, label]) => <DemoActionButton type="button" className="secondary-button" key={verdict} disabled={disabled || !note.trim()} onClick={() => onReview(verdict, note)}>{label}</DemoActionButton>)}</div>
    </AnimatedDetails>
  </div>;
}

type Props = {
  auth: OperatorAuth; profile: AiProfile; run: AgentTestRunData; disabled: boolean; deleteDisabled: boolean; showName?: boolean;
  onDelete: () => void;
  onReview: (step: number, verdict: string, note: string) => void;
  onProfileSaved?: (profile: AiProfile, field: "instructions" | "tool_instructions") => void;
  onSourceBusyChange?: (busy: boolean) => void;
};

export function AgentTestRun(props: Props) {
  const [scope, setScope] = useState({ auth: props.auth, revision: 0 });
  if (scope.auth !== props.auth) setScope({ auth: props.auth, revision: scope.revision + 1 });
  return <RunDetails key={`${props.profile.id}:${props.run.id}:${scope.revision}`} {...props} />;
}

function RunDetails({ auth, profile, run, disabled, deleteDisabled, showName = false, onDelete, onReview, onProfileSaved, onSourceBusyChange }: Props) {
  const { hourCycle } = useDateTimePreferences();
  const [detail, setDetail] = useState<AgentTestRunData | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [sourcesOpen, setSourcesOpen] = useState(false);
  const [source, setSource] = useState<SourceSelection>({ target: "agent_instructions" });
  const [suggestedText, setSuggestedText] = useState<string | undefined>();
  const [sourcesDirty, setSourcesDirty] = useState(false);
  const [sourcesBusy, setSourcesBusy] = useState(false);
  const [savedSources, setSavedSources] = useState<AppliedSource[]>([]);
  const request = useRef<AbortController | null>(null);
  useEffect(() => () => request.current?.abort(), [auth, profile.id, run.id]);
  const completed = ["passed", "behavior_error", "execution_error", "manual_review"].includes(run.status);

  async function loadDetail() {
    if (detail || loading) return;
    const controller = new AbortController();
    request.current = controller;
    setLoading(true); setError("");
    try {
      const result = await aiTestRequest<AgentTestRunData>(auth, profile.id, `test-runs/${encodeURIComponent(run.id)}`, { signal: controller.signal });
      if (!controller.signal.aborted) setDetail(result);
    } catch {
      if (!controller.signal.aborted) setError("Не удалось загрузить трассировку и инструкции. Попробуйте снова.");
    } finally {
      if (!controller.signal.aborted) setLoading(false);
    }
  }
  function openSources() { setSourcesOpen(true); void loadDetail(); }
  function editSource(recommendation: Recommendation) {
    setSource(recommendation.target === "knowledge_article"
      ? { target: "knowledge_article", articleId: recommendation.article_id }
      : { target: recommendation.change?.source_key === "tool_instructions" ? "tool_instructions" : "agent_instructions" });
    setSuggestedText(recommendation.suggested_text || undefined);
    openSources();
  }
  function sourceBusyChange(busy: boolean) { setSourcesBusy(busy); onSourceBusyChange?.(busy); }
  function recommendationsApplied(sources: AppliedSource[]) {
    setSavedSources(sources);
    setSuggestedText(undefined);
    for (const source of sources) {
      if (source.source_key === "instructions" || source.source_key === "tool_instructions") {
        onProfileSaved?.({ ...profile, [source.source_key]: source.text }, source.source_key);
      }
    }
  }
  const date = new Date(run.created_at);
  const testArticleIds = (detail?.snapshot.scenario.knowledge_articles ?? run.snapshot.scenario.knowledge_articles ?? []).map((article) => article.article_id);
  const dateLabel = Number.isNaN(date.getTime()) ? "Дата не указана" : new Intl.DateTimeFormat("ru", { day: "numeric", month: "long", hour: "2-digit", minute: "2-digit", year: "numeric", hourCycle }).format(date);
  return <AnimatedDetails className="agent-test-run">
    <summary className="agent-test-run-summary"><ChevronRight className="agent-test-disclosure" size={16} aria-hidden="true" />
      <span className="agent-test-run-date">{showName && <strong>{run.snapshot.scenario.name}</strong>}<time dateTime={run.created_at}>{dateLabel}</time></span>
      <TestStatus status={run.status} /><span className="agent-test-run-version">v{run.scenario_revision ?? "—"}{run.stale && <span>Устарел</span>}</span>
    </summary>
    <div className="agent-test-run-body">
      <div className="agent-test-run-tools">
        <span className="agent-test-context-note">{run.stale ? "Сценарий или настройки изменились после запуска." : `Сценарий v${run.scenario_revision ?? "—"}`}</span>
        <AnimatedDetails className="agent-test-overflow"><summary aria-label="Действия с запуском" title="Действия с запуском"><MoreHorizontal size={18} /></summary>
          <DemoActionButton type="button" className="secondary-button table-danger-link" disabled={deleteDisabled} onClick={onDelete}><Trash2 size={15} />Удалить запуск</DemoActionButton>
        </AnimatedDetails>
      </div>
      {[...(run.result.execution_errors ?? []), ...(run.result.failures ?? [])].map((failure, index) => <p className="agent-test-failure" key={index}>{failure}</p>)}
      {run.result.steps?.map((step, index) => <section className="agent-test-result-step" key={index}>
        <div className="agent-test-step-heading"><h4>Шаг {index + 1} · {step.at} сек.</h4><span>Оператор: {step.operator_present ? "подключён" : "нет"} · Диалог: {step.closed ? "закрыт" : "открыт"} · Таймер: {step.timer_remaining === null ? "нет" : `${step.timer_remaining} сек.`}</span></div>
        <div className="agent-test-conversation"><div><h5>Сообщение клиента</h5><p className="agent-test-message">{step.message || "Изменение времени / состояния"}</p></div><div><h5>Ответ агента</h5><p className="agent-test-message agent-test-reply">{step.reply || "Ответа нет"}</p></div></div>
        {run.result.semantic_review?.[index] && <StepReview key={JSON.stringify(run.result.semantic_review[index])} review={run.result.semantic_review[index]} disabled={disabled || !completed} onReview={(verdict, note) => onReview(index, verdict, note)} />}
      </section>)}
      {completed && <AgentTestRecommendations auth={auth} profileId={profile.id} runId={run.id} stale={run.stale} disabled={disabled || sourcesBusy} onOpenInstructions={openSources} onEditSource={editSource} testArticleIds={testArticleIds} hasUnsavedChanges={sourcesDirty} onApplied={recommendationsApplied} onBusyChange={sourceBusyChange}
        instructions={sourcesOpen ? detail ? <AgentTestInstructions auth={auth} profile={profile} runId={run.id} snapshot={detail.snapshot} trace={detail.result.trace ?? []} initialSource={source} suggestedText={suggestedText} onProfileSaved={onProfileSaved} onBusyChange={sourceBusyChange} savedSources={savedSources} onDirtyChange={setSourcesDirty} disabled={disabled || sourcesBusy} />
          : <div className="agent-test-source-loading">{loading ? <p role="status">Загружаем инструкции этого запуска…</p> : <button type="button" className="secondary-button" onClick={() => void loadDetail()}>Повторить загрузку</button>}</div> : undefined} />}
      {error && <p role="alert" className="form-error">{error}</p>}
      {detail ? <>
        <AnimatedDetails className="agent-test-trace-section"><summary>Трассировка ({detail.result.trace?.length ?? 0})</summary><AgentTestTrace trace={detail.result.trace ?? []} testArticleIds={testArticleIds} /></AnimatedDetails>
        <AnimatedDetails className="agent-test-snapshot"><summary>Снимок сценария и настроек</summary><pre>{JSON.stringify(detail.snapshot, null, 2)}</pre></AnimatedDetails>
      </> : <button type="button" className="secondary-button agent-test-load-trace" disabled={!completed || loading} onClick={() => void loadDetail()}><FileCode2 size={15} />{loading ? "Загружаем трассировку…" : "Загрузить трассу и снимок"}</button>}
    </div>
  </AnimatedDetails>;
}
