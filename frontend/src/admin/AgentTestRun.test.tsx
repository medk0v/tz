import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { aiTestRequest, type AiProfile } from "../api";
import { AgentTestRun, type AgentTestRunData } from "./AgentTestRun";

vi.mock("../api", () => ({ aiTestRequest: vi.fn() }));
afterEach(cleanup);

it("applies reviewed changes to the matching sources and profile fields only after all manual drafts are cleared", async () => {
  const auth = { kind: "session" } as const;
  const profile = { id: "profile-1", instructions: "Main original", tool_instructions: "Tools original", name: "Unrelated agent name" } as AiProfile;
  const run: AgentTestRunData = {
    id: "run-1", scenario_id: "scenario-1", scenario_revision: 1, status: "behavior_error", stale: false, created_at: "2026-09-10T10:00:00Z",
    snapshot: { scenario: { name: "Recommendation application" }, profile: { instructions: "Main original", tool_instructions: "Tools original" } },
    result: { trace: [{ step: 0, call: { tool: "read_article", parameters: { article_id: "article-1" } }, response: { article_id: "article-1", title: "Used article", version: 2, content: "Observed article fragment" } }] },
  };
  const recommendations = {
    summary: "Clarify the instructions and article", stale: false,
    recommendations: [
      { target: "agent_instructions", reason: "Main advice", suggested_text: "Main applied", change: { source_key: "instructions", expected_revision: "main-one", original_text: "Main original" } },
      { target: "agent_instructions", reason: "Tool advice", suggested_text: "Tools applied", change: { source_key: "tool_instructions", expected_revision: "tools-one", original_text: "Tools original" } },
      { target: "knowledge_article", article_id: "article-1", article_title: "Used article", article_version: 2, reason: "Article advice", suggested_text: "Article applied", change: { source_key: "article:article-1", expected_revision: "2", original_text: "Article original" } },
    ],
  };
  const onProfileSaved = vi.fn();
  const onSourceBusyChange = vi.fn();
  let finishApply!: (value: unknown) => void;
  vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profileId, path) => {
    if (path === "test-runs/run-1") return run;
    if (path === "test-runs/run-1/recommendations") return recommendations;
    if (path === "test-runs/run-1/sources/instructions") return { text: "Main original", revision: "main-one" };
    if (path === "test-runs/run-1/sources/tool_instructions") return { text: "Tools original", revision: "tools-one" };
    if (path === "test-runs/run-1/sources/article%3Aarticle-1") return { text: "Article original", revision: "2", title: "Used article", status: "published" };
    if (path === "test-runs/run-1/recommendations/apply") return new Promise((resolve) => { finishApply = resolve; });
    throw new Error(`Unexpected request: ${path}`);
  });
  render(<AgentTestRun auth={auth} profile={profile} run={run} disabled={false} deleteDisabled={false} showName onDelete={vi.fn()} onReview={vi.fn()} onProfileSaved={onProfileSaved} onSourceBusyChange={onSourceBusyChange} />);
  fireEvent.click(screen.getByText("Recommendation application"));
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  const main = await screen.findByRole("tabpanel", { name: "Инструкции агента" });
  fireEvent.change(await within(main).findByLabelText("Текст для следующих запусков"), { target: { value: "Unsaved main draft" } });
  expect(screen.getByRole("button", { name: "Применить рекомендации" })).toBeDisabled();
  const toolAdvice = screen.getByRole("region", { name: "Рекомендация 2: Инструкции инструментов" });
  fireEvent.click(within(toolAdvice).getByRole("button", { name: "Редактировать источник" }));
  const tools = screen.getByRole("tabpanel", { name: "Инструкции инструментов" });
  expect(await within(tools).findByLabelText("Текст для следующих запусков")).toHaveValue("Tools original");
  expect(within(tools).getByText("Tools applied")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Применить рекомендации" })).toBeDisabled();
  expect(onProfileSaved).not.toHaveBeenCalled();
  expect(vi.mocked(aiTestRequest).mock.calls.some((call) => call[2].endsWith("/apply"))).toBe(false);

  fireEvent.click(screen.getByRole("tab", { name: /Used article/ }));
  const article = screen.getByRole("tabpanel", { name: /Used article/ });
  expect(await within(article).findByLabelText("Текст для следующих запусков")).toHaveValue("Article original");
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции агента" }));
  expect(within(main).getByLabelText("Текст для следующих запусков")).toHaveValue("Unsaved main draft");
  fireEvent.click(within(main).getByRole("button", { name: "Отмена" }));
  expect(screen.getByRole("button", { name: "Применить рекомендации" })).toBeEnabled();
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции инструментов" }));
  fireEvent.click(screen.getByRole("button", { name: "Применить рекомендации" }));
  expect(screen.getByRole("button", { name: "Применяем рекомендации…" })).toBeDisabled();
  expect(within(tools).getByLabelText("Текст для следующих запусков")).toBeDisabled();
  expect(screen.getByRole("tab", { name: "Инструкции агента" })).toBeDisabled();
  expect(onSourceBusyChange).toHaveBeenLastCalledWith(true);
  expect(onProfileSaved).not.toHaveBeenCalled();
  const applyCall = vi.mocked(aiTestRequest).mock.calls.find((call) => call[2].endsWith("/apply"));
  expect(applyCall?.[3]?.method).toBe("POST");
  expect(JSON.parse(applyCall?.[3]?.body as string)).toEqual({ changes: recommendations.recommendations.map((recommendation) => ({ ...recommendation.change, suggested_text: recommendation.suggested_text })) });

  await act(async () => finishApply({ sources: [
    { source_key: "instructions", text: "Main applied", revision: "main-two" },
    { source_key: "tool_instructions", text: "Tools applied", revision: "tools-two" },
    { source_key: "article:article-1", text: "Article applied", revision: "3", title: "Updated article title", status: "published" },
  ] }));
  expect(screen.getByRole("button", { name: "Рекомендации применены" })).toBeDisabled();
  expect(within(tools).getByLabelText("Текст для следующих запусков")).toHaveValue("Tools applied");
  expect(within(tools).getByLabelText("Текст для следующих запусков")).toBeEnabled();
  expect(within(tools).getByRole("button", { name: "Сохранить изменения" })).toBeDisabled();
  expect(onSourceBusyChange.mock.calls).toEqual([[true], [false]]);
  expect(onProfileSaved.mock.calls).toEqual([
    [{ ...profile, instructions: "Main applied" }, "instructions"],
    [{ ...profile, tool_instructions: "Tools applied" }, "tool_instructions"],
  ]);
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции агента" }));
  expect(within(main).getByLabelText("Текст для следующих запусков")).toHaveValue("Main applied");
  expect(within(main).getByText("Main original")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: /Used article/ }));
  expect(within(article).getByLabelText("Текст для следующих запусков")).toHaveValue("Article applied");
  expect(within(article).getByRole("heading", { name: "Updated article title" })).toBeInTheDocument();
  expect(within(article).getByText("Текущая версия 3 · Опубликована")).toBeInTheDocument();
  expect(within(article).getByText("Observed article fragment")).toBeInTheDocument();
});
