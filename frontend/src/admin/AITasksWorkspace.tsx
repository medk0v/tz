import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { AnimatedDetails } from "../AnimatedDetails";
import type { ResourceVisibility } from "../resource-visibility";
import { useContentMotion } from "./useContentMotion";
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, } from "react";
import { ArrowDown, ArrowLeft, ArrowUp, Bot, CalendarClock, Check, CheckCircle2, ChevronRight, Circle, Copy, FileText, Inbox as LoaderCircle, Play, Plus, RefreshCw, Search, Settings2, ShieldCheck, ShoppingCart, Square, Trash2, UserRound, Users, X, XCircle } from "lucide-react";
import { isChatModelConnection } from "../ai-model-types";
import { createAiProfile, listAiProfiles, listAiProviders, listAiTaskRuns, type AiProfile, type AiProfileInput, type AiProvider, type AiTaskRun, type Inbox, type OperatorAuth } from "../api";
import { cancelExecution, getExecution, listExecutions, listTasks, removeTask, retryExecutionStep, saveTask, startExecution, startIndependentRuns, type ExecutionStep, type Task, type TaskInput, type TaskPlanPerformer, type TaskPlanStep, type TaskSchedule, type TeamExecution } from "../task-orchestration-api";
import { type TaskAgentOption, type TaskPerson } from "../task-board-api";
import { MessageBody } from "../MessageBody";
import { useDateTimePreferences, type HourCycle } from "../date-time-preferences";
import { useI18n, type Locale } from "../i18n";
import { DateTimeField } from "./DateTimeField";
import { DemoActionButton, useDemoReadOnly } from "./DemoReadOnly";
import { TaskScreenshots } from "./TaskScreenshots";
import { useCanonicalPageRoute, usePageRoute } from "./page-route";
import { fill } from "./task-text";
import { browserTimezone, errorText, profileAvailable, taskTitle, validSchedule } from "./task-schedule";
import { invalidSpan } from "./task-span";
import { taskWorkspaceText, type TaskWorkspaceText } from "./task-workspace-i18n";
import "./AITasksWorkspace.css";

/** What the signed-in employee may do with tasks. */
export interface TaskAccessFlags { own: boolean; manage: boolean; configure: boolean; ai: boolean }
interface Props { auth: OperatorAuth; inboxes: Inbox[]; canManageApi?: boolean; access?: TaskAccessFlags }
type Profile = TaskAgentOption;
type ResultTab = "result" | "steps" | "history";
type WorkspaceMode = "list";
/** The run shown first: a team execution or an independent occurrence opened from the calendar. */
interface RunFocus { executionId?: string | null; occurrence?: number | null }
const activeStatuses = new Set(["queued", "planning", "running", "reviewing"]);
const DEFAULT_ACCESS: TaskAccessFlags = { own: true, manage: true, configure: false, ai: true };
const DEFAULT_MODE: WorkspaceMode = "list";
const MAX_PLAN_STEPS = 30;

/** Task URLs: `/tasks[/calendar|/list]`, then `/<task id|new>` for an open task and `/<task id>/runs[/steps|/history]` for its runs. */
function taskRoute(segments: readonly string[]) {
  const [first] = segments;
  const mode: WorkspaceMode | null = first === "list" ? first : null;
  const [taskId = null, section, tabName] = mode ? segments.slice(1) : segments;
  const tab: ResultTab = tabName === "steps" || tabName === "history" ? tabName : "result";
  return { mode, taskId, runs: section === "runs", tab };
}
function taskSegments(mode: WorkspaceMode, taskId: string | null = null, runs = false, tab: ResultTab = "result"): string[] {
  const view = mode === DEFAULT_MODE ? [] : [mode];
  if (!taskId) return view;
  return runs ? [...view, taskId, "runs", ...(tab === "result" ? [] : [tab])] : [...view, taskId];
}
/** The next full hour, used for tasks created from the toolbar in calendar mode. */

function emptyTask(): TaskInput {
  return { text: "", attachment_ids: [], agent_ids: [], schedule: { kind: "manual" }, execution_mode: "independent", coordinator_id: null, expected_result: "", agent_roles: [], ...boardFields(null) };
}
/** Employees, placement, the time span and tags exist only where the board is available. */
function boardFields(task: Task | null): Partial<TaskInput> {
  return {};
  return {
    assignee_ids: [...(task?.assignee_ids ?? [])], coordinator_user_id: task?.coordinator_user_id ?? null,
    list_id: task?.list_id ?? null, column_id: task?.column_id ?? null,
    starts_at: task?.starts_at ?? null, due_at: task?.due_at ?? null, all_day: task?.all_day ?? false, time_zone: task?.time_zone ?? null,
    reminder_minutes: task?.reminder_minutes ?? null,
    tag_ids: [...(task?.tag_ids ?? [])],
  };
}
function inputFromTask(task: Task): TaskInput {
  return {
    visibility: task.visibility,
    text: task.text, agent_ids: [...task.agent_ids], schedule: structuredClone(task.schedule),
    attachment_ids: [...(task.attachment_ids ?? [])],
    execution_mode: task.execution_mode ?? "independent", coordinator_id: task.coordinator_id ?? null,
    expected_result: task.expected_result ?? "", agent_roles: structuredClone(task.agent_roles ?? []),
    ...boardFields(task),
    ...(Array.isArray(task.plan_steps) ? { plan_steps: task.plan_steps.map((step) => ({ id: step.id, title: step.title, instructions: step.instructions, performer: { kind: step.performer.kind, id: step.performer.id } })) } : {}),
  };
}
/** The predefined plan of a draft; ordinary tasks have none. */
function planOf(input: TaskInput): TaskPlanStep[] | null { return Array.isArray(input.plan_steps) ? input.plan_steps : null; }
function processLabel(task: Task, text: TaskWorkspaceText, short = true): string | null {
  return task.process ? fill(text(short ? "byProcessShort" : "byProcess"), { title: task.process.title, number: task.process.version }) : null;
}
function planSummary(task: Task, text: TaskWorkspaceText): string {
  return [processLabel(task, text), task.plan_progress ? `${task.plan_progress.done}/${task.plan_progress.total}` : null].filter(Boolean).join(" · ");
}
function missingPerformer(performer: TaskPlanPerformer, text: TaskWorkspaceText): string {
  return `${text(performer.kind === "employee" ? "employee" : "agent")} · ${text("planPerformerMissing")}`;
}
function agentOption(profile: AiProfile & { preset_key?: string | null }, providers: AiProvider[]): Profile {
  return { id: profile.id, name: profile.name, preset_key: profile.preset_key ?? null, ready: profileAvailable(profile, providers) };
}

function localDateTime(value: string): string {
  if (!value) return "";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? "" : new Date(date.getTime() - date.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
}
function scheduleFor(kind: TaskSchedule["kind"], current: TaskSchedule): TaskSchedule {
  if (kind === "manual") return { kind };
  if (kind === "once") return { kind, run_at: "" };
  const time = "time" in current ? current.time : "09:00";
  const timezone = "timezone" in current ? current.timezone : browserTimezone();
  const deadline = "ends_at" in current ? { ends_at: current.ends_at } : {};
  if (kind === "weekly") return { kind, time, timezone, ...deadline, weekdays: [1] };
  if (kind === "monthly") return { kind, time, timezone, ...deadline, month_days: [1] };
  if (kind === "yearly") return { kind, time, timezone, ...deadline, dates: [{ month: 1, day: 1 }] };
  return { kind, time, timezone, ...deadline };
}
function displayDate(value: string | null | undefined, locale: Locale, hourCycle: HourCycle | undefined): string {
  return value ? new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short", hourCycle }).format(new Date(value)) : "—";
}

function AgentIcon({ profile, human = false }: { profile?: Profile; human?: boolean }) {
  const key = profile?.preset_key;
  const Icon = human ? UserRound : key?.includes("sales") ? ShoppingCart : key?.includes("quality") ? ShieldCheck : key?.includes("operation") ? Settings2 : key?.includes("hr") ? Users : key?.includes("universal") ? UserRound : Bot;
  return <span className={`task-agent-icon task-agent-icon--${human ? "person" : key?.includes("quality") ? "quality" : key?.includes("operation") ? "operations" : "default"}`}><Icon size={21} aria-hidden="true" /></span>;
}

function ScheduleFields({ value, onChange, text, locale, disabled }: { value: TaskSchedule; onChange: (value: TaskSchedule) => void; text: TaskWorkspaceText; locale: Locale; disabled: boolean }) {
  const scheduleMotion = useContentMotion<HTMLDivElement>(value.kind);
  const week = Array.from({ length: 7 }, (_, i) => new Intl.DateTimeFormat(locale, { weekday: "short", timeZone: "UTC" }).format(new Date(Date.UTC(2026, 0, 5 + i))));
  return <div ref={scheduleMotion} className="task-schedule">
    <label className="task-field"><span>{text("schedule")}</span><select value={value.kind} onChange={(event) => onChange(scheduleFor(event.target.value as TaskSchedule["kind"], value))}>
      {(["manual", "once", "daily", "weekly", "monthly", "yearly"] as const).map((kind) => <option key={kind} value={kind}>{text(kind)}</option>)}
    </select></label>
    {value.kind === "once" && <div className="task-field"><span id="task-run-at-label">{text("runAt")}</span><DateTimeField mode="datetime" locale={locale} required disabled={disabled} aria-labelledby="task-run-at-label" value={localDateTime(value.run_at)} onChange={(next) => onChange({ ...value, run_at: next ? new Date(next).toISOString() : "" })} /></div>}
    {"time" in value && <div className="task-schedule-time"><div className="task-field"><span id="task-time-label">{text("time")}</span><DateTimeField mode="time" locale={locale} required disabled={disabled} aria-labelledby="task-time-label" value={value.time} onChange={(time) => onChange({ ...value, time })} /></div><label className="task-field"><span>{text("timezone")}</span><input required value={value.timezone} maxLength={100} onChange={(event) => onChange({ ...value, timezone: event.target.value })} /></label></div>}
    {value.kind === "weekly" && <fieldset className="task-day-fieldset"><legend>{text("weekdays")}</legend><div className="task-day-choices">{week.map((day, i) => <label key={i}><input type="checkbox" checked={value.weekdays.includes(i + 1)} onChange={(event) => onChange({ ...value, weekdays: event.target.checked ? [...value.weekdays, i + 1].sort((a, b) => a - b) : value.weekdays.filter((d) => d !== i + 1) })} /><span>{day}</span></label>)}</div></fieldset>}
    {value.kind === "monthly" && <fieldset className="task-day-fieldset"><legend>{text("monthDays")}</legend><div className="task-day-choices task-day-choices--month">{Array.from({ length: 31 }, (_, i) => i + 1).map((day) => <label key={day}><input type="checkbox" checked={value.month_days.includes(day)} onChange={(event) => onChange({ ...value, month_days: event.target.checked ? [...value.month_days, day].sort((a, b) => a - b) : value.month_days.filter((d) => d !== day) })} /><span>{day}</span></label>)}</div></fieldset>}
    {value.kind === "yearly" && <fieldset className="task-day-fieldset"><legend>{text("dates")}</legend><div className="task-annual-dates">{value.dates.map((date, i) => <div className="task-annual-date" key={i}><label className="task-field"><span>{text("month")}</span><select aria-label={`${text("month")} ${i + 1}`} value={date.month} onChange={(event) => onChange({ ...value, dates: value.dates.map((d, index) => index === i ? { ...d, month: Number(event.target.value) } : d) })}>{Array.from({ length: 12 }, (_, index) => <option key={index} value={index + 1}>{new Intl.DateTimeFormat(locale, { month: "long", timeZone: "UTC" }).format(new Date(Date.UTC(2000, index, 1)))}</option>)}</select></label><label className="task-field"><span>{text("day")}</span><input aria-label={`${text("day")} ${i + 1}`} type="number" min={1} max={31} value={date.day} onChange={(event) => onChange({ ...value, dates: value.dates.map((d, index) => index === i ? { ...d, day: Number(event.target.value) } : d) })} /></label><button type="button" className="task-icon-button" aria-label={`${text("removeDate")} ${i + 1}`} onClick={() => onChange({ ...value, dates: value.dates.filter((_, index) => index !== i) })}><X size={16} /></button></div>)}</div><button className="task-text-button" type="button" onClick={() => onChange({ ...value, dates: [...value.dates, { month: 1, day: 1 }] })}><Plus size={15} />{text("addDate")}</button></fieldset>}
    {"time" in value && <div className="task-field"><span id="task-ends-at-label">{text("endsAt")}</span><DateTimeField mode="datetime" locale={locale} disabled={disabled} value={value.ends_at ?? ""} aria-labelledby="task-ends-at-label" aria-describedby="task-ends-at-help" onChange={(next) => onChange({ ...value, ends_at: next ? next.length === 16 ? `${next}:00` : next : null })} /><span className="task-muted" id="task-ends-at-help">{text("endsAtHelp")} {value.timezone}</span></div>}
  </div>;
}

function CreateAgentDialog({ auth, providers, text, locale, onClose, onCreated }: { auth: OperatorAuth; providers: AiProvider[]; text: TaskWorkspaceText; locale: Locale; onClose: () => void; onCreated: (profile: AiProfile) => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState("");
  const visibility: ResourceVisibility | undefined = undefined;
  const [providerId, setProviderId] = useState("");
  const [model, setModel] = useState("");
  const [instructions, setInstructions] = useState("");
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (dialog.current?.showModal) dialog.current.showModal();
    else dialog.current?.setAttribute("open", "");
  }, []);
  async function create(event: FormEvent) {
    event.preventDefault();
    if (saving || auth.isDemo || !name.trim() || !instructions.trim()) return;
    setError(""); setSaving(true);
    const input: AiProfileInput = {
      visibility,
      name: name.trim(), status: providerId ? "active" : "draft", provider_connection_id: providerId || null, model: providerId ? model.trim() || null : null,
      instructions: instructions.trim(), tool_instructions: "", blacklist_reply_text: "", blacklist_reply_match_language: true,
      http_allowed_hosts: [], language: locale, max_output_tokens: 4000, auto_join_new_conversations: false,
      can_resolve_conversations: false, capabilities: { http_get: false, http_post: false, shell: false },
      telegram_notifications: { new_visitor: false, new_message: false, operator_request: false },
      custom_fields: [], secrets: [], knowledge_base_ids: [], channel_ids: [], public_identities: [{ language: locale, display_name: name.trim() }],
    };
    try { onCreated(await createAiProfile(auth, input)); }
    catch (failure) { setError(errorText(failure, text("createError"))); setSaving(false); }
  }
  return <dialog ref={dialog} className="task-agent-dialog" aria-labelledby="task-agent-dialog-title" onCancel={(event) => { event.preventDefault(); if (!saving) onClose(); }}>
    <form onSubmit={(event) => void create(event)}><header><h2 id="task-agent-dialog-title">{text("createAgent")}</h2><button type="button" className="task-icon-button" aria-label={text("cancel")} disabled={saving} onClick={onClose}><X size={19} /></button></header>
      <p className="task-muted">{text("agentHelp")}</p>
      <fieldset disabled={saving} className="task-form-fields"><label className="task-field"><span>{text("agentName")}</span><input autoFocus required maxLength={200} value={name} onChange={(event) => setName(event.target.value)} /></label>
        <label className="task-field"><span id="task-agent-provider-label">{text("provider")}</span><select value={providerId} aria-labelledby="task-agent-provider-label" aria-describedby={!providerId ? "task-agent-draft-help" : undefined} onChange={(event) => setProviderId(event.target.value)}><option value="">{text("connectLater")}</option>{providers.filter((provider) => provider.status === "active" && provider.provider_kind !== "anthropic" && isChatModelConnection(provider)).map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}</select>{!providerId && <span className="task-muted" id="task-agent-draft-help">{text("agentDraftHelp")}</span>}</label>
        <label className="task-field"><span>{text("model")}</span><input value={model} disabled={!providerId} maxLength={200} placeholder={providers.find((provider) => provider.id === providerId)?.default_model} onChange={(event) => setModel(event.target.value)} /></label>
        <label className="task-field"><span>{text("instructions")}</span><textarea required rows={5} maxLength={24000} value={instructions} onChange={(event) => setInstructions(event.target.value)} /></label>
      </fieldset>
      {error && <p className="task-error" role="alert" id="task-agent-create-error">{error}</p>}
      <footer><DemoActionButton className="primary-button" type="submit" disabled={saving || !name.trim() || !instructions.trim()} aria-describedby={error ? "task-agent-create-error" : undefined}>{saving ? text("creating") : text("create")}</DemoActionButton><button type="button" className="task-text-button" disabled={saving} onClick={onClose}>{text("cancel")}</button></footer>
    </form>
  </dialog>;
}

interface PlanStepsProps { steps: TaskPlanStep[]; profiles: Profile[]; people: TaskPerson[]; text: TaskWorkspaceText; onChange: (steps: TaskPlanStep[]) => void }
/** The predefined steps of a plan task; their performers are the team. */
function PlanSteps({ steps, profiles, people, text, onChange }: PlanStepsProps) {
  const patch = (index: number, change: Partial<TaskPlanStep>) => onChange(steps.map((step, position) => position === index ? { ...step, ...change } : step));
  const move = (index: number, offset: -1 | 1) => {
    const next = [...steps];
    [next[index], next[index + offset]] = [next[index + offset], next[index]];
    onChange(next);
  };
  return <section className="task-plan" aria-labelledby="task-plan-title"><h3 id="task-plan-title">{text("planSteps")} <span>{steps.length}/{MAX_PLAN_STEPS}</span></h3>
    <ol className="task-plan-steps">{steps.map((step, index) => {
      const number = index + 1;
      const label = fill(text("planStepNumber"), { number });
      const performerKey = step.performer.id ? `${step.performer.kind}:${step.performer.id}` : "";
      const known = step.performer.kind === "employee" ? people.some((person) => person.id === step.performer.id) : profiles.some((profile) => profile.id === step.performer.id);
      return <li key={step.id} className="task-plan-step" aria-label={label}>
        <div className="task-plan-step-heading"><strong>{label}</strong>
          <button type="button" className="task-icon-button" aria-label={fill(text("planMoveStepUp"), { number })} disabled={index === 0} onClick={() => move(index, -1)}><ArrowUp size={15} /></button>
          <button type="button" className="task-icon-button" aria-label={fill(text("planMoveStepDown"), { number })} disabled={index === steps.length - 1} onClick={() => move(index, 1)}><ArrowDown size={15} /></button>
          <button type="button" className="task-icon-button" aria-label={fill(text("planRemoveStep"), { number })} disabled={steps.length === 1} onClick={() => onChange(steps.filter((item) => item.id !== step.id))}><Trash2 size={15} /></button>
        </div>
        <label className="task-field"><span>{text("stepTitle")}</span><input maxLength={200} value={step.title} onChange={(event) => patch(index, { title: event.target.value })} /></label>
        <label className="task-field"><span>{text("stepInstructions")}</span><textarea rows={3} maxLength={10000} value={step.instructions} onChange={(event) => patch(index, { instructions: event.target.value })} /></label>
        <label className="task-field"><span>{text("planPerformer")}</span><select value={performerKey} onChange={(event) => { const [kind, id = ""] = event.target.value.split(":"); patch(index, { performer: { kind: kind === "employee" ? "employee" : "agent", id } }); }}>
          <option value="">{text("choosePerformer")}</option>
          {performerKey && !known && <option value={performerKey}>{missingPerformer(step.performer, text)}</option>}
          {people.length > 0 && <optgroup label={text("employees")}>{people.map((person) => <option value={`employee:${person.id}`} key={person.id}>{person.name}</option>)}</optgroup>}
          <optgroup label={text("planAiAgents")}>{profiles.map((profile) => <option value={`agent:${profile.id}`} key={profile.id}>{profile.name}</option>)}</optgroup>
        </select></label>
      </li>;
    })}</ol>
    <button type="button" className="secondary-button task-add-agent" disabled={steps.length >= MAX_PLAN_STEPS} onClick={() => onChange([...steps, { id: crypto.randomUUID(), title: "", instructions: "", performer: { kind: "agent", id: "" } }])}><Plus size={16} />{text("addStep")}</button>
  </section>;
}

interface TeamRailProps {
  draft: TaskInput; profiles: Profile[]; people: TaskPerson[]; canCreateAgent: boolean; text: TaskWorkspaceText;
  onChange: (patch: Partial<TaskInput>) => void; onCreate: () => void;
}
function TeamRail({ draft, profiles, people, canCreateAgent, text, onChange, onCreate }: TeamRailProps) {
  const [adding, setAdding] = useState(false);
  const plan = planOf(draft);
  const isTeam = plan !== null || draft.execution_mode === "team";
  const withPeople = people.length > 0;
  const assignees = draft.assignee_ids ?? [];
  const coordinatorKey = draft.coordinator_user_id ? `user:${draft.coordinator_user_id}` : draft.coordinator_id ? `agent:${draft.coordinator_id}` : "";
  const teamMotion = useContentMotion<HTMLElement>(draft.execution_mode);
  const coordinatorMotion = useContentMotion<HTMLDivElement>(`${draft.execution_mode}:${coordinatorKey}`);
  const coordinator = profiles.find((profile) => profile.id === draft.coordinator_id);
  const coordinatorPerson = people.find((person) => person.id === draft.coordinator_user_id);
  const members = draft.agent_ids.length + (isTeam ? assignees.length : 0);
  const limit = isTeam ? 8 : 32;
  const agentsFull = isTeam ? members >= limit : draft.agent_ids.length >= limit;
  const peopleFull = isTeam ? members >= limit : assignees.length >= limit;
  const agentCandidates = profiles.filter((profile) => !draft.agent_ids.includes(profile.id) && (!isTeam || profile.id !== draft.coordinator_id));
  const personCandidates = people.filter((person) => !assignees.includes(person.id) && (!isTeam || person.id !== draft.coordinator_user_id));
  function addAgent(profile: Profile) {
    if (agentsFull) return;
    onChange({ agent_ids: [...draft.agent_ids, profile.id], agent_roles: [...draft.agent_roles, { agent_id: profile.id, role: "" }] });
    setAdding(false);
  }
  function addPerson(person: TaskPerson) {
    if (peopleFull) return;
    onChange({ assignee_ids: [...assignees, person.id], agent_roles: [...draft.agent_roles, { agent_id: person.id, role: "" }] });
    setAdding(false);
  }
  function chooseCoordinator(value: string) {
    const [kind, id] = value.split(":");
    // The coordinator of a plan may perform its steps, so the plan and its roles stay.
    if (plan) { onChange({ coordinator_id: kind === "agent" ? id : null, ...(withPeople ? { coordinator_user_id: kind === "user" ? id : null } : {}) }); return; }
    onChange({
      coordinator_id: kind === "agent" ? id : null,
      ...(withPeople ? { coordinator_user_id: kind === "user" ? id : null, assignee_ids: assignees.filter((item) => item !== id) } : {}),
      agent_ids: draft.agent_ids.filter((item) => item !== id), agent_roles: draft.agent_roles.filter((role) => role.agent_id !== id),
    });
  }
  const title = isTeam ? text("teamTitle") : withPeople ? text("peopleAndAgents") : text("agents");
  const roleInput = (id: string, name: string) => {
    const role = draft.agent_roles.find((item) => item.agent_id === id)?.role ?? "";
    return <input className="task-role-input" aria-label={`${text("role")}: ${name}`} placeholder={text("rolePlaceholder")} maxLength={500} value={role} onChange={(event) => onChange({ agent_roles: [...draft.agent_roles.filter((item) => item.agent_id !== id), { agent_id: id, role: event.target.value }] })} />;
  };
  return <aside ref={teamMotion} className="task-team-rail" aria-label={title}>
    <h2>{title}</h2><p className="task-muted">{isTeam ? text("teamHelp") : withPeople ? text("independentHumanHelp") : text("independentHelp")}{isTeam && withPeople ? ` ${text("teamEmployeeHelp")}` : ""}</p>
    {isTeam && <div className="task-coordinator"><label className="task-field"><span>{text("coordinator")}</span><div ref={coordinatorMotion} className="task-coordinator-select"><AgentIcon profile={coordinator} human={Boolean(coordinatorPerson)} />
      <select value={coordinatorKey} onChange={(event) => chooseCoordinator(event.target.value)}><option value="">{text("chooseCoordinator")}</option>
        {withPeople ? <><optgroup label={text("agents")}>{profiles.map((profile) => <option value={`agent:${profile.id}`} key={profile.id}>{profile.name}</option>)}</optgroup><optgroup label={text("employees")}>{people.map((person) => <option value={`user:${person.id}`} key={person.id}>{person.name}</option>)}</optgroup></>
          : profiles.map((profile) => <option value={`agent:${profile.id}`} key={profile.id}>{profile.name}</option>)}
      </select></div></label>
      {coordinator && <span className="task-muted task-profile-kind">{text(coordinator.preset_key ? "preset" : "custom")}{coordinator.ready ? "" : ` · ${text("unavailable")}`}</span>}
      {coordinatorPerson && <span className="task-muted task-profile-kind">{text("employee")} · {text("coordinatorEmployeeHelp")}</span>}
    </div>}
    {plan ? <PlanSteps steps={plan} profiles={profiles} people={people} text={text} onChange={(plan_steps) => onChange({ plan_steps })} /> : <div className="task-members"><h3>{text(isTeam ? "members" : withPeople ? "members" : "agents")} <span>{isTeam ? `${members}/${limit}` : `${draft.agent_ids.length + (withPeople ? assignees.length : 0)}`}</span></h3>
      {draft.agent_ids.map((id) => {
        const profile = profiles.find((item) => item.id === id);
        return <div className="task-member" key={id}><div className="task-member-summary"><AgentIcon profile={profile} /><div><strong>{profile?.name ?? text("unavailable")}</strong><span className={profile?.ready ? "task-muted" : "task-unavailable"}>{profile?.ready ? text(profile.preset_key ? "preset" : "custom") : text("unavailable")}</span></div><button className="task-icon-button" type="button" aria-label={`${text("removeAgent")}: ${profile?.name ?? id}`} onClick={() => onChange({ agent_ids: draft.agent_ids.filter((item) => item !== id), agent_roles: draft.agent_roles.filter((item) => item.agent_id !== id) })}><X size={16} /></button></div>
          {isTeam && roleInput(id, profile?.name ?? id)}
        </div>;
      })}
      {withPeople && assignees.map((id) => {
        const person = people.find((item) => item.id === id);
        return <div className="task-member" key={id}><div className="task-member-summary"><AgentIcon human /><div><strong>{person?.name ?? text("unavailable")}</strong><span className="task-muted">{text("employee")}</span></div><button className="task-icon-button" type="button" aria-label={`${text("removeAgent")}: ${person?.name ?? id}`} onClick={() => onChange({ assignee_ids: assignees.filter((item) => item !== id), agent_roles: draft.agent_roles.filter((item) => item.agent_id !== id) })}><X size={16} /></button></div>
          {isTeam && roleInput(id, person?.name ?? id)}
        </div>;
      })}
      {draft.agent_ids.length === 0 && assignees.length === 0 && <p className="task-muted">{profiles.length || withPeople ? text(isTeam ? "invalidTeam" : "noParticipants") : text("noAgents")}</p>}
      <button type="button" className="secondary-button task-add-agent" aria-expanded={adding} disabled={agentsFull && (!withPeople || peopleFull)} onClick={() => setAdding((value) => !value)}><Plus size={16} />{text(withPeople ? "addParticipant" : "addAgent")}</button>
      <AnimatedDisclosure open={adding} className="task-agent-picker"><h4>{text("projectAgents")}</h4>{agentCandidates.map((profile) => <button key={profile.id} type="button" disabled={agentsFull} onClick={() => addAgent(profile)}><AgentIcon profile={profile} /><span><strong>{profile.name}</strong><span className="task-muted">{profile.ready ? text(profile.preset_key ? "preset" : "custom") : text("unavailable")}</span></span><Plus size={15} /></button>)}{agentCandidates.length === 0 && <p className="task-muted">{text("allAdded")}</p>}
        {withPeople && <><h4>{text("employees")}</h4>{personCandidates.map((person) => <button key={person.id} type="button" disabled={peopleFull} onClick={() => addPerson(person)}><AgentIcon human /><span><strong>{person.name}</strong><span className="task-muted">{text("employee")}</span></span><Plus size={15} /></button>)}{personCandidates.length === 0 && <p className="task-muted">{text("allAdded")}</p>}</>}
        {canCreateAgent && <DemoActionButton className="task-create-agent" type="button" onClick={() => { setAdding(false); onCreate(); }}><Plus size={16} />{text("createAgent")}</DemoActionButton>}</AnimatedDisclosure>
      {!adding && profiles.length === 0 && canCreateAgent && <DemoActionButton type="button" className="task-text-button" onClick={onCreate}><Plus size={15} />{text("createAgent")}</DemoActionButton>}
      <p className="task-muted task-preset-help">{text("presetHelp")}</p>
    </div>}
  </aside>;
}

function StatusIcon({ status }: { status: string }) {
  if (status === "succeeded" || status === "completed") return <CheckCircle2 size={20} />;
  if (status === "failed" || status === "cancelled" || status === "blocked") return <XCircle size={20} />;
  if (activeStatuses.has(status) || status === "processing") return <LoaderCircle size={20} className="task-spin" />;
  return <Circle size={20} />;
}

function orderedExecutionSteps(steps: ExecutionStep[]): ExecutionStep[] {
  const positions = new Map(steps.map((step, index) => [step.id, index]));
  const placed = new Set<string>();
  const pending = [...steps];
  const ordered: ExecutionStep[] = [];
  const rank = { planning: 0, work: 1, review: 2 };
  const compare = (left: ExecutionStep, right: ExecutionStep): number => {
    if (left.kind === "planning" || right.kind === "planning") {
      const planningOrder = Number(right.kind === "planning") - Number(left.kind === "planning");
      if (planningOrder) return planningOrder;
    }
    const leftTime = Date.parse(left.started_at ?? left.finished_at ?? "");
    const rightTime = Date.parse(right.started_at ?? right.finished_at ?? "");
    if (Number.isFinite(leftTime) && Number.isFinite(rightTime) && leftTime !== rightTime) return leftTime - rightTime;
    if (Number.isFinite(leftTime) !== Number.isFinite(rightTime)) return Number.isFinite(leftTime) ? -1 : 1;
    return rank[left.kind] - rank[right.kind] || positions.get(left.id)! - positions.get(right.id)!;
  };
  while (pending.length) {
    // Dependencies also place correction rounds after the review that requested them.
    const ready = pending.filter((step) => step.depends_on.every((id) => !positions.has(id) || placed.has(id)));
    // Keep every step visible even if an incomplete response contains a dependency cycle.
    const next = [...(ready.length ? ready : pending)].sort(compare)[0];
    ordered.push(next);
    placed.add(next.id);
    pending.splice(pending.indexOf(next), 1);
  }
  return ordered;
}

function ExecutionTimeline({ execution, text, locale, detailed = false, busy, onRetry }: { execution: TeamExecution; text: TaskWorkspaceText; locale: Locale; detailed?: boolean; busy: boolean; onRetry: (id: string) => void }) {
  const { hourCycle } = useDateTimePreferences();
  return <ol className={`task-timeline ${detailed ? "task-timeline--detailed" : ""}`}>
    {orderedExecutionSteps(execution.steps).map((step) => <li key={step.id} className={`task-timeline-step task-status--${step.status}`}>
      <span className="task-timeline-icon"><StatusIcon status={step.status} /></span><div className="task-step-content"><div className="task-step-title"><strong>{step.kind === "planning" ? text("planning") : step.kind === "review" ? text("reviewing") : step.title}</strong><span>{step.started_at ? new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit", hourCycle }).format(new Date(step.started_at)) : text(step.status)}</span></div><p className="task-muted">{step.agent_name || text(step.kind === "work" ? "executor" : "coordinator")}{step.assignee_user_id ? ` · ${text("employee")}` : ""} · {step.assignee_user_id && step.status === "running" ? text("awaitingEmployee") : text(step.status)}</p>
        {step.status === "pending" && step.depends_on.length > 0 && <p className="task-muted">{text("waiting")}: {step.depends_on.map((id) => execution.steps.find((item) => item.id === id)?.title ?? id).join(", ")}</p>}
        {(step.input_text || step.result_text || step.error) && <AnimatedDetails open={detailed || step.status === "failed"} className="task-step-details"><summary>{text("stepResult")}</summary>{step.input_text && <AnimatedDetails><summary>{text("input")}</summary><p className="task-plain-text">{step.input_text}</p></AnimatedDetails>}{step.result_text && <MessageBody body={step.result_text} format="markdown" variant="document" />}{step.error && <p className="task-error">{step.error}</p>}<p className="task-muted">{text("attempts")}: {step.attempts ?? 0}</p></AnimatedDetails>}
        {step.may_have_effects && step.status === "failed" && <p className="task-unavailable">{text("needsReview")}</p>}
        {step.status === "failed" && !step.may_have_effects && !activeStatuses.has(execution.status) && execution.status !== "cancelled" && <DemoActionButton className="secondary-button" type="button" disabled={busy} onClick={() => onRetry(step.id)}><RefreshCw size={14} />{text("retryStep")}</DemoActionButton>}
      </div>
    </li>)}
  </ol>;
}

export function AITasksWorkspace({ auth, access = DEFAULT_ACCESS }: Props) {
  const { locale } = useI18n();
  const { hourCycle } = useDateTimePreferences();
  const text = useMemo(() => taskWorkspaceText(locale), [locale]);
  const readOnly = useDemoReadOnly() || !!auth.isDemo;
  const manage = access.manage;
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const routeSegmentsRef = useRef(route.segments);
  const requested = taskRoute(route.segments);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [providers, setProviders] = useState<AiProvider[]>([]);
  const [drafts, setDrafts] = useState<Record<string, TaskInput>>({ new: emptyTask() });
  const [search, setSearch] = useState("");
  const [loadState, setLoadState] = useState<"loading" | "ready" | "error">("loading");
  const [loadVersion, setLoadVersion] = useState(0);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<"save" | "run" | "delete" | "execution" | "tag" | null>(null);
  const [uploadingScreenshots, setUploadingScreenshots] = useState(false);
  const [creatingAgent, setCreatingAgent] = useState(false);
  const [executions, setExecutions] = useState<TeamExecution[]>([]);
  const [independentRuns, setIndependentRuns] = useState<AiTaskRun[]>([]);
  const [executionId, setExecutionId] = useState<string | null>(null);
  const [runOccurrence, setRunOccurrence] = useState<number | null>(null);
  /** The task whose run history is held, and its pending load. */
  const [historyTaskId, setHistoryTaskId] = useState<string | null>(null);
  const [historyLoad, setHistoryLoad] = useState<{ task: Task; executionId: string | null } | null>(null);
  // Until the URL names a view, the page opens the last used one; afterwards the bare URL means the default view.
  // The URL names the view, the open task (the list selection or the drawer on boards) and its runs; anything else falls back to the page's default.
  const mode = DEFAULT_MODE;
  const routedTaskId = requested.taskId === "new" ? manage ? "new" : null
    : requested.taskId && (loadState === "loading" || tasks.some((task) => task.id === requested.taskId)) ? requested.taskId : null;
  const selectedId = routedTaskId ?? tasks[0]?.id ?? "new";
  const selected = tasks.find((item) => item.id === selectedId);
  const view = requested.runs && routedTaskId && selected ? "execution" : "editor";
  const draft = drafts[selectedId] ?? emptyTask();
  // The span is read and written in the task's own zone; without one, the viewer's.
  const plan = planOf(draft);
  const isTeam = view === "execution" ? selected?.execution_mode === "team" : plan !== null || draft.execution_mode === "team";
  const tab: ResultTab = requested.tab === "steps" && !isTeam ? "result" : requested.tab;
  const editorMotion = useContentMotion<HTMLFormElement>(`${view}:${selectedId}`);
  const resultMotion = useContentMotion<HTMLDivElement>(`${view}:${tab}:${executionId}`);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [historyError, setHistoryError] = useState("");
  const [copied, setCopied] = useState(false);
  const runKeys = useRef<Record<string, string>>({});
  const historyRequest = useRef(0);
  const modeMotion = useContentMotion<HTMLDivElement>(draft.execution_mode);
  const execution = executions.find((item) => item.id === executionId);
  const dirty = !selected || JSON.stringify(draft) !== JSON.stringify(inputFromTask(selected));
  const title = taskTitle(view === "execution" ? selected?.text ?? draft.text : draft.text, text("newTask"));
  const error = errors[selectedId];
  const people = useMemo<TaskPerson[]>(() => [], []);
  const peopleNames = useMemo(() => new Map(people.map((person) => [person.id, person.name])), [people]);

  useEffect(() => { routeSegmentsRef.current = route.segments; }, [route.segments]);
  useCanonicalPageRoute(route, taskSegments(mode, view === "execution" ? selectedId : routedTaskId, view === "execution", tab), loadState === "ready");
  // A run that is still starting is not shown once the operator moves to another task or view.
  useEffect(() => { historyRequest.current += 1; }, [selectedId, view, mode]);
  // The agent dialog adds its agent to the task it was opened for.
  const [agentTaskId, setAgentTaskId] = useState(selectedId);
  if (agentTaskId !== selectedId) { setAgentTaskId(selectedId); setCreatingAgent(false); }

  useEffect(() => {
    let active = true;
    const ai = access.ai ? Promise.all([listAiProfiles(auth), listAiProviders(auth)]) : Promise.resolve(null);
    Promise.all([listTasks(auth), ai]).then(([nextTasks, aiData]) => {
      if (!active) return;
      setTasks(nextTasks);
      if (aiData) { setProviders(aiData[1]); setProfiles(aiData[0].map((profile) => agentOption(profile, aiData[1]))); }
      else setProfiles([]);
      setDrafts((current) => ({ ...Object.fromEntries(nextTasks.map((task) => [task.id, inputFromTask(task)])), ...current }));
      setLoadState("ready");
    }).catch(() => { if (active) setLoadState("error"); });
    return () => { active = false; historyRequest.current += 1; };
  }, [auth, loadVersion, access.ai]);

  const patchDraft = (patch: Partial<TaskInput>) => {
    setDrafts((current) => ({ ...current, [selectedId]: { ...(current[selectedId] ?? emptyTask()), ...patch } }));
    setErrors((current) => ({ ...current, [selectedId]: "" }));
  };
  /** Selects a task in the list, or opens it in the drawer on boards. */
  function selectTask(id: string) { void navigateRoute(taskSegments(mode, id)); }
  function startNewTask() {
    setDrafts((current) => ({ ...current, new: emptyTask() }));
    selectTask("new");
  }
  /** Keeps unsaved edits while taking server-side changes of fields the user did not touch. */
  function validation(): string | null {
    const assignees = draft.assignee_ids ?? [];
    if (!draft.text.trim()) return text("invalidText");
    if (plan) {
      if (!(draft.coordinator_id ?? draft.coordinator_user_id)) return text("chooseCoordinator");
      if (plan.length > MAX_PLAN_STEPS) return text("invalidPlanLimit");
      if (plan.length === 0 || plan.some((step) => !step.title.trim() || !step.instructions.trim() || !step.performer.id)) return text("invalidPlanSteps");
    } else if (isTeam) {
      const coordinator = draft.coordinator_id ?? draft.coordinator_user_id;
      const members = draft.agent_ids.length + assignees.length;
      if (!coordinator || members < 1 || members > 8 || draft.agent_ids.includes(coordinator) || assignees.includes(coordinator)) return text("invalidTeam");
    } else {
      if (draft.agent_ids.length > 32 || assignees.length > 32) return text("invalidAgents");
      if (draft.schedule.kind !== "manual" && draft.agent_ids.length === 0) return text("invalidAgents");
      if (draft.agent_ids.length === 0) return text("invalidAgents");
    }
    if (!validSchedule(draft.schedule)) return text("invalidSchedule");
    if (invalidSpan(draft)) return text("invalidRange");
    if (draft.reminder_minutes != null && !draft.starts_at && !draft.due_at) return text("invalidReminder");
    const executing = [...(plan ? plan.filter((step) => step.performer.kind === "agent").map((step) => step.performer.id) : draft.agent_ids), ...(isTeam && draft.coordinator_id ? [draft.coordinator_id] : [])];
    if ((!isTeam || draft.schedule.kind !== "manual") && executing.some((id) => !profiles.find((profile) => profile.id === id)?.ready)) return text("invalidProvider");
    return null;
  }
  async function submitTask(event: FormEvent) {
    event.preventDefault();
    if (busy || uploadingScreenshots || readOnly) return;
    const invalid = validation();
    if (invalid) { setErrors((current) => ({ ...current, [selectedId]: invalid })); return; }
    const savingId = selectedId;
    const members = [...draft.agent_ids, ...(draft.assignee_ids ?? [])];
    setBusy("save"); setErrors((current) => ({ ...current, [savingId]: "" }));
    try {
      const input: TaskInput = {
        ...draft, text: draft.text.trim(), expected_result: draft.expected_result.trim(),
        coordinator_id: isTeam ? draft.coordinator_id : null,
        ...({}),
        agent_roles: isTeam ? members.map((id) => ({ agent_id: id, role: draft.agent_roles.find((item) => item.agent_id === id)?.role.trim() ?? "" })) : [],
      };
      if (plan) {
        // The server derives the team from the performers of the steps.
        const performers = new Set(plan.map((step) => step.performer.id));
        Object.assign(input, {
          execution_mode: "team", agent_ids: [], plan_steps: plan.map((step) => ({ ...step, title: step.title.trim(), instructions: step.instructions.trim() })),
          agent_roles: draft.agent_roles.filter((role) => performers.has(role.agent_id)).map((role) => ({ agent_id: role.agent_id, role: role.role.trim() })),
        } satisfies Partial<TaskInput>);
        delete input.assignee_ids;
      }
      const saved = await saveTask(auth, selected?.id ?? null, input);
      setTasks((current) => [saved, ...current.filter((item) => item.id !== saved.id)]);
      setDrafts((current) => ({ ...current, [saved.id]: inputFromTask(saved), ...(savingId === "new" ? { new: emptyTask() } : {}) }));
      if (savingId === "new" && taskRoute(routeSegmentsRef.current).taskId === "new") void navigateRoute(taskSegments(mode, saved.id), { replace: true });
    } catch (failure) { setErrors((current) => ({ ...current, [savingId]: errorText(failure, text("saveError")) })); }
    finally { setBusy(null); }
  }
  async function deleteSelected() {
    if (!selected || busy || uploadingScreenshots || readOnly || !window.confirm(text("deleteConfirm"))) return;
    setBusy("delete");
    try {
      await removeTask(auth, selected.id);
      // The page then shows the first remaining task or closes the drawer, and the URL follows.
      setTasks((current) => current.filter((item) => item.id !== selected.id));
    } catch (failure) { setErrors((current) => ({ ...current, [selectedId]: errorText(failure, text("saveError")) })); }
    finally { setBusy(null); }
  }
  /** Loads the run history of a task, optionally focused on one team execution or independent occurrence. */
  function loadHistory(task: Task, focus: RunFocus = {}) {
    setHistoryTaskId(task.id); setHistoryLoad({ task, executionId: focus.executionId ?? null }); setHistoryLoading(true); setHistoryError(""); setCopied(false);
    setExecutions([]); setIndependentRuns([]); setExecutionId(null); setRunOccurrence(focus.occurrence ?? null);
  }
  function openRuns(task: Task, focus: RunFocus = {}) {
    loadHistory(task, focus);
    void navigateRoute(taskSegments(mode, task.id, true));
  }
  function reloadHistory(focus: RunFocus = {}) {
    if (!selected) return;
    historyRequest.current += 1;
    loadHistory(selected, focus);
  }
  // Runs opened from a link, a reload or Back/Forward load their task's history too.
  if (view === "execution" && selected && selected.id !== historyTaskId) loadHistory(selected);
  useEffect(() => {
    if (!historyLoad) return;
    let active = true;
    const { task, executionId: focused } = historyLoad;
    const load = async () => {
      try {
        if (task.execution_mode === "team") {
          let items = await listExecutions(auth, task.id);
          // The list holds only recent executions; an older one opened from the calendar is fetched directly.
          if (focused && !items.some((item) => item.id === focused)) items = [...items, await getExecution(auth, focused)].sort((a, b) => b.created_at.localeCompare(a.created_at));
          if (!active) return;
          setExecutions(items); setExecutionId(focused && items.some((item) => item.id === focused) ? focused : items[0]?.id ?? null);
        } else {
          const items = await listAiTaskRuns(auth, task.id);
          if (active) setIndependentRuns(items);
        }
      } catch (failure) { if (active) setHistoryError(errorText(failure, text("loadError"))); }
      finally { if (active) setHistoryLoading(false); }
    };
    void load();
    return () => { active = false; };
  }, [auth, historyLoad, text]);
  const updateExecution = useCallback((updated: TeamExecution) => {
    setExecutions((current) => [updated, ...current.filter((item) => item.id !== updated.id)].sort((a, b) => b.created_at.localeCompare(a.created_at)));
  }, []);
  const pollingExecutionId = view === "execution" && execution && activeStatuses.has(execution.status) ? execution.id : null;
  useEffect(() => {
    if (!pollingExecutionId) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { const next = await getExecution(auth, pollingExecutionId); if (active) { updateExecution(next); setHistoryError(""); } }
      catch (failure) { if (active) setHistoryError(errorText(failure, text("loadError"))); }
      if (active) timer = setTimeout(() => void poll(), 3000);
    };
    timer = setTimeout(() => void poll(), 1500);
    return () => { active = false; clearTimeout(timer); };
  }, [auth, pollingExecutionId, updateExecution, text]);
  const pollingIndependentTaskId = view === "execution" && selected && selected.execution_mode !== "team" && independentRuns.some((run) => run.status === "pending" || run.status === "processing") ? selected.id : null;
  useEffect(() => {
    if (!pollingIndependentTaskId) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try { const items = await listAiTaskRuns(auth, pollingIndependentTaskId); if (active) { setIndependentRuns(items); setHistoryError(""); } }
      catch (failure) { if (active) setHistoryError(errorText(failure, text("loadError"))); }
      if (active) timer = setTimeout(() => void poll(), 3000);
    };
    timer = setTimeout(() => void poll(), 2000);
    return () => { active = false; clearTimeout(timer); };
  }, [auth, pollingIndependentTaskId, text]);

  async function runTask() {
    if (!selected || dirty || busy || readOnly) return;
    const taskId = selected.id;
    const request = ++historyRequest.current;
    const key = runKeys.current[taskId] ?? crypto.randomUUID();
    runKeys.current[taskId] = key;
    setBusy("run"); setErrors((current) => ({ ...current, [taskId]: "" })); setHistoryError(""); setHistoryLoading(false); setHistoryLoad(null);
    try {
      if (selected.execution_mode === "team") {
        const started = await startExecution(auth, taskId, key);
        if (request !== historyRequest.current) return;
        setExecutions((current) => [started, ...current.filter((item) => item.id !== started.id && item.task_id === taskId)]); setExecutionId(started.id);
      } else {
        const items = await startIndependentRuns(auth, taskId, key);
        if (request !== historyRequest.current) return;
        setIndependentRuns(items); setExecutionId(null);
      }
      // The runs page shows the started run without reloading the history.
      delete runKeys.current[taskId]; setHistoryTaskId(taskId); setCopied(false); setRunOccurrence(null);
      void navigateRoute(taskSegments(mode, taskId, true));
      
    } catch (failure) { setErrors((current) => ({ ...current, [taskId]: errorText(failure, text("saveError")) })); }
    finally { setBusy(null); }
  }
  async function mutateExecution(stepId?: string) {
    if (!execution || busy || readOnly) return;
    setBusy("execution"); setHistoryError("");
    try { updateExecution(await (stepId ? retryExecutionStep(auth, execution.id, stepId) : cancelExecution(auth, execution.id))); }
    catch (failure) { setHistoryError(errorText(failure, text("loadError"))); }
    finally { setBusy(null); }
  }
  async function copyResult() {
    if (!execution?.result_text) return;
    try { await navigator.clipboard.writeText(execution.result_text); setCopied(true); }
    catch { setHistoryError(text("copyError")); }
  }

  // Board: moving cards between statuses of a list or kinds of the combined boards.

  // An occurrence opened from the calendar, or the latest one when it is no longer retained.
  const latestOccurrence = independentRuns[0] ? Date.parse(independentRuns[0].scheduled_for) : null;
  const shownOccurrence = runOccurrence !== null && independentRuns.some((run) => Date.parse(run.scheduled_for) === runOccurrence) ? runOccurrence : latestOccurrence;
  const canRun = Boolean(selected && (selected.execution_mode === "team" || selected.agent_ids.length > 0));
  const runButton = manage && canRun ? <DemoActionButton className="primary-button" type="button" disabled={busy !== null || uploadingScreenshots || dirty || !!(execution && activeStatuses.has(execution.status))} title={dirty ? text("saveBeforeRun") : undefined} onClick={() => void runTask()}>{busy === "run" ? <LoaderCircle size={16} className="task-spin" /> : <Play size={16} />}{text(busy === "run" ? "runningAction" : view === "execution" ? "runAgain" : "runNow")}</DemoActionButton> : null;

  const editorForm = <form ref={editorMotion} className="task-editor-form" onSubmit={(event) => void submitTask(event)}>
    <fieldset disabled={busy !== null || loadState !== "ready"} className="task-editor-fields">
      <div className="task-editor-main"><header className="task-editor-heading"><h2>{title}</h2>{selected?.process && <p className="task-muted task-process-line">{processLabel(selected, text, false)}</p>}<span className="task-muted">{text(!selected ? "draft" : dirty ? "unsaved" : "saved")}</span></header>
        <div className={`task-mode-picker ${plan ? "task-mode-picker--locked" : ""}`} data-mode={isTeam ? "team" : "independent"} role="group" aria-label={text("executionMode")} aria-describedby={plan ? "task-plan-hint" : undefined}><button type="button" aria-pressed={!isTeam} disabled={plan !== null} className={!isTeam ? "task-mode--active" : ""} onClick={() => patchDraft({ execution_mode: "independent" })}><Bot size={18} />{text("independent")}</button><button type="button" aria-pressed={isTeam} disabled={plan !== null} className={isTeam ? "task-mode--active" : ""} onClick={() => patchDraft({ execution_mode: "team" })}><Users size={18} />{text("team")}</button></div>
        {plan && <p id="task-plan-hint" className="task-muted task-plan-hint">{text("planTeamHint")}</p>}
        <div ref={modeMotion} className="task-editor-content">
          <TaskScreenshots key={selectedId} auth={auth} value={draft.attachment_ids ?? []} text={text} disabled={busy !== null || loadState !== "ready"} onChange={(attachment_ids) => patchDraft({ attachment_ids })} onBusyChange={setUploadingScreenshots}>
            <label className="task-field task-description"><span>{text("description")}</span><textarea rows={5} maxLength={50000} value={draft.text} onChange={(event) => patchDraft({ text: event.target.value })} /></label>
          </TaskScreenshots>
          {isTeam && <label className="task-field task-expected-result"><span>{text("expectedResult")}</span><textarea rows={3} maxLength={5000} value={draft.expected_result} onChange={(event) => patchDraft({ expected_result: event.target.value })} /></label>}
          <ScheduleFields value={draft.schedule} onChange={(schedule) => patchDraft({ schedule })} text={text} locale={locale} disabled={busy !== null || loadState !== "ready"} />
          {selected?.next_occurrence_at && <p className="task-muted task-next-run"><CalendarClock size={15} />{displayDate(selected.next_occurrence_at, locale, hourCycle)}</p>}
        </div>
        <div className="task-editor-footer">{error && <p id="task-save-error" role="alert" className="task-error">{error}</p>}<div className="task-editor-actions"><DemoActionButton className="primary-button" type="submit" disabled={busy !== null || uploadingScreenshots || !dirty} aria-describedby={error ? "task-save-error" : undefined}>{text(busy === "save" ? "saving" : "save")}</DemoActionButton>{selected && runButton}{dirty && selected && <button className="task-text-button" type="button" disabled={uploadingScreenshots} onClick={() => patchDraft(inputFromTask(selected))}>{text("cancel")}</button>}</div>
          {selected && <div className="task-editor-secondary">{canRun && <button type="button" className="task-text-button" onClick={() => openRuns(selected)}><CalendarClock size={15} />{text("showResult")}</button>}<DemoActionButton type="button" className="task-text-button task-delete" disabled={busy !== null || uploadingScreenshots} onClick={() => void deleteSelected()}><Trash2 size={14} />{text("delete")}</DemoActionButton></div>}
        </div>
      </div>
      <TeamRail key={selectedId} draft={draft} profiles={profiles} people={[]} canCreateAgent={access.ai} text={text} onChange={patchDraft} onCreate={() => setCreatingAgent(true)} />
    </fieldset>
  </form>;
  return <section className="page ai-settings-page task-workspace">
    {view === "editor" ? <>
      <header className="page-toolbar task-workspace-toolbar"><div><h1>{text("title")}</h1><p>{text("subtitle")}</p></div>
        <div className="task-workspace-toolbar-actions">
          
          {manage && <DemoActionButton className="primary-button" type="button" disabled={busy !== null || uploadingScreenshots} onClick={startNewTask}><Plus size={17} />{text("newTask")}</DemoActionButton>}
        </div>
      </header>
      {loadState === "error" && <div className="task-load-error" role="alert">{text("loadError")}<button type="button" className="task-text-button" onClick={() => { setLoadState("loading"); setLoadVersion((value) => value + 1); }}>{text("retry")}</button></div>}
      {<div className="task-workspace-layout">
        <aside className="task-list-rail" aria-label={text("allTasks")}><div className="task-list-heading"><h2>{text("allTasks")}</h2><Search size={17} /></div><label className="task-search"><span className="sr-only">{text("search")}</span><input type="search" placeholder={text("search")} value={search} onChange={(event) => setSearch(event.target.value)} /></label>
          <div className="task-list-items">{selectedId === "new" && <button className="task-list-row task-list-row--selected" type="button" aria-current="true"><FileText size={20} /><span><strong>{taskTitle(draft.text, text("newTask"))}</strong><span>{text("draft")}</span></span></button>}
            {tasks.filter((task) => task.text.toLocaleLowerCase().includes(search.toLocaleLowerCase())).map((task) => <button className={`task-list-row ${selectedId === task.id ? "task-list-row--selected" : ""}`} key={task.id} type="button" aria-current={selectedId === task.id ? "true" : undefined} disabled={busy !== null} onClick={() => selectTask(task.id)}><FileText size={20} /><span><strong>{taskTitle(task.text, text("newTask"))}</strong><span>{task.execution_mode === "team" ? text("teamShort") : profiles.find((profile) => profile.id === task.agent_ids[0])?.name ?? peopleNames.get(task.assignee_ids?.[0] ?? "") ?? text("agents")} · {text(task.schedule.kind)}{planSummary(task, text) && ` · ${planSummary(task, text)}`}</span></span></button>)}
            {loadState === "loading" && <p className="task-list-message" role="status">{text("loading")}</p>}{loadState === "ready" && tasks.length === 0 && !draft.text.trim() && <p className="task-list-message">{text("noTasks")}</p>}{search && tasks.length > 0 && !tasks.some((task) => task.text.toLocaleLowerCase().includes(search.toLocaleLowerCase())) && <p className="task-list-message">{text("noMatches")}</p>}
          </div>
        </aside>
        {editorForm}
      </div>}
      
    </> : <>
      <header className="task-execution-header"><button type="button" className="task-text-button" onClick={() => void navigateRoute(taskSegments(mode, selectedId))}><ArrowLeft size={15} />{text("back")}</button><div className="task-execution-title"><div><h1>{title}</h1><p className="task-muted">{text(isTeam ? "team" : "independent")}{execution ? ` · ${displayDate(execution.created_at, locale, hourCycle)}` : ""}</p></div><div className="task-execution-actions">{execution && <span className={`task-status task-status--${execution.status}`}><StatusIcon status={execution.status} />{text(execution.status)}</span>}{runButton}{manage && <button type="button" className="secondary-button" onClick={() => void navigateRoute(taskSegments(mode, selectedId))}><Settings2 size={16} />{text("taskSettings")}</button>}{manage && execution && activeStatuses.has(execution.status) && <DemoActionButton className="secondary-button" disabled={busy !== null} onClick={() => void mutateExecution()}><Square size={14} />{text("stop")}</DemoActionButton>}</div></div>
      </header>
      <div className="task-result-tabs" role="tablist" aria-label={text("showResult")}>{(["result", ...(isTeam ? ["steps"] : []), "history"] as ResultTab[]).map((item) => <button type="button" role="tab" aria-selected={tab === item} aria-controls={`task-result-panel-${item}`} id={`task-result-tab-${item}`} className={tab === item ? "task-result-tab--active" : ""} key={item} onClick={() => { if (item === "history") reloadHistory(); void navigateRoute(taskSegments(mode, selectedId, true, item)); }}>{text(item)}</button>)}<button className="task-icon-button task-refresh" aria-label={text("refresh")} onClick={() => reloadHistory({ executionId, occurrence: runOccurrence })}><RefreshCw size={16} /></button></div>
      {(historyError || error) && <p className="task-error task-execution-error" role="alert">{historyError || error}</p>}
      <div ref={resultMotion} className="task-result-panel" role="tabpanel" id={`task-result-panel-${tab}`} aria-labelledby={`task-result-tab-${tab}`}>
        {historyLoading ? <div className="empty-state" role="status">{text("loading")}</div> : isTeam ? tab === "history" ? <div className="task-run-history">{executions.length === 0 && <div className="empty-state">{text("noRuns")}</div>}{executions.map((item) => <button key={item.id} type="button" className="task-history-row" onClick={() => { setExecutionId(item.id); setCopied(false); void navigateRoute(taskSegments(mode, selectedId, true)); }}><span className={`task-status--${item.status}`}><StatusIcon status={item.status} /></span><span><strong>{displayDate(item.created_at, locale, hourCycle)}</strong><span className="task-muted">{text(item.status)} · {item.steps.filter((step) => step.status === "succeeded").length}/{item.steps.length}</span></span><ChevronRight size={18} /></button>)}</div> : !execution ? <div className="empty-state">{text("noRuns")}</div> : tab === "steps" ? <div className="task-steps-view"><ExecutionTimeline execution={execution} text={text} locale={locale} busy={busy !== null || !manage} detailed onRetry={(id) => void mutateExecution(id)} /></div> : <div className="task-result-layout">
          <article className="task-result-document"><h2>{text("result")}</h2>{execution.result_text ? <><MessageBody body={execution.result_text} format="markdown" variant="document" /><button className="secondary-button task-copy-result" onClick={() => void copyResult()}>{copied ? <Check size={16} /> : <Copy size={16} />}{text(copied ? "copied" : "copy")}</button></> : <p className="task-muted">{text("noResult")}</p>}{execution.error && <p className="task-error">{execution.error}</p>}
            {execution.steps.some((step) => step.kind === "work" && step.result_text) && <section className="task-materials"><h3>{text("material")}</h3>{orderedExecutionSteps(execution.steps).filter((step) => step.kind === "work" && step.result_text).map((step) => <AnimatedDetails key={step.id}><summary><FileText size={18} /><span>{step.title}</span></summary><MessageBody body={step.result_text!} format="markdown" variant="document" /></AnimatedDetails>)}</section>}
          </article>
          <aside className="task-execution-rail"><h2>{text("executionSummary")}</h2><ExecutionTimeline execution={execution} text={text} locale={locale} busy={busy !== null || !manage} onRetry={(id) => void mutateExecution(id)} />
            <section className="task-execution-team"><h2>{text("teamComposition")}</h2>{[...new Map(execution.steps.map((step) => [step.agent_id ?? step.assignee_user_id ?? step.agent_name, step])).values()].map((step) => { const profile = profiles.find((item) => item.id === step.agent_id); const participant = step.agent_id ?? step.assignee_user_id; return <div className="task-execution-member" key={participant ?? step.agent_name ?? step.id}><AgentIcon profile={profile} human={Boolean(step.assignee_user_id)} /><span><strong>{step.agent_name ?? profile?.name ?? text("executor")}</strong><span className="task-muted">{text(execution.steps.some((item) => (item.agent_id ?? item.assignee_user_id) === participant && item.kind !== "work") ? "coordinator" : step.assignee_user_id ? "employee" : "executor")}</span></span></div>; })}</section>
          </aside>
        </div> : <div className="task-independent-results">{independentRuns.length === 0 && <div className="empty-state">{text("noRuns")}</div>}{independentRuns.filter((run) => tab === "history" || Date.parse(run.scheduled_for) === shownOccurrence).map((run) => <article key={run.id} className="task-independent-run"><header><h2>{run.agent_name}</h2><span className={`task-status task-status--${run.status}`}><StatusIcon status={run.status} />{text(run.status)}</span></header><p className="task-muted">{displayDate(run.scheduled_for, locale, hourCycle)}</p>{run.output && <MessageBody body={run.output} format="markdown" variant="document" />}{run.error && <p className="task-error">{run.error}</p>}<AnimatedDetails><summary>{text("input")}</summary><p className="task-plain-text">{run.task_text}</p></AnimatedDetails></article>)}</div>}
      </div>
    </>}
    {creatingAgent && <CreateAgentDialog auth={auth} providers={providers} text={text} locale={locale} onClose={() => setCreatingAgent(false)} onCreated={(profile) => { setProfiles((current) => [...current, agentOption(profile, providers)]); patchDraft({ agent_ids: [...draft.agent_ids, profile.id], agent_roles: [...draft.agent_roles, { agent_id: profile.id, role: "" }] }); setCreatingAgent(false); }} />}
  </section>;
}
