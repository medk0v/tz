import { AnimatedDetails } from "../AnimatedDetails";
import { useState } from "react";
import { BookOpen, ChevronRight, Code2, Cpu, Globe, Plug, Timer, Wrench } from "lucide-react";
import "./AgentTestTrace.css";

type TraceObject = Record<string, unknown>;

function object(value: unknown): TraceObject {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as TraceObject : {};
}

function decoded(value: unknown): unknown {
  if (typeof value !== "string") return value;
  try { return JSON.parse(value); } catch { return value; }
}

function text(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value, null, 2) ?? String(value);
}

function preview(value: unknown, limit = 160): string {
  const content = text(value).replace(/\s+/g, " ");
  return content.length > limit ? `${content.slice(0, limit)}…` : content;
}

const toolLabels: Record<string, string> = {
  read_article: "Чтение статьи", http: "HTTP-запрос", browser: "Открытие страницы",
  integration: "Вызов API", shell: "Команда", notify_operator: "Уведомление оператора",
  resolve: "Закрытие диалога", schedule_reminder: "Установка таймера",
  timer_cancelled: "Отмена таймера", timer_fired: "Срабатывание таймера",
};

function TraceValue({ label, value }: { label: string; value: unknown }) {
  const [expanded, setExpanded] = useState(false);
  const content = text(value);
  const shortened = content.length > 800;
  return <section className="agent-test-trace-value" aria-label={label}>
    <strong>{label}</strong>
    <pre>{expanded || !shortened ? content : `${content.slice(0, 800)}…`}</pre>
    {shortened && <button type="button" className="agent-test-trace-text-button" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{expanded ? "Свернуть текст" : "Показать полностью"}</button>}
  </section>;
}

function RawTrace({ value }: { value: unknown }) {
  const [open, setOpen] = useState(false);
  return <AnimatedDetails className="agent-test-trace-raw" onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary><Code2 size={16} aria-hidden="true" /> Исходные данные события</summary>
    {open && <pre>{JSON.stringify(value, null, 2) ?? String(value)}</pre>}
  </AnimatedDetails>;
}

function TraceEvent({ entry, index, testArticleIds }: { entry: unknown; index: number; testArticleIds: string[] }) {
  const [open, setOpen] = useState(false);
  const event = object(entry);
  const call = object(event.call);
  const params = object(call.parameters);
  const response = decoded(event.response);
  const output = object(response);
  const api = object(event.api);
  const tool = typeof call.tool === "string" ? call.tool : "";
  const modelEvent = !tool && "response_model" in event;
  const testArticle = tool === "read_article" && typeof params.article_id === "string" && testArticleIds.includes(params.article_id);
  const title = testArticle ? "Чтение статьи внутри теста" : tool ? Object.hasOwn(toolLabels, tool) ? toolLabels[tool] : preview(tool, 80) : modelEvent ? "Ответ модели" : "Событие";
  const Icon = tool === "read_article" ? BookOpen : tool === "integration" ? Plug
    : tool === "http" || tool === "browser" ? Globe : tool.includes("timer") || tool === "schedule_reminder" ? Timer
      : modelEvent ? Cpu : Wrench;
  const statusCode = typeof api.status_code === "number" ? api.status_code : typeof output.status_code === "number" ? output.status_code : undefined;
  const failed = output.ok === false || (typeof output.error === "string" && output.error.length > 0) || (statusCode !== undefined && statusCode >= 400);
  const inputPreview = tool === "read_article" ? output.title ?? params.article_id
    : tool === "http" ? [params.method, params.url].filter((part) => typeof part === "string").join(" ")
      : tool === "browser" ? params.url : tool === "integration" ? [params.integration_key, params.action_key].filter((part) => typeof part === "string").join(" / ")
        : modelEvent ? event.response_model : call.parameters ?? entry;
  const responsePreview = output.content ?? output.body ?? output.text ?? response;
  const step = typeof event.step === "number" && Number.isInteger(event.step) && event.step >= 0 ? event.step + 1 : undefined;
  const at = typeof event.at === "number" && Number.isFinite(event.at) ? event.at : undefined;

  return <li className="agent-test-trace-event">
    <AnimatedDetails onToggle={(toggle) => setOpen(toggle.currentTarget.open)}>
      <summary className="agent-test-trace-summary">
        <span className="agent-test-trace-number" aria-hidden="true">{index + 1}</span>
        <Icon className="agent-test-trace-icon" size={17} aria-hidden="true" />
        <span className="agent-test-trace-description">
          <span className="agent-test-trace-title">{title}{failed && <span className="agent-test-trace-error">Ошибка</span>}{statusCode !== undefined && <span className={failed ? "agent-test-trace-error" : "agent-test-trace-meta"}>HTTP {statusCode}</span>}</span>
          {inputPreview !== undefined && <span className="agent-test-trace-preview">{preview(inputPreview)}</span>}
        </span>
        <span className="agent-test-trace-position">{step !== undefined && <span>Шаг {step}</span>}{at !== undefined && <span>{at} сек. сценария</span>}</span>
        <ChevronRight className="agent-test-trace-chevron" size={16} aria-hidden="true" />
      </summary>
      {open && <div className="agent-test-trace-content">
        {tool && <div className="agent-test-trace-meta">Инструмент: <code>{tool}</code>{api.source === "browser" && " · Браузер"}</div>}
        {modelEvent ? <dl className="agent-test-trace-model">
          {[["Настроена", event.configured_model], ["Запрошена", event.request_model], ["Ответила", event.response_model]].map(([label, value]) => value !== undefined && <div key={String(label)}><dt>{String(label)}</dt><dd>{text(value)}</dd></div>)}
        </dl> : ("parameters" in call || "response" in event) && <div className="agent-test-trace-values">
          {"parameters" in call && <TraceValue label="Входные данные" value={call.parameters} />}
          {"response" in event && <TraceValue label={tool === "read_article" ? "Прочитанный фрагмент" : "Ответ"} value={responsePreview === null ? "Ответ отсутствует в трассе" : responsePreview} />}
        </div>}
        {"body" in api && api.body !== event.response && <TraceValue label="Ответ HTTP" value={decoded(api.body)} />}
        <RawTrace value={entry} />
      </div>}
    </AnimatedDetails>
  </li>;
}

export function AgentTestTrace({ trace, testArticleIds = [] }: { trace: unknown[]; testArticleIds?: string[] }) {
  if (!trace.length) return <p className="agent-test-trace-empty">В этом запуске нет событий трассировки.</p>;
  return <ol className="agent-test-trace" aria-label="События трассировки">
    {trace.map((entry, index) => <TraceEvent key={index} entry={entry} index={index} testArticleIds={testArticleIds} />)}
  </ol>;
}
