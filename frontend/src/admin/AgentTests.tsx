import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { AnimatedDetails } from "../AnimatedDetails";
import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Eye, MoreHorizontal, Pencil, Play, Plus, Save, Trash2, X } from "lucide-react";
import { aiTestRequest, type AiProfile, type AiChannelOption, type OperatorAuth } from "../api";
import { AgentTestRun, type AgentTestRunData as Run } from "./AgentTestRun";
import "./AgentTests.css";

type Call = { tool: string; parameters: Record<string, unknown> };
type Step = {
  message: string; advance_seconds: number; operator_present: boolean | null; closed: boolean | null;
  expected_meaning: string; required_facts: string[]; forbidden_facts: string[];
  required_actions: Call[]; forbidden_actions: Call[]; expected_timer_seconds: number | null;
  expected_timer_active: boolean | null; expected_closed: boolean | null; expected_reply: boolean | null;
};
type Scenario = {
  name: string; language: string; channel_id: string | null;
  contact?: { contact_id: string; display_name: string | null } | null;
  knowledge_articles?: { article_id: string; title: string; body: string; version?: number }[];
  history: { author: string; text: string }[]; operator_present: boolean; closed: boolean; timer_seconds: number | null;
  steps: Step[]; fixtures: { call: Call; response: string; uses: number }[]; live_allowlist: Call[]; source_articles: string[];
};
type Saved = { id: string; revision: number; scenario: Scenario };
const emptyStep = (message = ""): Step => ({ message, advance_seconds: 0, operator_present: null, closed: null, expected_meaning: "", required_facts: [], forbidden_facts: [], required_actions: [], forbidden_actions: [], expected_timer_seconds: null, expected_timer_active: null, expected_closed: null, expected_reply: null });
const newScenario = (message = "", language = "ru"): Scenario => ({ name: message.slice(0, 150), language, channel_id: null, history: [], operator_present: false, closed: false, timer_seconds: null, steps: [emptyStep(message)], fixtures: [], live_allowlist: [], source_articles: [] });
const lines = (text: string) => text.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
const contactIdPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
function JsonField({ label, value, onChange }: { label: string; value: unknown; onChange: (value: unknown) => void }) {
  const [text, setText] = useState(JSON.stringify(value, null, 2));
  const [error, setError] = useState("");
  return <label><span>{label}</span><textarea rows={5} spellCheck={false} value={text} onChange={(event) => {
    const next = event.target.value; setText(next);
    try { const parsed: unknown = JSON.parse(next); if (!Array.isArray(parsed)) throw new Error(); onChange(parsed); setError(""); }
    catch { setError("Нужен корректный JSON-массив. Исправьте перед сохранением."); }
  }} aria-invalid={Boolean(error)} />{error && <span role="alert">{error}</span>}</label>;
}

function OptionalState({ label, value, onChange }: { label: string; value: boolean | null; onChange: (value: boolean | null) => void }) {
  return <label><span>{label}</span><select value={value === null ? "" : String(value)} onChange={(event) => onChange(event.target.value === "" ? null : event.target.value === "true")}><option value="">Не задано</option><option value="true">Да</option><option value="false">Нет</option></select></label>;
}

function TestHistory({ open, id, label, children }: { open: boolean; id: string; label: string; children: ReactNode }) {
  const [visited, setVisited] = useState(open);
  if (open && !visited) setVisited(true);
  return visited || open ? <AnimatedDisclosure open={open} keepMounted className="agent-test-history" id={id} role="region" aria-label={label}>{children}</AnimatedDisclosure> : null;
}

export function AgentTests({ auth, profile, channels = [], disabledReason, onProfileSaved, onSourceBusyChange }: { auth: OperatorAuth; profile: AiProfile; channels?: AiChannelOption[]; disabledReason?: string; onProfileSaved?: (profile: AiProfile, field: "instructions" | "tool_instructions") => void; onSourceBusyChange?: (busy: boolean) => void }) {
  const [items, setItems] = useState<Saved[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [expandedHistories, setExpandedHistories] = useState<string[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [draft, setDraft] = useState<Scenario | null>(null);
  const [editing, setEditing] = useState<Saved | null>(null);
  const [editorKey, setEditorKey] = useState(0);
  const [bulk, setBulk] = useState("");
  const [busy, setBusy] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const alive = useRef(true);
  const stopQueue = useRef(false);
  useEffect(() => {
    alive.current = true;
    let disposed = false;
    async function refresh() {
      try {
        const [tests, history] = await Promise.all([aiTestRequest<{ items: Saved[] }>(auth, profile.id, "tests"), aiTestRequest<{ items: Run[] }>(auth, profile.id, "test-runs")]);
        if (!disposed) { setItems(tests.items); setRuns(history.items); }
      } catch (cause) { if (!disposed) setError(cause instanceof Error ? cause.message : "Не удалось загрузить тесты"); }
    }
    void refresh();
    const timer = window.setInterval(() => void refresh(), 6000);
    return () => { disposed = true; alive.current = false; window.clearInterval(timer); };
  }, [auth, profile.id]);

  async function reload() {
    const [tests, history] = await Promise.all([aiTestRequest<{ items: Saved[] }>(auth, profile.id, "tests"), aiTestRequest<{ items: Run[] }>(auth, profile.id, "test-runs")]);
    if (alive.current) { setItems(tests.items); setRuns(history.items); }
  }
  async function mutate(work: () => Promise<unknown>, message: string) {
    setBusy(true); setError(""); setNotice("");
    try { await work(); await reload(); setNotice(message); } catch (cause) { setError(cause instanceof Error ? cause.message : "Не удалось сохранить тест"); } finally { setBusy(false); }
  }
  function edit(item: Saved | null) { setError(""); setEditing(item); setDraft(item ? structuredClone(item.scenario) : newScenario("", profile.language.split(",")[0]?.trim() || "ru")); setEditorKey((key) => key + 1); }
  function patchStep(index: number, change: Partial<Step>) { setDraft((current) => current && ({ ...current, steps: current.steps.map((step, position) => position === index ? { ...step, ...change } : step) })); }
  function patchContact(change: Partial<NonNullable<Scenario["contact"]>>) { setDraft((current) => current?.contact ? { ...current, contact: { ...current.contact, ...change } } : current); }
  async function launch(ids: string[]) {
    setRunning(true); setError(""); setNotice(""); stopQueue.current = false;
    try {
      for (const [index, id] of ids.entries()) {
        if (!alive.current || stopQueue.current) break;
        setExpandedHistories((current) => current.includes(id) ? current : [...current, id]);
        setNotice(`Запуск ${index + 1} из ${ids.length}. Используются сохранённые настройки агента.`);
        const started = await aiTestRequest<{ id: string }>(auth, profile.id, "test-runs", { method: "POST", body: JSON.stringify({ scenario_id: id }) });
        while (alive.current) {
          const history = await aiTestRequest<{ items: Run[] }>(auth, profile.id, "test-runs");
          if (alive.current) setRuns(history.items);
          if (history.items.some((run) => run.id === started.id && run.status !== "running")) break;
          await new Promise((resolve) => window.setTimeout(resolve, 3000));
        }
      }
      if (alive.current) setNotice(stopQueue.current ? "Очередь остановлена" : "Запуски завершены");
    } catch (cause) { if (alive.current) { setNotice(""); setError(cause instanceof Error ? cause.message : "Ошибка запуска"); } }
    finally { if (alive.current) setRunning(false); }
  }
  const runDisabled = busy || running || runs.some((run) => run.status === "running");
  // Saved scenarios can be removed while their snapshots run, but not from a pending local queue.
  const deleteDisabled = busy || running;

  const selectedIds = items.filter((item) => selected.includes(item.id)).map((item) => item.id);
  const allSelected = items.length > 0 && selectedIds.length === items.length;
  async function clearTestHistory(item: Saved) {
    await mutate(async () => {
      await aiTestRequest(auth, profile.id, `tests/${encodeURIComponent(item.id)}/runs`, { method: "DELETE" });
      setRuns((current) => current.filter((run) => run.scenario_id !== item.id || run.status === "running"));
    }, `История завершённых запусков теста «${item.scenario.name}» очищена`);
  }
  async function removeTests(ids: string[]) {
    setBusy(true); setError(""); setNotice("");
    let deleted = 0;
    try {
      for (const id of ids) {
        await aiTestRequest(auth, profile.id, `tests/${id}`, { method: "DELETE" });
        deleted += 1;
        setItems((current) => current.filter((item) => item.id !== id));
        setSelected((current) => current.filter((selectedId) => selectedId !== id));
        setExpandedHistories((current) => current.filter((scenarioId) => scenarioId !== id));
        if (editing?.id === id) { setDraft(null); setEditing(null); }
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Не удалось удалить тесты");
    } finally {
      if (deleted > 0) setNotice(`Удалено тестов: ${deleted}. История запусков сохранена`);
      setBusy(false);
    }
  }

  const saveWaitReason = disabledReason || (busy ? "Дождитесь завершения текущей операции с тестами." : "");
  const saveErrorId = `agent-test-save-error-${profile.id}`;
  const contactError = draft?.contact && !contactIdPattern.test(draft.contact.contact_id.trim()) ? "Укажите ID контакта в формате UUID." : "";
  const contactErrorId = `agent-test-contact-error-${profile.id}`;
  const editor = draft && <div className="agent-test-editor" key={editorKey}>
      <div className="agent-test-editor-heading"><h4>{editing ? "Редактирование теста" : "Новый тест"}</h4><div className="agent-test-editor-controls"><div className="agent-test-toolbar"><DemoActionButton type="button" className="primary-button" disabled={Boolean(saveWaitReason) || !draft.name.trim()} title={saveWaitReason || undefined} aria-describedby={error ? saveErrorId : undefined} onClick={(event) => {
        if (contactError) { setError(contactError); return; }
        if (event.currentTarget.closest(".agent-test-editor")?.querySelector('[aria-invalid="true"]')) { setError("Исправьте JSON перед сохранением"); return; }
        const scenario = draft.contact ? { ...draft, contact: { contact_id: draft.contact.contact_id.trim(), display_name: draft.contact.display_name?.trim() || null } } : draft;
        void mutate(async () => { await aiTestRequest(auth, profile.id, editing ? `tests/${editing.id}` : "tests", { method: editing ? "PUT" : "POST", body: JSON.stringify(editing ? { revision: editing.revision, scenario } : { scenarios: [scenario] }) }); setDraft(null); }, "Тест сохранён");
      }}><Save size={15} />{saveWaitReason ? "Подождите…" : "Сохранить тест"}</DemoActionButton><button type="button" className="secondary-button" onClick={() => setDraft(null)}>Отмена</button></div>
      {error && <p id={saveErrorId} role="alert" className="form-error agent-test-save-error">{error}</p>}</div></div>
      {saveWaitReason && <p role="status">{saveWaitReason}</p>}
      <label><span>Название</span><input value={draft.name} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></label>
      <div className="agent-test-fields"><label><span>Язык</span><input value={draft.language} onChange={(event) => setDraft({ ...draft, language: event.target.value })} /></label><label><span>Назначенный канал</span><select value={draft.channel_id ?? ""} onChange={(event) => setDraft({ ...draft, channel_id: event.target.value || null })}><option value="">Первый активный канал</option>{profile.channel_ids.map((id) => <option key={id} value={id}>{channels.find((channel) => channel.id === id)?.name ?? id}</option>)}</select></label><label><span>Начальный таймер, секунд</span><input type="number" min={10} max={82800} value={draft.timer_seconds ?? ""} onChange={(event) => setDraft({ ...draft, timer_seconds: event.target.value === "" ? null : Number(event.target.value) })} /></label></div>
      <div className="agent-test-toolbar"><label><input type="checkbox" checked={Boolean(draft.contact)} onChange={(event) => setDraft({ ...draft, contact: event.target.checked ? { contact_id: crypto.randomUUID(), display_name: null } : null })} />Контакт в тесте</label></div>
      {draft.contact && <>
        <p className="agent-test-context-note">Эти данные заменяют карточку контакта для агента в тесте. Реальный контакт не создаётся.</p>
        <div className="agent-test-fields">
          <label><span>ID контакта</span><input value={draft.contact.contact_id} spellCheck={false} autoComplete="off" aria-invalid={Boolean(contactError)} aria-describedby={contactError ? contactErrorId : undefined} onChange={(event) => patchContact({ contact_id: event.target.value })} />{contactError && <span id={contactErrorId}>{contactError}</span>}</label>
          <label><span>Имя контакта</span><input value={draft.contact.display_name ?? ""} maxLength={250} autoComplete="off" placeholder="Необязательно" onChange={(event) => patchContact({ display_name: event.target.value || null })} /></label>
        </div>
      </>}
      <div className="agent-test-toolbar"><label><input type="checkbox" checked={draft.operator_present} onChange={(event) => setDraft({ ...draft, operator_present: event.target.checked })} />Оператор уже подключён</label><label><input type="checkbox" checked={draft.closed} onChange={(event) => setDraft({ ...draft, closed: event.target.checked })} />Диалог закрыт</label></div>
      <AnimatedDetails><summary>Начальная история</summary><p>Массив сообщений: {`[{"author":"contact","text":"Здравствуйте"}]`}. Автор: contact, ai или operator.</p><JsonField label="История" value={draft.history} onChange={(history) => setDraft({ ...draft, history: history as Scenario["history"] })} /></AnimatedDetails>
      {draft.steps.map((step, index) => <section className="agent-test-step" key={index}>
        <div className="agent-test-toolbar"><h4>Шаг {index + 1}</h4>{draft.steps.length > 1 && <button className="icon-button" type="button" aria-label={`Удалить шаг ${index + 1}`} onClick={() => { setDraft({ ...draft, steps: draft.steps.filter((_, position) => position !== index) }); setEditorKey((key) => key + 1); }}><Trash2 size={15} /></button>}</div>
        <div className="agent-test-step-fields"><label><span>Сообщение клиента (пустое — только состояние/время)</span><textarea rows={3} value={step.message} onChange={(event) => patchStep(index, { message: event.target.value })} /></label>
        <label><span>Ожидаемый смысл — проверяется ИИ после ответа</span><textarea rows={3} value={step.expected_meaning} onChange={(event) => patchStep(index, { expected_meaning: event.target.value })} /></label></div>
        <AnimatedDetails><summary>Факты и точные фразы</summary><div className="agent-test-fields"><label><span>Обязательные факты / точные фразы, по строке</span><textarea rows={3} value={step.required_facts.join("\n")} onChange={(event) => patchStep(index, { required_facts: event.target.value.split("\n") })} onBlur={(event) => patchStep(index, { required_facts: lines(event.target.value) })} /></label><label><span>Запрещённые факты / точные фразы, по строке</span><textarea rows={3} value={step.forbidden_facts.join("\n")} onChange={(event) => patchStep(index, { forbidden_facts: event.target.value.split("\n") })} onBlur={(event) => patchStep(index, { forbidden_facts: lines(event.target.value) })} /></label></div></AnimatedDetails>
        <AnimatedDetails><summary>Время, оператор и строгие проверки действий</summary><div className="agent-test-fields"><label><span>Продвинуть время на, секунд</span><input type="number" min={0} max={86400} value={step.advance_seconds} onChange={(event) => patchStep(index, { advance_seconds: Number(event.target.value) })} /></label><OptionalState label="Подключить оператора" value={step.operator_present} onChange={(value) => patchStep(index, { operator_present: value })} /><OptionalState label="Закрыть диалог" value={step.closed} onChange={(value) => patchStep(index, { closed: value })} /><OptionalState label="Ожидается ответ" value={step.expected_reply} onChange={(value) => patchStep(index, { expected_reply: value })} /><OptionalState label="Ожидается активный таймер" value={step.expected_timer_active} onChange={(value) => patchStep(index, { expected_timer_active: value })} /><OptionalState label="Ожидается закрытый диалог" value={step.expected_closed} onChange={(value) => patchStep(index, { expected_closed: value })} /><label><span>Ожидаемый остаток таймера, секунд</span><input type="number" min={0} value={step.expected_timer_seconds ?? ""} onChange={(event) => patchStep(index, { expected_timer_seconds: event.target.value === "" ? null : Number(event.target.value) })} /></label></div>
          <p>Действия сопоставляются точно: tool и parameters. Пример: {`[{"tool":"schedule_reminder","parameters":{"delay_seconds":300}}]`}.</p>
          <JsonField label="Обязательные действия" value={step.required_actions} onChange={(value) => patchStep(index, { required_actions: value as Call[] })} /><JsonField label="Запрещённые действия" value={step.forbidden_actions} onChange={(value) => patchStep(index, { forbidden_actions: value as Call[] })} />
        </AnimatedDetails>
      </section>)}
      <button type="button" className="secondary-button" onClick={() => setDraft({ ...draft, steps: [...draft.steps, emptyStep()] })} disabled={draft.steps.length >= 20}>Добавить шаг</button>
      <AnimatedDetails><summary>Статьи внутри теста</summary>
        <p>Эти статьи хранятся только в сценарии теста. Во время запуска агент видит их в каталоге и читает как обычные статьи. Они не добавляются в рабочую базу знаний.</p>
        <p>Укажите отдельный UUID, заголовок и текст каждой статьи. Версия необязательна, по умолчанию — 1. Пример: {`[{"article_id":"00000000-0000-4000-8000-000000000001","title":"Персональный ответ","body":"Заготовленный ответ","version":1}]`}.</p>
        <JsonField label="Статьи теста" value={draft.knowledge_articles ?? []} onChange={(value) => setDraft({ ...draft, knowledge_articles: value as Scenario["knowledge_articles"] })} />
      </AnimatedDetails>
      <AnimatedDetails><summary>Подготовленные ответы и реальные запросы</summary>
        <p>Ответ выдаётся после точного вызова. Инструменты: http, browser, integration, shell, notify_operator, resolve, schedule_reminder. Для http параметры — method, url и body для POST; для browser — url, ответ содержит текст страницы в text; для integration — integration_key, action_key, parameters. response — строка вывода; uses — число выдач. У integration формат: ok, status_code, content_type, body (строка). Не вставляйте секреты.</p>
        <JsonField label="Подготовленные ответы" value={draft.fixtures} onChange={(value) => setDraft({ ...draft, fixtures: value as Scenario["fixtures"] })} /><JsonField label="Точные разрешённые реальные вызовы" value={draft.live_allowlist} onChange={(value) => setDraft({ ...draft, live_allowlist: value as Call[] })} />
        <p>Подходящий подготовленный ответ всегда используется первым. Если его нет, пустой список разрешает HTTP GET и открытие страниц в браузере по правам GET и доменам агента, а заполненный — только точные вызовы из списка. Для browser укажите вызов с URL открываемой страницы. POST и интеграции требуют точного разрешения в списке. Для shell доступны только подготовленные ответы. Реальные запросы могут изменить внешние данные.</p>
      </AnimatedDetails>

    </div>;

  const renderRun = (run: Run, showName = false) => <AgentTestRun key={run.id} auth={auth} profile={profile} run={run} showName={showName}
    disabled={busy} deleteDisabled={busy || running || run.status === "running"} onProfileSaved={onProfileSaved} onSourceBusyChange={onSourceBusyChange}
    onDelete={() => void mutate(() => aiTestRequest(auth, profile.id, `test-runs/${run.id}`, { method: "DELETE" }), "Запуск удалён из истории")}
    onReview={(step, verdict, note) => void mutate(() => aiTestRequest(auth, profile.id, `test-runs/${run.id}/review`, { method: "POST", body: JSON.stringify({ step, verdict, note }) }), "Оценка сохранена")} />;

  const itemIds = new Set(items.map((item) => item.id));
  const deletedTestRuns = runs.filter((run) => !itemIds.has(run.scenario_id));

  return <div className="agent-tests">
    <div className="agent-test-toolbar">
      <button type="button" className="secondary-button" onClick={() => edit(null)}><Plus size={15} />Новый тест</button>
      <DemoActionButton type="button" className="secondary-button" disabled={busy} onClick={() => void mutate(() => aiTestRequest(auth, profile.id, "test-seeds", { method: "POST" }), "Добавлены сценарии по статьям назначенной базы знаний. Проверьте ожидаемый смысл перед оценкой.")}>Добавить набор по базе знаний</DemoActionButton>
      <DemoActionButton type="button" className="secondary-button" disabled={busy || running || !runs.some((run) => run.status !== "running")} onClick={() => void mutate(async () => {
        await aiTestRequest(auth, profile.id, "test-runs", { method: "DELETE" });
      }, "История завершённых запусков очищена")}><Trash2 size={15} />Очистить историю запуска тестов</DemoActionButton>
    </div>
    <p>Запуски используют сохранённые настройки, опубликованную базу знаний и статьи внутри выбранного теста. Сообщения, уведомления, закрытие диалога и таймеры изолированы от рабочих чатов.</p>
    {error && !draft && <p role="alert" className="form-error">{error}</p>}{notice && <p role="status">{notice}</p>}
    {!editing && editor}
    <div className="agent-test-help"><AnimatedDetails><summary>Добавить вопросы списком</summary><label><span>Каждый вопрос с новой строки</span><textarea rows={5} value={bulk} onChange={(event) => setBulk(event.target.value)} /></label><DemoActionButton type="button" className="secondary-button" disabled={busy || !bulk.trim()} onClick={() => void mutate(async () => { await aiTestRequest(auth, profile.id, "tests", { method: "POST", body: JSON.stringify({ scenarios: lines(bulk).map((question) => newScenario(question, profile.language.split(",")[0]?.trim() || "ru")) }) }); setBulk(""); }, "Вопросы добавлены")}>Добавить вопросы</DemoActionButton></AnimatedDetails>
    <AnimatedDetails><summary>Как оцениваются результаты</summary><p>После ответа ИИ сопоставляет его с ожидаемым смыслом и сохраняет оценку с обоснованием. Факты проверяются точным вхождением фраз, действия — совпадением параметров. Оценка смысла не отменяет строгие ошибки.</p></AnimatedDetails></div>
    <div className="agent-test-batch-actions"><div className="agent-test-toolbar agent-test-toolbar--launch"><DemoActionButton type="button" className="secondary-button" disabled={runDisabled || selectedIds.length === 0} onClick={() => void launch(selectedIds)}><Play size={15} />Выбранные ({selectedIds.length})</DemoActionButton><DemoActionButton type="button" className="secondary-button" disabled={runDisabled || !items.length} onClick={() => void launch(items.map((item) => item.id))}>Весь набор</DemoActionButton>{running && <button type="button" className="secondary-button" onClick={() => { stopQueue.current = true; setNotice("Текущий запуск завершится, остальные остановлены."); }}>Остановить очередь</button>}</div>
    <div className="agent-test-toolbar">
      <button type="button" className="secondary-button" disabled={busy || !items.length} onClick={() => setSelected(allSelected ? [] : items.map((item) => item.id))}>{allSelected ? "Снять выделение" : "Выделить все"}</button>
      <DemoActionButton type="button" className="secondary-button agent-test-delete" disabled={deleteDisabled || selectedIds.length === 0} title={running ? "Дождитесь завершения очереди запусков" : undefined} onClick={() => void removeTests(selectedIds)}><Trash2 size={15} />Удалить выбранные ({selectedIds.length})</DemoActionButton>
    </div>
    </div>
    <div className="agent-test-list">{items.map((item) => {
      const itemRuns = runs.filter((run) => run.scenario_id === item.id);
      const isEditing = Boolean(draft && editing?.id === item.id);
      const historyExpanded = expandedHistories.includes(item.id);
      const historyId = `agent-test-history-${item.id}`;
      return <section className="agent-test-item" key={item.id} aria-label={`Тест ${item.scenario.name}`}>
        <div className="agent-test-row">
          <input type="checkbox" aria-label={`Выбрать ${item.scenario.name}`} disabled={busy} checked={selected.includes(item.id)} onChange={(event) => setSelected((current) => event.target.checked ? [...current, item.id] : current.filter((id) => id !== item.id))} />
          <div className="agent-test-name">{item.scenario.name}<span>{item.scenario.language} · {item.scenario.steps.length} шаг. · v{item.revision}</span></div>
          <div className="agent-test-actions">
            <DemoActionButton type="button" className="primary-button" aria-label={`Запустить ${item.scenario.name}`} disabled={runDisabled || isEditing} onClick={() => void launch([item.id])}><Play size={15} /><span>Запустить</span></DemoActionButton>
            <button type="button" className="icon-button" aria-label={`${historyExpanded ? "Скрыть" : "Показать"} историю запусков: ${item.scenario.name}`} title={historyExpanded ? "Скрыть историю запусков" : "Показать историю запусков"} aria-expanded={historyExpanded} aria-controls={historyId} onClick={() => setExpandedHistories((current) => historyExpanded ? current.filter((id) => id !== item.id) : [...current, item.id])}><Eye size={15} /></button>
            <button type="button" className="secondary-button" aria-label={`${isEditing ? "Закрыть редактирование" : "Редактировать"} ${item.scenario.name}`} aria-expanded={isEditing} onClick={() => {
              if (isEditing) { setDraft(null); setEditing(null); } else edit(item);
            }}>{isEditing ? <X size={15} /> : <Pencil size={15} />}<span>{isEditing ? "Закрыть" : "Редактировать"}</span></button>
            <AnimatedDetails className="agent-test-overflow"><summary aria-label={`Действия с тестом ${item.scenario.name}`} title="Действия с тестом"><MoreHorizontal size={18} /></summary><DemoActionButton type="button" className="secondary-button agent-test-delete" aria-label={`Удалить ${item.scenario.name}`} disabled={deleteDisabled} title={running ? "Дождитесь завершения очереди запусков" : undefined} onClick={() => void removeTests([item.id])}><Trash2 size={15} />Удалить тест</DemoActionButton></AnimatedDetails>
          </div>
        </div>
        {isEditing && editor}
        <TestHistory open={historyExpanded} id={historyId} label={`История запусков: ${item.scenario.name}`}>
          <div className="agent-test-toolbar agent-test-history-heading">
            <h4>История запусков</h4>
            <button type="button" className="secondary-button" aria-label={`Очистить историю запусков: ${item.scenario.name}`} disabled={busy || running || !itemRuns.some((run) => run.status !== "running")} onClick={() => void clearTestHistory(item)}><Trash2 size={15} />Очистить историю</button>
          </div>
          {itemRuns.length > 0 ? <div className="agent-test-runs"><div className="agent-test-runs-heading" aria-hidden="true"><span /><span>Запуск</span><span>Результат</span><span>Версия</span></div>{itemRuns.map((run) => renderRun(run))}</div> : <p className="agent-test-empty">Запусков пока нет.</p>}
        </TestHistory>
      </section>;
    })}</div>
    {items.length === 0 && <p>Тестов пока нет.</p>}
    {deletedTestRuns.length > 0 && <section className="agent-test-history" aria-label="Запуски удалённых тестов">
      <h4>Запуски удалённых тестов</h4>
      {deletedTestRuns.map((run) => renderRun(run, true))}
    </section>}
  </div>;
}
