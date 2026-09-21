import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { aiTestRequest } from "../api";
import { AgentTestRecommendations } from "./AgentTestRecommendations";

vi.mock("../api", () => ({ aiTestRequest: vi.fn() }));
const auth = { kind: "session" } as const;
const props = { auth, profileId: "profile-1", runId: "run-1", stale: false, disabled: false };
const result = {
  summary: "Не проверен ID контакта", stale: true,
  recommendations: [{ target: "agent_instructions", reason: "Применена статья другого контакта", suggested_text: "Проверьте contact_id", step: 1 },
    { target: "knowledge_article", reason: "Добавьте условие", suggested_text: "<img src=x onerror=alert(1)>", article_id: "article-1", article_title: "Персональный ответ", article_version: 7 }],
};
beforeEach(() => vi.mocked(aiTestRequest).mockReset());
afterEach(cleanup);

it("requests advice only on demand, shows source metadata, and retries without editing data", async () => {
  const onEditInstructions = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValueOnce(result).mockRejectedValueOnce(new Error("Internal server error"));
  const { container } = render(<AgentTestRecommendations {...props} onEditInstructions={onEditInstructions} />);
  expect(aiTestRequest).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  await screen.findByText(result.summary);
  expect(screen.getByText(/Настройки или сценарий изменились/)).toBeInTheDocument();
  expect(screen.getByText(/ID: article-1 · Версия 7/)).toBeInTheDocument();
  expect(container.querySelector("img")).toBeNull();
  expect(onEditInstructions).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Открыть инструкции с рекомендацией" }));
  expect(onEditInstructions).toHaveBeenCalledWith("Проверьте contact_id");
  fireEvent.click(screen.getByRole("button", { name: "Обновить рекомендации" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось получить рекомендации. Попробуйте снова.");
  expect(screen.queryByText(result.summary)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Обновить рекомендации" })).toBeEnabled();
  expect(vi.mocked(aiTestRequest).mock.calls).toHaveLength(2);
  for (const call of vi.mocked(aiTestRequest).mock.calls) {
    expect(call).toEqual([auth, "profile-1", "test-runs/run-1/recommendations", { method: "POST", signal: expect.any(AbortSignal) }]);
  }
});

it.each(["profileId", "runId"] as const)("ignores previous advice and pending responses after %s changes", async (field) => {
  let finish!: (value: unknown) => void;
  vi.mocked(aiTestRequest).mockResolvedValueOnce(result).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const rendered = render(<AgentTestRecommendations {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  await screen.findByText(result.summary);
  fireEvent.click(screen.getByRole("button", { name: "Обновить рекомендации" }));
  rendered.rerender(<AgentTestRecommendations {...props} {...{ [field]: "other" }} />);
  expect(screen.getByRole("button", { name: "Рекомендации по инструкциям" })).toBeEnabled();
  await act(async () => finish(result));
  expect(screen.queryByText(result.summary)).not.toBeInTheDocument();
  await waitFor(() => expect(vi.mocked(aiTestRequest).mock.calls[1][3]?.signal?.aborted).toBe(true));
});

it("opens the used sources with recommendations and targets the selected article without applying its text", async () => {
  const onOpenInstructions = vi.fn();
  const onEditSource = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValue(result);
  render(<AgentTestRecommendations {...props} onOpenInstructions={onOpenInstructions} onEditSource={onEditSource} instructions={<p>Сохранённые инструкции запуска</p>} />);
  fireEvent.click(screen.getByRole("button", { name: "Использованные инструкции" }));
  expect(onOpenInstructions).toHaveBeenCalledTimes(1);
  expect(aiTestRequest).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  await screen.findByText(result.summary);
  expect(onOpenInstructions).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("region", { name: "Инструкции и рекомендации" })).toHaveTextContent("Сохранённые инструкции запуска");
  const article = screen.getByRole("region", { name: "Рекомендация 2: Статья базы знаний" });
  fireEvent.click(within(article).getByRole("button", { name: "Редактировать источник" }));
  expect(onEditSource).toHaveBeenCalledWith(result.recommendations[1]);
  expect(vi.mocked(aiTestRequest).mock.calls).toHaveLength(1);
  expect(vi.mocked(aiTestRequest).mock.calls[0][2]).toBe("test-runs/run-1/recommendations");
});

it("labels scenario article advice and offers its test snapshot instead of a knowledge source editor", async () => {
  const onEditSource = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValue(result);
  render(<AgentTestRecommendations {...props} testArticleIds={["article-1"]} onEditSource={onEditSource} />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  const article = await screen.findByRole("region", { name: "Рекомендация 2: Статья внутри теста" });
  expect(within(article).queryByRole("button", { name: "Редактировать источник" })).not.toBeInTheDocument();
  fireEvent.click(within(article).getByRole("button", { name: "Посмотреть статью теста" }));
  expect(onEditSource).toHaveBeenCalledWith(result.recommendations[1]);
});

it("closes and reopens cached recommendations without another request and restores focus to the analysis trigger", async () => {
  vi.mocked(aiTestRequest).mockResolvedValueOnce(result);
  render(<AgentTestRecommendations {...props} />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  await screen.findByText(result.summary);
  const workspace = screen.getByRole("region", { name: "Инструкции и рекомендации" });
  fireEvent.click(screen.getByRole("button", { name: "Закрыть инструкции и рекомендации" }));
  expect(workspace).toBeInTheDocument();
  expect(workspace).not.toBeVisible();
  expect(screen.queryByRole("region", { name: "Инструкции и рекомендации" })).not.toBeInTheDocument();
  const trigger = screen.getByRole("button", { name: "Показать рекомендации" });
  expect(trigger).toHaveFocus();
  expect(aiTestRequest).toHaveBeenCalledTimes(1);
  fireEvent.click(trigger);
  expect(workspace).toBeVisible();
  expect(screen.getByText(result.summary)).toBeVisible();
  expect(screen.getByRole("button", { name: "Обновить рекомендации" })).toBeEnabled();
  expect(aiTestRequest).toHaveBeenCalledTimes(1);
});

it("keeps the workspace closed when pending recommendations arrive and reopens the completed result on demand", async () => {
  let finish!: (value: unknown) => void;
  vi.mocked(aiTestRequest).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  render(<AgentTestRecommendations {...props} onOpenInstructions={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  const workspace = screen.getByRole("region", { name: "Инструкции и рекомендации" });
  expect(workspace).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Закрыть инструкции и рекомендации" }));
  expect(workspace).not.toBeVisible();
  expect(screen.getByRole("button", { name: "Использованные инструкции" })).toHaveFocus();
  await act(async () => finish(result));
  expect(workspace).not.toBeVisible();
  expect(screen.queryByRole("region", { name: "Инструкции и рекомендации" })).not.toBeInTheDocument();
  expect(screen.getByText(result.summary)).not.toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Показать рекомендации" }));
  expect(workspace).toBeVisible();
  expect(screen.getByText(result.summary)).toBeVisible();
  expect(aiTestRequest).toHaveBeenCalledTimes(1);
});

const exactResult = {
  ...result,
  recommendations: [
    { ...result.recommendations[0], change: { source_key: "tool_instructions", expected_revision: "tools-revision", original_text: "Проверьте контакт" } },
    { ...result.recommendations[1], change: { source_key: "article:article-1", expected_revision: "7", original_text: "Ответьте клиенту" } },
    { target: "runtime", reason: "Не хватает данных", suggested_text: "Проверьте доступность сервиса" },
  ],
};

it("previews the exact source replacements and saves all of them once only after applying", async () => {
  const onApplied = vi.fn();
  const onBusyChange = vi.fn();
  let finish!: (value: unknown) => void;
  vi.mocked(aiTestRequest).mockResolvedValueOnce(exactResult).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const { container } = render(<AgentTestRecommendations {...props} onApplied={onApplied} onBusyChange={onBusyChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  const tools = await screen.findByRole("region", { name: "Рекомендация 1: Инструкции инструментов" });
  expect(within(tools).getByText("Было")).toBeInTheDocument();
  expect(within(tools).getByText("Проверьте контакт")).toBeInTheDocument();
  expect(within(tools).getByText("Станет")).toBeInTheDocument();
  expect(within(tools).getByText("Проверьте contact_id")).toBeInTheDocument();
  expect(container.querySelector("img")).toBeNull();
  expect(aiTestRequest).toHaveBeenCalledTimes(1);
  expect(screen.getByText(/показанные замены «Было → Станет»: 2/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Применить рекомендации" }));
  expect(screen.getByRole("button", { name: "Применяем рекомендации…" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Обновить рекомендации" })).toBeDisabled();
  expect(onBusyChange).toHaveBeenLastCalledWith(true);
  expect(aiTestRequest).toHaveBeenLastCalledWith(auth, "profile-1", "test-runs/run-1/recommendations/apply", {
    method: "POST", signal: expect.any(AbortSignal), body: JSON.stringify({ changes: exactResult.recommendations.flatMap((item) => "change" in item ? [{ ...item.change, suggested_text: item.suggested_text }] : []) }),
  });
  const sources = [{ source_key: "tool_instructions", text: "Проверьте contact_id", revision: "new-revision" }, { source_key: "article:article-1", text: "<img src=x onerror=alert(1)>", revision: "8", status: "draft" }];
  await act(async () => finish({ sources }));
  expect(onApplied).toHaveBeenCalledWith(sources);
  expect(onBusyChange).toHaveBeenLastCalledWith(false);
  expect(screen.getByRole("button", { name: "Рекомендации применены" })).toBeDisabled();
  expect(screen.getByText(/Изменённые черновики статей нужно опубликовать/)).toBeInTheDocument();
  expect(aiTestRequest).toHaveBeenCalledTimes(2);
});

it("keeps the exact proposal after a revision conflict and requires fresh recommendations", async () => {
  const onApplied = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValueOnce(exactResult).mockRejectedValueOnce({ status: 409 }).mockResolvedValueOnce(exactResult);
  render(<AgentTestRecommendations {...props} onApplied={onApplied} />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  fireEvent.click(await screen.findByRole("button", { name: "Применить рекомендации" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Изменения не применены. Обновите рекомендации");
  expect(screen.getByText("Проверьте контакт")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Применить рекомендации" })).toBeDisabled();
  expect(onApplied).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Обновить рекомендации" }));
  expect(await screen.findByRole("button", { name: "Применить рекомендации" })).toBeEnabled();
});

it("requires manual source drafts to be saved or discarded before applying", async () => {
  vi.mocked(aiTestRequest).mockResolvedValueOnce(exactResult);
  const rendered = render(<AgentTestRecommendations {...props} hasUnsavedChanges />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  expect(await screen.findByRole("button", { name: "Применить рекомендации" })).toBeDisabled();
  expect(screen.getByText(/Сохраните или отмените ручные правки/)).toBeInTheDocument();
  rendered.rerender(<AgentTestRecommendations {...props} hasUnsavedChanges={false} />);
  expect(screen.getByRole("button", { name: "Применить рекомендации" })).toBeEnabled();
  expect(aiTestRequest).toHaveBeenCalledTimes(1);
});

it.each([[], result.recommendations, [{ ...exactResult.recommendations[1] }]].map((recommendations) => ({ recommendations })))("shows a disabled apply button when no supported exact replacement exists", async ({ recommendations }) => {
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ ...result, recommendations });
  render(<AgentTestRecommendations {...props} testArticleIds={["article-1"]} />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  expect(await screen.findByRole("button", { name: "Применить рекомендации" })).toBeDisabled();
  expect(screen.getByText(/Нет точных замен для применения/)).toBeInTheDocument();
});

it("ignores an apply response from a previous run and clears its busy state", async () => {
  const onApplied = vi.fn();
  const onBusyChange = vi.fn();
  let finish!: (value: unknown) => void;
  vi.mocked(aiTestRequest).mockResolvedValueOnce(exactResult).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const rendered = render(<AgentTestRecommendations {...props} onApplied={onApplied} onBusyChange={onBusyChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
  fireEvent.click(await screen.findByRole("button", { name: "Применить рекомендации" }));
  rendered.rerender(<AgentTestRecommendations {...props} runId="run-2" onApplied={onApplied} onBusyChange={onBusyChange} />);
  await act(async () => finish({ sources: [] }));
  expect(onApplied).not.toHaveBeenCalled();
  expect(onBusyChange).toHaveBeenLastCalledWith(false);
  expect(screen.queryByRole("button", { name: "Рекомендации применены" })).not.toBeInTheDocument();
});
