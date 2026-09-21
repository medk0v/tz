import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import * as tasksApi from "../task-orchestration-api";
import * as boardApi from "../task-board-api";
import { I18nContext, createI18n } from "../i18n";
import { DemoReadOnlyContext } from "./DemoReadOnly";
import { AITasksWorkspace } from "./AITasksWorkspace";
import { AITasksView } from "./AISettingsView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";
import { changeDateTimeField, dateTimeFieldInput } from "./date-time-field-test-utils";

vi.mock("../api", () => ({ downloadTaskScreenshot: vi.fn(), managementApiRequest: vi.fn(), listAiProfiles: vi.fn(), listAiProviders: vi.fn(), listAiTaskRuns: vi.fn(), createAiProfile: vi.fn() }));
vi.mock("../task-orchestration-api", () => ({ uploadTaskScreenshot: vi.fn(), listTasks: vi.fn(), saveTask: vi.fn(), removeTask: vi.fn(), listExecutions: vi.fn(), getExecution: vi.fn(), startExecution: vi.fn(), cancelExecution: vi.fn(), retryExecutionStep: vi.fn(), startIndependentRuns: vi.fn() }));
vi.mock("../task-board-api", () => ({ getTaskBoard: vi.fn(), listMySteps: vi.fn(), moveTask: vi.fn(), submitStep: vi.fn(), createTaskList: vi.fn(), updateTaskList: vi.fn(), deleteTaskList: vi.fn(), createTaskColumn: vi.fn(), updateTaskColumn: vi.fn(), deleteTaskColumn: vi.fn(), orderTaskColumns: vi.fn(), createTaskTag: vi.fn(), updateTaskTag: vi.fn(), deleteTaskTag: vi.fn() }));

const board: boardApi.TaskBoard = {
  permissions: { manage: true, configure: true }, current_user_id: "me", tags: [], people: [], agents: [],
  lists: [{ id: "inbox", name: null, emoji: null, color: null, is_default: true, position: 0, columns: [
    { id: "backlog", list_id: "inbox", name: null, preset_key: "backlog", emoji: null, color: "#8a94a6", kind: "todo", position: 0 },
    { id: "doing", list_id: "inbox", name: null, preset_key: "in_progress", emoji: null, color: "#3b82f6", kind: "in_progress", position: 1 },
    { id: "done", list_id: "inbox", name: null, preset_key: "done", emoji: null, color: "#22a06b", kind: "done", position: 2 },
  ] }],
};
const auth: api.OperatorAuth = { kind: "session" };
const provider: api.AiProvider = { id: "provider", name: "Local model", provider_kind: "openai_compatible", base_url: "https://model.example/v1", status: "active", default_model: "local-model", api_key_configured: true, created_at: "2026-09-10T10:00:00Z", updated_at: "2026-09-10T10:00:00Z" };
const coordinator: api.AiProfile = {
  id: "coordinator", name: "Coordinator", provider_connection_id: provider.id, avatar_url: null, status: "active", mode: "copilot", model: null,
  instructions: "Coordinate work.", tool_instructions: "", blacklist_reply_text: "", blacklist_reply_match_language: true, http_allowed_hosts: [], language: "en", max_output_tokens: 4000,
  auto_join_new_conversations: false, can_resolve_conversations: false, capabilities: { http_get: false, http_post: false, shell: false },
  telegram_notifications: { new_visitor: false, new_message: false, operator_request: false }, custom_fields: [], secrets: [], knowledge_base_ids: [], channel_ids: [], public_identities: [],
  created_at: "2026-09-10T10:00:00Z", updated_at: "2026-09-10T10:00:00Z",
};
const researcher: api.AiProfile = { ...coordinator, id: "researcher", name: "Researcher" };
const editor: api.AiProfile = { ...coordinator, id: "editor", name: "Editor" };
const task: tasksApi.Task = {
  id: "task-1", text: "Prepare a launch plan", schedule: { kind: "manual" }, agent_ids: [researcher.id],
  execution_mode: "team", coordinator_id: coordinator.id, expected_result: "A sourced launch plan", agent_roles: [{ agent_id: researcher.id, role: "Find requirements" }],
  next_occurrence_at: null, created_at: "2026-09-10T10:00:00Z", updated_at: "2026-09-10T10:00:00Z",
};
const independentTask: tasksApi.Task = { ...task, id: "task-2", text: "Independent summary", execution_mode: "independent", coordinator_id: null, expected_result: "", agent_roles: [] };
const step: tasksApi.ExecutionStep = {
  id: "research-step", title: "Find requirements", agent_id: researcher.id, agent_name: researcher.name,
  kind: "work", status: "succeeded", depends_on: [], input_text: "Find launch requirements.", result_text: "Requirements collected.", error: null,
  started_at: "2026-09-10T10:01:00Z", finished_at: "2026-09-10T10:02:00Z", attempts: 1, may_have_effects: false,
};
const execution: tasksApi.TeamExecution = {
  id: "execution-1", task_id: task.id, status: "succeeded", result_text: "**Launch plan**\n\n[Unsafe](javascript:alert(1))\n\n<script>alert(1)</script>\n\nReady for review.", error: null,
  created_at: "2026-09-10T10:00:00Z", updated_at: "2026-09-10T10:03:00Z", started_at: "2026-09-10T10:00:00Z", finished_at: "2026-09-10T10:03:00Z", steps: [step],
};

function mount(readOnly = false) {
  return render(<I18nContext.Provider value={createI18n("en", () => {})}><DemoReadOnlyContext.Provider value={readOnly}><AITasksWorkspace auth={readOnly ? { kind: "session", isDemo: true } : auth} inboxes={[]} /></DemoReadOnlyContext.Provider></I18nContext.Provider>);
}
function mountAt(segments: string[]) {
  return render(<I18nContext.Provider value={createI18n("en", () => {})}><MemoryPageRoute initialSegments={segments}><AITasksWorkspace auth={auth} inboxes={[]} /><CurrentPageRoute /></MemoryPageRoute></I18nContext.Provider>);
}
const pageRoute = () => screen.getByTestId("page-route");
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(boardApi.getTaskBoard).mockResolvedValue(board);
  vi.mocked(boardApi.listMySteps).mockResolvedValue([]);
  localStorage.clear();
    vi.mocked(api.managementApiRequest).mockResolvedValue({ projects: [], departments: [] });
  vi.mocked(tasksApi.listTasks).mockResolvedValue([task, independentTask]);
  vi.mocked(api.listAiProfiles).mockResolvedValue([coordinator, researcher, editor]);
  vi.mocked(api.listAiProviders).mockResolvedValue([provider]);
  vi.mocked(api.listAiTaskRuns).mockResolvedValue([]);
  vi.mocked(tasksApi.listExecutions).mockResolvedValue([execution]);
  vi.mocked(tasksApi.getExecution).mockResolvedValue(execution);
  vi.mocked(tasksApi.saveTask).mockImplementation(async (_auth, id, input) => ({ ...task, ...input, id: id ?? "task-new" }));
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("AITasksWorkspace", () => {
  it("waits for a screenshot before saving and keeps it when editing the saved task", async () => {
    let complete!: (value: { id: string }) => void;
    vi.mocked(tasksApi.uploadTaskScreenshot).mockReturnValue(new Promise((resolve) => { complete = resolve; }));
    vi.mocked(api.downloadTaskScreenshot).mockRejectedValue(new Error("Preview unavailable"));
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "New task" }));
    fireEvent.change(screen.getByLabelText("Task description"), { target: { value: "Fix this screen" } });
    fireEvent.change(screen.getByLabelText("Add files"), { target: { files: [new File(["image"], "screen.png", { type: "image/png" })] } });
    expect(screen.getByRole("button", { name: "Save task" })).toBeDisabled();
    await act(async () => { complete({ id: "screenshot" }); });
    fireEvent.click(screen.getByRole("button", { name: "Add agent" }));
    fireEvent.click(screen.getByRole("button", { name: /Researcher.*Custom agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, null, expect.objectContaining({ attachment_ids: ["screenshot"] })));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save task" })).toBeDisabled());
    fireEvent.change(screen.getByLabelText("Task description"), { target: { value: "Fix this screen today" } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenLastCalledWith(auth, "task-new", expect.objectContaining({ attachment_ids: ["screenshot"] })));
  });

  it.each(["2000-01-01T09:00", "2099-09-12T16:45"])("creates a one-time task with an ISO timestamp for %s", async (local) => {
    render(<I18nContext.Provider value={createI18n("en", () => {})}><AITasksView auth={auth} inboxes={[]} /></I18nContext.Provider>);
    await screen.findByDisplayValue(task.text);
    expect(screen.getByRole("heading", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "New task" }));
    fireEvent.change(screen.getByLabelText("Task description"), { target: { value: "Send the quarterly report" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: "once" } });
    const date = new Date(local);
    const runAt = screen.getByLabelText("Date and time");
    expect(runAt).toHaveTextContent("Select date and time");
    expect(dateTimeFieldInput(runAt)).toBeRequired();
    expect(dateTimeFieldInput(runAt)).toBeInvalid();
    changeDateTimeField(runAt, local);
    expect(runAt).toBeValid();
    expect(dateTimeFieldInput(runAt)).toHaveValue(local);
    // English follows a 12-hour clock unless the user's time format says otherwise.
    expect(runAt).toHaveTextContent(`${new Intl.DateTimeFormat("en", { month: "short" }).format(date)} ${date.getDate()}, ${date.getFullYear()}, ${new Intl.DateTimeFormat("en", { hour: "numeric", minute: "2-digit" }).format(date).replace(/\s/g, " ")}`);
    fireEvent.click(screen.getByRole("button", { name: "Add agent" }));
    fireEvent.click(screen.getByRole("button", { name: /Researcher.*Custom agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, null, expect.objectContaining({ text: "Send the quarterly report", execution_mode: "independent", agent_ids: [researcher.id], schedule: { kind: "once", run_at: new Date(local).toISOString() } })));
    await waitFor(() => expect(screen.getByRole("button", { name: /Send the quarterly report.*Researcher/ })).toBeInTheDocument());
    expect(tasksApi.listTasks).toHaveBeenCalledOnce();
  });

  it("creates a recurring task with a deadline in its selected time zone", async () => {
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "New task" }));
    fireEvent.change(screen.getByLabelText("Task description"), { target: { value: "Send daily reports until the deadline" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: "daily" } });
    fireEvent.change(screen.getByLabelText("Time zone"), { target: { value: "Pacific/Honolulu" } });
    const deadline = screen.getByLabelText("Do not run after");
    expect(dateTimeFieldInput(deadline)).not.toBeRequired();
    expect(deadline).not.toHaveAttribute("aria-required");
    expect(deadline).toHaveAccessibleDescription("Leave empty for no end date. Time zone: Pacific/Honolulu Select date and time");
    changeDateTimeField(deadline, "2099-09-12T16:45");
    fireEvent.change(screen.getByLabelText("Time zone"), { target: { value: "Europe/Istanbul" } });
    expect(dateTimeFieldInput(deadline)).toHaveValue("2099-09-12T16:45:00");
    expect(deadline).toHaveAccessibleDescription(/^Leave empty for no end date\. Time zone: Europe\/Istanbul Sep 12, 2099, 4:45\sPM$/);
    fireEvent.click(screen.getByRole("button", { name: "Add agent" }));
    fireEvent.click(screen.getByRole("button", { name: /Researcher.*Custom agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, null, expect.objectContaining({
      schedule: { kind: "daily", time: "09:00", timezone: "Europe/Istanbul", ends_at: "2099-09-12T16:45:00" },
    })));
  });

  it("loads, edits and clears an existing recurring deadline without converting its time zone", async () => {
    vi.mocked(tasksApi.listTasks).mockResolvedValue([{ ...task, schedule: { kind: "daily", time: "09:00", timezone: "Pacific/Honolulu", ends_at: "2099-09-12T16:45:30" } }]);
    mount();
    await screen.findByDisplayValue(task.text);
    const deadline = screen.getByLabelText("Do not run after");
    expect(dateTimeFieldInput(deadline)).toHaveValue("2099-09-12T16:45:30");
    changeDateTimeField(deadline, "2099-09-13T18:30");
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenLastCalledWith(auth, task.id, expect.objectContaining({
      schedule: { kind: "daily", time: "09:00", timezone: "Pacific/Honolulu", ends_at: "2099-09-13T18:30:00" },
    })));
    await waitFor(() => expect(screen.getByLabelText("Do not run after")).toBeEnabled());
    changeDateTimeField(screen.getByLabelText("Do not run after"), "");
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenLastCalledWith(auth, task.id, expect.objectContaining({
      schedule: { kind: "daily", time: "09:00", timezone: "Pacific/Honolulu", ends_at: null },
    })));
    expect(dateTimeFieldInput(screen.getByLabelText("Do not run after"))).toHaveValue("");
  });

  it("preserves the deadline when switching between all recurring schedules", async () => {
    vi.mocked(tasksApi.listTasks).mockResolvedValue([{ ...task, schedule: { kind: "daily", time: "09:00", timezone: "Europe/Istanbul", ends_at: "2099-09-12T16:45:00" } }]);
    mount();
    await screen.findByDisplayValue(task.text);
    for (const kind of ["weekly", "monthly", "yearly", "daily"]) {
      fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: kind } });
      expect(dateTimeFieldInput(screen.getByLabelText("Do not run after"))).toHaveValue("2099-09-12T16:45:00");
    }
    fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: "weekly" } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, task.id, expect.objectContaining({
      schedule: { kind: "weekly", time: "09:00", timezone: "Europe/Istanbul", weekdays: [1], ends_at: "2099-09-12T16:45:00" },
    })));
  });

  it.each(["manual", "once"] as const)("removes the recurring deadline when switching to %s", async (kind) => {
    vi.mocked(tasksApi.listTasks).mockResolvedValue([{ ...task, schedule: { kind: "daily", time: "09:00", timezone: "Europe/Istanbul", ends_at: "2099-09-12T16:45:00" } }]);
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: kind } });
    expect(screen.queryByLabelText("Do not run after")).not.toBeInTheDocument();
    if (kind === "once") changeDateTimeField(screen.getByLabelText("Date and time"), "2099-09-12T16:45");
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, task.id, expect.objectContaining({
      schedule: kind === "manual" ? { kind } : { kind, run_at: new Date("2099-09-12T16:45").toISOString() },
    })));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Run" })).toBeEnabled());
    fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: "daily" } });
    expect(dateTimeFieldInput(screen.getByLabelText("Do not run after"))).toHaveValue("");
  });

  it("limits teams to eight participants and independent tasks to 32 agents", async () => {
    const members = Array.from({ length: 33 }, (_, index) => ({ ...researcher, id: `member-${index}`, name: `Member ${index}` }));
    const fullTask = { ...task, execution_mode: "independent" as const, coordinator_id: null, agent_ids: members.slice(0, 32).map((profile) => profile.id), agent_roles: [] };
    vi.mocked(api.listAiProfiles).mockResolvedValue([coordinator, ...members]);
    vi.mocked(tasksApi.listTasks).mockResolvedValue([fullTask]);
    mount();
    await screen.findByDisplayValue(task.text);
    expect(screen.getByRole("button", { name: "Add agent" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Remove agent: Member 0" }));
    expect(screen.getByRole("button", { name: "Add agent" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Team with coordinator" }));
    fireEvent.change(screen.getByRole("combobox", { name: "Coordinator" }), { target: { value: coordinator.id } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Select a coordinator and 1–8 other participants.");
    expect(tasksApi.saveTask).not.toHaveBeenCalled();
  });

  it("shows loading, empty and error history states and ignores a response after leaving the task", async () => {
    let finish!: (items: tasksApi.TeamExecution[]) => void;
    vi.mocked(tasksApi.listExecutions).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    expect(screen.getByRole("status")).toHaveTextContent("Loading…");
    await act(async () => finish([]));
    expect(screen.getByText("This task has not run yet.")).toBeInTheDocument();
    vi.mocked(tasksApi.listExecutions).mockRejectedValueOnce(new Error("Run history unavailable"));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Run history unavailable");
    vi.mocked(tasksApi.listExecutions).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    fireEvent.click(screen.getByRole("button", { name: "Back to tasks" }));
    fireEvent.click(screen.getByRole("button", { name: /Independent summary.*Researcher/ }));
    await act(async () => finish([execution]));
    expect(screen.getByLabelText("Task description")).toHaveValue(independentTask.text);
    expect(screen.queryByText("Ready for review.")).not.toBeInTheDocument();
  });

  it("deletes the selected task after confirmation and keeps the next task available", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    vi.mocked(tasksApi.removeTask).mockResolvedValue(undefined);
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "Delete task" }));
    await waitFor(() => expect(tasksApi.removeTask).toHaveBeenCalledWith(auth, task.id));
    await waitFor(() => expect(screen.getByLabelText("Task description")).toHaveValue(independentTask.text));
    expect(screen.queryByRole("button", { name: /Prepare a launch plan.*Team/ })).not.toBeInTheDocument();
    confirm.mockRestore();
  });
  it("edits team assignments and keeps the entire draft with an exact save error", async () => {
    vi.mocked(tasksApi.saveTask).mockRejectedValueOnce(new Error("The selected coordinator has been disabled."));
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.change(screen.getByLabelText("Task description"), { target: { value: "Revised launch plan" } });
    fireEvent.change(screen.getByLabelText("Expected result"), { target: { value: "A reviewed plan with sources" } });
    fireEvent.change(screen.getByLabelText("Assignment in this team: Researcher"), { target: { value: "Check requirements and sources" } });
    fireEvent.click(screen.getByRole("button", { name: "Add agent" }));
    fireEvent.click(screen.getByRole("button", { name: /Editor.*Custom agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The selected coordinator has been disabled.");
    expect(screen.getByLabelText("Task description")).toHaveValue("Revised launch plan");
    expect(screen.getByLabelText("Expected result")).toHaveValue("A reviewed plan with sources");
    expect(screen.getByLabelText("Assignment in this team: Researcher")).toHaveValue("Check requirements and sources");
    expect(screen.getByRole("button", { name: "Save task" })).toHaveAttribute("aria-describedby", "task-save-error");
    expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, task.id, expect.objectContaining({
      execution_mode: "team", coordinator_id: coordinator.id, agent_ids: [researcher.id, editor.id],
      expected_result: "A reviewed plan with sources", agent_roles: [{ agent_id: researcher.id, role: "Check requirements and sources" }, { agent_id: editor.id, role: "" }],
    }));
  });

  it("preserves a task draft while creating and adding a real agent", async () => {
    const created = { ...editor, id: "custom-agent", name: "Launch editor" };
    vi.mocked(api.createAiProfile).mockResolvedValue(created);
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.change(screen.getByLabelText("Task description"), { target: { value: "Keep this task draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Add agent" }));
    fireEvent.click(screen.getByRole("button", { name: "Create your own agent" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Agent name"), { target: { value: "Launch editor" } });
    fireEvent.change(within(dialog).getByLabelText("Provider"), { target: { value: provider.id } });
    fireEvent.change(within(dialog).getByLabelText("Instructions"), { target: { value: "Edit launch plans and identify gaps." } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Create agent" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByLabelText("Task description")).toHaveValue("Keep this task draft");
    expect(screen.getByLabelText("Assignment in this team: Launch editor")).toBeInTheDocument();
    expect(api.createAiProfile).toHaveBeenCalledWith(auth, expect.objectContaining({
      name: "Launch editor", provider_connection_id: provider.id, instructions: "Edit launch plans and identify gaps.",
      auto_join_new_conversations: false, capabilities: { http_get: false, http_post: false, shell: false },
    }));
  });

  it("creates a draft team member before a project has any model connection", async () => {
    vi.mocked(api.listAiProviders).mockResolvedValue([]);
    vi.mocked(api.createAiProfile).mockResolvedValue({ ...editor, id: "draft-editor", name: "Draft editor", status: "draft", provider_connection_id: null });
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "Add agent" }));
    fireEvent.click(screen.getByRole("button", { name: "Create your own agent" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByLabelText("Agent name"), { target: { value: "Draft editor" } });
    fireEvent.change(within(dialog).getByLabelText("Instructions"), { target: { value: "Prepare the final report." } });
    expect(within(dialog).getByLabelText("Model")).toBeDisabled();
    expect(within(dialog).getByText(/saved as a draft/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Create agent" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(api.createAiProfile).toHaveBeenCalledWith(auth, expect.objectContaining({
      name: "Draft editor", status: "draft", provider_connection_id: null, model: null,
      telegram_notifications: { new_visitor: false, new_message: false, operator_request: false },
      public_identities: [{ language: "en", display_name: "Draft editor" }],
    }));
    expect(screen.getByLabelText("Assignment in this team: Draft editor")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, task.id, expect.objectContaining({ agent_ids: [researcher.id, "draft-editor"], schedule: { kind: "manual" } })));
  });

  it("retains per-task drafts when switching tasks and rejects an incomplete team", async () => {
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.change(screen.getByLabelText("Expected result"), { target: { value: "Unfinished editing" } });
    fireEvent.click(screen.getByRole("button", { name: /Independent summary.*Researcher.*On demand/ }));
    expect(screen.getByLabelText("Task description")).toHaveValue(independentTask.text);
    fireEvent.click(screen.getByRole("button", { name: /Prepare a launch plan.*Team.*On demand/ }));
    expect(screen.getByLabelText("Expected result")).toHaveValue("Unfinished editing");
    fireEvent.click(screen.getByRole("button", { name: "Remove agent: Researcher" }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Select a coordinator and 1–8 other participants.");
    expect(tasksApi.saveTask).not.toHaveBeenCalled();
  });

  it("saves on-demand team drafts with preset agents that still need a provider, but requires configuration for a schedule", async () => {
    vi.mocked(api.listAiProfiles).mockResolvedValue([{ ...coordinator, status: "draft", provider_connection_id: null }, { ...researcher, status: "draft", provider_connection_id: null }]);
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.change(screen.getByLabelText("Expected result"), { target: { value: "Draft result" } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.getByRole("button", { name: "Run now" })).toBeEnabled());
    fireEvent.change(screen.getByLabelText("Run", { selector: "select" }), { target: { value: "weekly" } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Configure the selected agents in AI");
    expect(tasksApi.saveTask).toHaveBeenCalledOnce();
  });

  it("opens real run output and step context without executing model HTML or unsafe links", async () => {
    vi.mocked(tasksApi.listExecutions).mockResolvedValue([{ ...execution, result_text: `# Launch plan\n\n## Preparation\n\n${execution.result_text}`, steps: [{ ...step, result_text: "## Collected evidence\n\nRequirements collected." }] }]);
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    await screen.findByText("Ready for review.");
    expect(screen.getByRole("heading", { level: 1, name: "Launch plan" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "Preparation" })).toBeInTheDocument();
    expect(tasksApi.listExecutions).toHaveBeenCalledWith(auth, task.id);
    expect(document.querySelector("script")).toBeNull();
    expect(document.querySelector('a[href^="javascript:"]')).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Execution steps" }));
    expect(screen.getByRole("heading", { level: 2, name: "Collected evidence" })).toBeInTheDocument();
    expect(screen.getByText("Requirements collected.")).toBeInTheDocument();
    expect(screen.getByText("Find launch requirements.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Run history" }));
    await waitFor(() => expect(screen.getByRole("tabpanel")).toHaveTextContent("Completed · 1/1"));
  });

  it("orders the timeline by dependencies and start times while retaining corrections and failed steps", async () => {
    const planning: tasksApi.ExecutionStep = { ...step, id: "planning", kind: "planning", started_at: "2026-09-10T10:00:00Z" };
    const early: tasksApi.ExecutionStep = { ...step, id: "early", title: "Initial research", depends_on: [planning.id], started_at: "2026-09-10T10:01:00Z" };
    const late: tasksApi.ExecutionStep = { ...step, id: "late", title: "Risk analysis", depends_on: [planning.id], started_at: "2026-09-10T10:02:00Z" };
    const review: tasksApi.ExecutionStep = { ...step, id: "review", kind: "review", depends_on: [early.id, late.id], started_at: "2026-09-10T10:03:00Z" };
    const correction: tasksApi.ExecutionStep = { ...step, id: "correction", title: "Correct the evidence", status: "failed", error: "Source unavailable", depends_on: [early.id, review.id], started_at: "2026-09-10T10:04:00Z" };
    const blocked: tasksApi.ExecutionStep = { ...step, id: "blocked", title: "Update the analysis", status: "blocked", depends_on: [correction.id], started_at: null, finished_at: null };
    const final: tasksApi.ExecutionStep = { ...step, id: "final-review", kind: "review", status: "pending", depends_on: [blocked.id], started_at: null, finished_at: null };
    const unordered = [review, late, final, blocked, correction, early, planning];
    vi.mocked(tasksApi.listExecutions).mockResolvedValue([{ ...execution, status: "needs_attention", steps: unordered }]);
    const { container } = mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    await screen.findByText("Ready for review.");
    const titles = () => Array.from(container.querySelectorAll(".task-timeline .task-step-title > strong"), (node) => node.textContent);
    expect(titles()).toEqual(["Planning", "Initial research", "Risk analysis", "Reviewing", "Correct the evidence", "Update the analysis", "Reviewing"]);
    expect(container.querySelectorAll(".task-timeline-step")).toHaveLength(7);
    expect(container.querySelector(".task-timeline-step.task-status--failed")).toHaveTextContent("Correct the evidence");
    expect(unordered[0].id).toBe(review.id);
    fireEvent.click(screen.getByRole("tab", { name: "Execution steps" }));
    expect(titles()).toEqual(["Planning", "Initial research", "Risk analysis", "Reviewing", "Correct the evidence", "Update the analysis", "Reviewing"]);
  });

  it("uses phase and stable source order when pending steps have no timing information", async () => {
    const pending = { ...step, status: "pending" as const, started_at: null, finished_at: null, depends_on: [] };
    vi.mocked(tasksApi.listExecutions).mockResolvedValue([{ ...execution, steps: [
      { ...pending, id: "review", kind: "review" },
      { ...pending, id: "second", title: "First listed work" },
      { ...pending, id: "planning", kind: "planning" },
      { ...pending, id: "first", title: "Second listed work" },
    ] }]);
    const { container } = mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    await screen.findByText("Ready for review.");
    expect(Array.from(container.querySelectorAll(".task-timeline .task-step-title > strong"), (node) => node.textContent)).toEqual(["Planning", "First listed work", "Second listed work", "Reviewing"]);
  });

  it("renders saved execution history using its saved mode while a different mode remains in the editor draft", async () => {
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "Separate agents" }));
    fireEvent.change(screen.getByLabelText("Task description"), { target: { value: "Unsaved independent description" } });
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    await screen.findByText("Ready for review.");
    expect(screen.getByRole("heading", { name: task.text })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Execution steps" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Task settings" }));
    expect(screen.getByLabelText("Task description")).toHaveValue("Unsaved independent description");
    expect(screen.getByRole("button", { name: "Separate agents" })).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps weekly schedules and rejects empty days or an invalid time zone", async () => {
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: "weekly" } });
    changeDateTimeField(screen.getByLabelText("Time"), "09:30");
    fireEvent.change(screen.getByLabelText("Time zone"), { target: { value: "Europe/Istanbul" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Fri" }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, task.id, expect.objectContaining({ schedule: { kind: "weekly", time: "09:30", timezone: "Europe/Istanbul", weekdays: [1, 5] } })));
    await waitFor(() => expect(screen.getByRole("button", { name: "Run now" })).toBeEnabled());
    fireEvent.click(screen.getByRole("checkbox", { name: "Mon" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Fri" }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Check the date, time, time zone and selected days.");
    fireEvent.click(screen.getByRole("checkbox", { name: "Mon" }));
    fireEvent.change(screen.getByLabelText("Time zone"), { target: { value: "No/Such_Zone" } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Check the date, time, time zone and selected days.");
    expect(tasksApi.saveTask).toHaveBeenCalledOnce();
  });

  it("preserves monthly and annual choices, including leap days, and rejects invalid dates", async () => {
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: "monthly" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "31" }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenCalledWith(auth, task.id, expect.objectContaining({ schedule: expect.objectContaining({ kind: "monthly", month_days: [1, 31] }) })));
    await waitFor(() => expect(screen.getByRole("button", { name: "Run now" })).toBeEnabled());
    fireEvent.change(screen.getByRole("combobox", { name: "Run" }), { target: { value: "yearly" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Month 1" }), { target: { value: "2" } });
    fireEvent.change(screen.getByRole("spinbutton", { name: "Day 1" }), { target: { value: "30" } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Check the date, time, time zone and selected days.");
    expect(tasksApi.saveTask).toHaveBeenCalledOnce();
    fireEvent.change(screen.getByRole("spinbutton", { name: "Day 1" }), { target: { value: "29" } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(tasksApi.saveTask).toHaveBeenLastCalledWith(auth, task.id, expect.objectContaining({ schedule: expect.objectContaining({ kind: "yearly", dates: [{ month: 2, day: 29 }] }) })));
  });

  it("reuses the idempotency key after an uncertain start failure and can cancel an active execution", async () => {
    const activeRun = { ...execution, status: "running" as const, result_text: null, finished_at: null, steps: [{ ...step, status: "running" as const, result_text: null }] };
    vi.mocked(tasksApi.startExecution).mockRejectedValueOnce(new Error("Connection lost")).mockResolvedValueOnce(activeRun);
    vi.mocked(tasksApi.cancelExecution).mockResolvedValue({ ...activeRun, status: "cancelled" });
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "Run now" }));
    await screen.findByText("Connection lost");
    fireEvent.click(screen.getByRole("button", { name: "Run now" }));
    await screen.findByRole("button", { name: "Stop run" });
    const calls = vi.mocked(tasksApi.startExecution).mock.calls;
    expect(calls[0][2]).toBe(calls[1][2]);
    expect(calls[0][2]).toMatch(/^[0-9a-f-]{36}$/);
    fireEvent.click(screen.getByRole("button", { name: "Stop run" }));
    await waitFor(() => expect(tasksApi.cancelExecution).toHaveBeenCalledWith(auth, execution.id));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Stop run" })).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Run again" })).toBeEnabled();
  });

  it("polls active executions and stops polling after completion", async () => {
    vi.mocked(tasksApi.listExecutions).mockResolvedValue([{ ...execution, status: "running", result_text: null }]);
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    await screen.findByRole("button", { name: "Stop run" });
    vi.useFakeTimers();
    // Reopen under fake timers so the polling timeout belongs to this clock.
    fireEvent.click(screen.getByRole("button", { name: "Task settings" }));
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    await act(async () => { await Promise.resolve(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(1600); });
    expect(tasksApi.getExecution).toHaveBeenCalledWith(auth, execution.id);
    expect(screen.getByText("Ready for review.")).toBeInTheDocument();
    const requests = vi.mocked(tasksApi.getExecution).mock.calls.length;
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(tasksApi.getExecution).toHaveBeenCalledTimes(requests);
  });

  it("offers retry only for failed steps without uncertain external effects", async () => {
    vi.mocked(tasksApi.listExecutions).mockResolvedValue([{ ...execution, status: "needs_attention", steps: [
      { ...step, id: "safe", title: "Safe read", status: "failed", result_text: null, error: "Provider unavailable" },
      { ...step, id: "unsafe", title: "External action", status: "failed", may_have_effects: true, result_text: null, error: "Unknown outcome" },
    ] }]);
    vi.mocked(tasksApi.retryExecutionStep).mockResolvedValue({ ...execution, status: "running" });
    mount();
    await screen.findByDisplayValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    const retry = await screen.findByRole("button", { name: "Retry step" });
    expect(screen.getAllByRole("button", { name: "Retry step" })).toHaveLength(1);
    expect(screen.getByText(/previous attempt may have performed an external action/)).toBeInTheDocument();
    fireEvent.click(retry);
    await waitFor(() => expect(tasksApi.retryExecutionStep).toHaveBeenCalledWith(auth, execution.id, "safe"));
  });

  it("runs independent agents through the existing run model and blocks demo writes", async () => {
    vi.mocked(tasksApi.listTasks).mockResolvedValue([independentTask]);
    vi.mocked(tasksApi.startIndependentRuns).mockResolvedValue([{ id: "run", agent_id: researcher.id, agent_name: researcher.name, task_text: independentTask.text, scheduled_for: "2026-09-10T10:00:00Z", status: "completed", attempts: 1, output: "Independent output", error: null, started_at: null, completed_at: null, created_at: "2026-09-10T10:00:00Z", updated_at: "2026-09-10T10:00:00Z" }]);
    const first = mount();
    await screen.findByDisplayValue(independentTask.text);
    fireEvent.click(screen.getByRole("button", { name: "Run now" }));
    await screen.findByText("Independent output");
    expect(tasksApi.startIndependentRuns).toHaveBeenCalledWith(auth, independentTask.id, expect.any(String));
    expect(tasksApi.startExecution).not.toHaveBeenCalled();
    first.unmount();
    mount(true);
    await screen.findByDisplayValue(independentTask.text);
    expect(screen.getByRole("button", { name: "Run now" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "New task" })).toBeDisabled();
  });

  it("opens the task runs tab named by the page URL and keeps the URL current", async () => {
    mountAt([task.id, "runs", "steps"]);
    expect(await screen.findByText("Find launch requirements.")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Execution steps" })).toHaveAttribute("aria-selected", "true");
    expect(tasksApi.listExecutions).toHaveBeenCalledWith(auth, task.id);
    fireEvent.click(screen.getByRole("tab", { name: "Result" }));
    expect(pageRoute()).toHaveTextContent(/^task-1\/runs$/);
    expect(screen.getByText("Ready for review.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Run history" }));
    expect(pageRoute()).toHaveTextContent(/^task-1\/runs\/history$/);
    await waitFor(() => expect(screen.getByRole("tabpanel")).toHaveTextContent("Completed · 1/1"));
    fireEvent.click(screen.getByRole("button", { name: "Back to tasks" }));
    expect(pageRoute()).toHaveTextContent(/^task-1$/);
    expect(screen.getByLabelText("Task description")).toHaveValue(task.text);
    fireEvent.click(screen.getByRole("button", { name: /Independent summary.*Researcher/ }));
    expect(pageRoute()).toHaveTextContent(/^task-2$/);
    expect(screen.getByLabelText("Task description")).toHaveValue(independentTask.text);
    fireEvent.click(screen.getByRole("button", { name: "View runs" }));
    expect(pageRoute()).toHaveTextContent(/^task-2\/runs$/);
    expect(api.listAiTaskRuns).toHaveBeenCalledWith(auth, independentTask.id);
    fireEvent.click(screen.getByRole("button", { name: "Task settings" }));
    expect(pageRoute()).toHaveTextContent(/^task-2$/);
  });

  it("replaces the new task URL with the saved task", async () => {
    mountAt(["new"]);
    await screen.findByRole("button", { name: /Prepare a launch plan.*Team/ });
    expect(screen.getByLabelText("Task description")).toHaveValue("");
    fireEvent.change(screen.getByLabelText("Task description"), { target: { value: "Summarize feedback" } });
    fireEvent.click(screen.getByRole("button", { name: "Add agent" }));
    fireEvent.click(screen.getByRole("button", { name: /Researcher.*Custom agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    await waitFor(() => expect(pageRoute()).toHaveTextContent(/^task-new$/));
    expect(screen.getByLabelText("Task description")).toHaveValue("Summarize feedback");
    expect(screen.getByRole("button", { name: /Summarize feedback.*Researcher/ })).toHaveAttribute("aria-current", "true");
  });

  it.each([[[], ""], [["missing"], ""], [["runs"], ""], [["task-1", "details"], "task-1"], [["task-2", "runs", "steps"], "task-2/runs"], [["new", "runs"], "new"]])(
    "replaces the task URL %j with the page it shows", async (segments, canonical) => {
      mountAt(segments);
      await waitFor(() => expect(pageRoute().textContent).toBe(canonical));
      expect(tasksApi.listTasks).toHaveBeenCalledOnce();
    },
  );
});

describe("AITasksWorkspace plan tasks", () => {
  const planBoard: boardApi.TaskBoard = { ...board, people: [{ id: "anna", name: "Anna" }, { id: "boris", name: "Boris" }], agents: [{ id: researcher.id, name: researcher.name, preset_key: null, ready: true }] };
  const planTask: tasksApi.Task = {
    ...task, id: "task-plan", text: "Onboard a new client", agent_ids: [researcher.id], assignee_ids: ["anna"],
    agent_roles: [{ agent_id: "anna", role: " Collects " }, { agent_id: researcher.id, role: "Checks" }],
    process: { id: "process-1", version: 3, title: "Client onboarding" }, plan_progress: { done: 1, total: 2 },
    plan_steps: [
      { id: "step-1", title: "Collect documents", instructions: "Ask the client for the documents.", performer: { kind: "employee", id: "anna" } },
      { id: "step-2", title: "Check the documents", instructions: "Verify every document.", performer: { kind: "agent", id: researcher.id } },
    ],
  };
  const planStep = (number: number) => within(screen.getByRole("listitem", { name: `Step ${number}` }));
  beforeEach(() => {
    vi.mocked(boardApi.getTaskBoard).mockResolvedValue(planBoard);
    vi.mocked(tasksApi.listTasks).mockResolvedValue([planTask, task]);
  });

  it("blocks saving a plan with an incomplete step and shows the exact conflict from the server", async () => {
    vi.mocked(tasksApi.saveTask).mockRejectedValueOnce(new Error("the task changed; reload it before saving"));
    mount();
    await screen.findByDisplayValue(planTask.text);
    fireEvent.change(planStep(1).getByLabelText("Step title"), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Every step needs a title, a description and a performer.");
    expect(tasksApi.saveTask).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Remove step 1" }));
    expect(screen.getByRole("button", { name: "Remove step 1" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Save task" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("the task changed; reload it before saving");
    expect(planStep(1).getByLabelText("Step title")).toHaveValue("Check the documents");
  });

});
