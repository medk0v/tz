import type { ResourceVisibility } from "./resource-visibility";
import { managementApiRequest, type AiTaskSchedule, type AiTaskRun, type OperatorAuth } from "./api";

export type TaskSchedule = AiTaskSchedule | { kind: "manual" };
export interface TaskAgentRole { agent_id: string; role: string }
/** AI profile id for an agent, user id for an employee. */
export interface TaskPlanPerformer { kind: "agent" | "employee"; id: string }
export interface TaskPlanStep { id: string; title: string; instructions: string; performer: TaskPlanPerformer }
/** The approved process version a task was launched from; immutable. */
export interface TaskProcess { id: string; version: number; title: string }
export interface TaskPlanProgress { done: number; total: number }
export interface TaskInput {
  visibility?: ResourceVisibility;
  text: string;
  schedule: TaskSchedule;
  agent_ids: string[];
  execution_mode: "independent" | "team";
  coordinator_id: string | null;
  expected_result: string;
  agent_roles: TaskAgentRole[];
  /** Employees; omitted to keep the current assignment. */
  assignee_ids?: string[];
  coordinator_user_id?: string | null;
  list_id?: string | null;
  column_id?: string | null;
  /** Start of the span whose end is due_at. Omitted to keep it, null clears it. */
  starts_at?: string | null;
  /** Deadline, and the end of the span that starts_at opens. */
  due_at?: string | null;
  /** The span covers whole days in time_zone rather than a time of day. */
  all_day?: boolean;
  /** IANA zone the span was entered in, which its instants cannot recover. */
  time_zone?: string | null;
  /** Minutes before the span opens to remind everyone working on the task. */
  reminder_minutes?: number | null;
  tag_ids?: string[];
  attachment_ids?: string[];
  /** Predefined plan; omitted to keep the stored one, null clears it. */
  plan_steps?: TaskPlanStep[] | null;
}
export type TaskLatestStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled" | "needs_attention";
export interface Task extends TaskInput {
  id: string;
  /** Owning project, including tasks shared with other projects. */
  project_id?: string;
  next_occurrence_at: string | null;
  created_at: string;
  updated_at: string;
  waiting_user_ids?: string[];
  position?: number;
  completed_at?: string | null;
  latest_status?: TaskLatestStatus | null;
  latest_at?: string | null;
  process?: TaskProcess | null;
  plan_progress?: TaskPlanProgress | null;
}
export type ExecutionStatus = "queued" | "planning" | "running" | "reviewing" | "succeeded" | "failed" | "cancelled" | "needs_attention";
export interface ExecutionStep {
  id: string;
  title: string;
  agent_id: string | null;
  /** Set for a step performed by an employee. */
  assignee_user_id?: string | null;
  agent_name?: string;
  kind: "planning" | "work" | "review";
  status: "pending" | "running" | "succeeded" | "failed" | "cancelled" | "blocked";
  depends_on: string[];
  input_text?: string;
  result_text: string | null;
  error: string | null;
  started_at: string | null;
  finished_at: string | null;
  attempts: number;
  may_have_effects: boolean;
}
export interface TeamExecution {
  id: string;
  task_id: string;
  status: ExecutionStatus;
  result_text: string | null;
  error: string | null;
  created_at: string;
  updated_at: string;
  started_at: string | null;
  finished_at: string | null;
  steps: ExecutionStep[];
  events?: Array<{ id: string; event_type: string; message: string; created_at: string }>;
}

export type TaskCalendarStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled" | "needs_attention";
/** A task occurrence that already started; independent agent runs are merged. */
export interface TaskCalendarEntry {
  task_id: string;
  execution_id: string | null;
  starts_at: string;
  status: TaskCalendarStatus;
}
export interface TaskCalendarHistory { items: TaskCalendarEntry[]; truncated: boolean }

const taskPath = (id: string) => `/api/v1/ai/tasks/${encodeURIComponent(id)}`;
const executionPath = (id: string) => `/api/v1/ai/task-executions/${encodeURIComponent(id)}`;

export async function listTasks(auth: OperatorAuth): Promise<Task[]> {
  const result = await managementApiRequest<{ items: Task[] }>(auth, "/api/v1/ai/tasks");
  return result.items;
}
export function listTaskCalendar(auth: OperatorAuth, from: Date, to: Date): Promise<TaskCalendarHistory> {
  const query = new URLSearchParams({ from: from.toISOString(), to: to.toISOString() });
  return managementApiRequest(auth, `/api/v1/ai/task-calendar?${query}`);
}
export function saveTask(auth: OperatorAuth, id: string | null, input: TaskInput): Promise<Task> {
  return managementApiRequest(auth, id ? taskPath(id) : "/api/v1/ai/tasks", {
    method: id ? "PATCH" : "POST", body: JSON.stringify(input),
  });
}
export function uploadTaskScreenshot(auth: OperatorAuth, file: File, signal?: AbortSignal): Promise<{ id: string }> {
  const body = new FormData();
  body.append("file", file, file.name);
  return managementApiRequest(auth, "/api/v1/ai/task-attachments", { method: "POST", body, signal });
}
export function removeTask(auth: OperatorAuth, id: string): Promise<void> {
  return managementApiRequest(auth, taskPath(id), { method: "DELETE" });
}
export async function listExecutions(auth: OperatorAuth, taskId: string): Promise<TeamExecution[]> {
  const result = await managementApiRequest<{ items: TeamExecution[] }>(auth, `${taskPath(taskId)}/executions`);
  return result.items;
}
export function getExecution(auth: OperatorAuth, id: string): Promise<TeamExecution> {
  return managementApiRequest(auth, executionPath(id));
}
export function startExecution(auth: OperatorAuth, taskId: string, idempotencyKey: string): Promise<TeamExecution> {
  return managementApiRequest(auth, `${taskPath(taskId)}/executions`, {
    method: "POST", headers: { "Idempotency-Key": idempotencyKey },
  });
}
export function cancelExecution(auth: OperatorAuth, id: string): Promise<TeamExecution> {
  return managementApiRequest(auth, `${executionPath(id)}/cancel`, { method: "POST" });
}
export function retryExecutionStep(auth: OperatorAuth, id: string, stepId: string): Promise<TeamExecution> {
  return managementApiRequest(auth, `${executionPath(id)}/retry`, {
    method: "POST", body: JSON.stringify({ step_id: stepId }),
  });
}
export async function startIndependentRuns(auth: OperatorAuth, taskId: string, idempotencyKey: string): Promise<AiTaskRun[]> {
  const result = await managementApiRequest<{ items: AiTaskRun[] }>(auth, `${taskPath(taskId)}/runs`, {
    method: "POST", headers: { "Idempotency-Key": idempotencyKey },
  });
  return result.items;
}
