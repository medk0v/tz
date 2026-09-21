import { managementApiRequest, type OperatorAuth } from "./api";
import type { TeamExecution } from "./task-orchestration-api";

export type TaskColumnKind = "todo" | "in_progress" | "done";
export interface TaskColumn {
  id: string;
  list_id: string;
  /** Null until a preset status is renamed; the interface names presets. */
  name: string | null;
  preset_key: "backlog" | "in_progress" | "done" | null;
  emoji: string | null;
  color: string;
  kind: TaskColumnKind;
  position: number;
}
export interface TaskList {
  id: string;
  name: string | null;
  emoji: string | null;
  color: string | null;
  is_default: boolean;
  position: number;
  columns: TaskColumn[];
}
export interface TaskTag { id: string; name: string; color: string; parent_tag_id?: string | null }
export interface TaskPerson { id: string; name: string }
export interface TaskAgentOption { id: string; name: string; preset_key: string | null; ready: boolean }
export interface TaskBoard {
  permissions: { manage: boolean; configure: boolean };
  current_user_id: string;
  lists: TaskList[];
  tags: TaskTag[];
  people: TaskPerson[];
  agents: TaskAgentOption[];
}
export interface TaskMoveResult {
  list_id: string;
  column_id: string;
  completed_at: string | null;
  positions: Array<{ id: string; position: number }>;
}
export interface WaitingStepMember { id: string; name: string; role: string; kind: "agent" | "employee" }
export interface WaitingStepResult { step_key: string; title: string; participant: string; output: string }
export interface WaitingStep {
  id: string;
  execution_id: string;
  task_id: string;
  task_text: string;
  expected_result: string;
  kind: "planning" | "work" | "review";
  title: string;
  assignment: string;
  original_assignment: string | null;
  started_at: string | null;
  revision_round_available: boolean;
  members: WaitingStepMember[];
  results: WaitingStepResult[];
}
export interface PlannedStepInput { key: string; title: string; agent_id: string; instructions: string; depends_on: string[] }
export type StepSubmission =
  | { status?: "completed" | "blocked"; output: string }
  | { steps: PlannedStepInput[] }
  | { status: "complete" | "needs_revision" | "incomplete"; result: string; reason?: string; revisions?: Array<{ step_key: string; instructions: string }> };

export interface ListInput { name: string; emoji?: string | null; color?: string | null }
export interface ColumnInput { name: string; emoji?: string | null; color: string; kind: TaskColumnKind }
export interface TagInput { name: string; color: string; parent_tag_id?: string | null }

const json = (method: string, body: unknown): RequestInit => ({ method, body: JSON.stringify(body) });

export async function getTaskBoard(auth: OperatorAuth): Promise<TaskBoard> {
  const board = await managementApiRequest<Partial<TaskBoard> | null>(auth, "/api/v1/task-board");
  if (!board || !Array.isArray(board.lists)) throw new Error("The task board response is invalid.");
  return {
    permissions: board.permissions ?? { manage: false, configure: false }, current_user_id: board.current_user_id ?? "",
    lists: board.lists, tags: board.tags ?? [], people: board.people ?? [], agents: board.agents ?? [],
  };
}
export function createTaskList(auth: OperatorAuth, input: ListInput): Promise<TaskList> {
  return managementApiRequest(auth, "/api/v1/task-lists", json("POST", input));
}
export function updateTaskList(auth: OperatorAuth, id: string, input: Partial<ListInput>): Promise<TaskList> {
  return managementApiRequest(auth, `/api/v1/task-lists/${encodeURIComponent(id)}`, json("PATCH", input));
}
export function deleteTaskList(auth: OperatorAuth, id: string): Promise<void> {
  return managementApiRequest(auth, `/api/v1/task-lists/${encodeURIComponent(id)}`, { method: "DELETE" });
}
export function createTaskColumn(auth: OperatorAuth, listId: string, input: ColumnInput): Promise<TaskList> {
  return managementApiRequest(auth, `/api/v1/task-lists/${encodeURIComponent(listId)}/columns`, json("POST", input));
}
export function updateTaskColumn(auth: OperatorAuth, id: string, input: Partial<ColumnInput>): Promise<TaskList> {
  return managementApiRequest(auth, `/api/v1/task-columns/${encodeURIComponent(id)}`, json("PATCH", input));
}
export function deleteTaskColumn(auth: OperatorAuth, id: string): Promise<TaskList> {
  return managementApiRequest(auth, `/api/v1/task-columns/${encodeURIComponent(id)}`, { method: "DELETE" });
}
export function orderTaskColumns(auth: OperatorAuth, listId: string, columnIds: string[]): Promise<TaskList> {
  return managementApiRequest(auth, `/api/v1/task-lists/${encodeURIComponent(listId)}/column-order`, json("PUT", { column_ids: columnIds }));
}
export function createTaskTag(auth: OperatorAuth, input: TagInput): Promise<TaskTag> {
  return managementApiRequest(auth, "/api/v1/task-tags", json("POST", input));
}
export function updateTaskTag(auth: OperatorAuth, id: string, input: Partial<TagInput>): Promise<TaskTag> {
  return managementApiRequest(auth, `/api/v1/task-tags/${encodeURIComponent(id)}`, json("PATCH", input));
}
export function deleteTaskTag(auth: OperatorAuth, id: string): Promise<void> {
  return managementApiRequest(auth, `/api/v1/task-tags/${encodeURIComponent(id)}`, { method: "DELETE" });
}
export function moveTask(auth: OperatorAuth, taskId: string, columnId: string, index: number): Promise<TaskMoveResult> {
  return managementApiRequest(auth, `/api/v1/ai/tasks/${encodeURIComponent(taskId)}/move`, json("POST", { column_id: columnId, index }));
}
export async function listMySteps(auth: OperatorAuth): Promise<WaitingStep[]> {
  const result = await managementApiRequest<{ items?: WaitingStep[] } | null>(auth, "/api/v1/ai/task-steps/mine");
  return Array.isArray(result?.items) ? result.items : [];
}
export function submitStep(auth: OperatorAuth, stepId: string, submission: StepSubmission): Promise<TeamExecution> {
  return managementApiRequest(auth, `/api/v1/ai/task-steps/${encodeURIComponent(stepId)}/submit`, json("POST", submission));
}
