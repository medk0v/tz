import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { aiTestRequest, type AiProfile } from "../api";
import { AgentTestInstructions } from "./AgentTestInstructions";

vi.mock("../api", () => ({ aiTestRequest: vi.fn() }));
const auth = { kind: "session" } as const;
const profile = { id: "profile-1", instructions: "Stale profile value", tool_instructions: "Stale tool value", name: "Unsaved unrelated name" } as AiProfile;
const props = {
  auth, profile, runId: "run-1",
  snapshot: { profile: { instructions: "Instructions at run time", tool_instructions: "Tools at run time" }, knowledge: [{ article_id: "unread", title: "Unread article", content: "Unobserved content" }] },
  trace: [
    { step: 0, call: { tool: "read_article", parameters: { article_id: "article-1", offset: 15 } }, response: { article_id: "article-1", title: "Used article", version: 2, content: "<img src=x onerror=alert(1)>" } },
    { step: 1, call: { tool: "read_article", parameters: { article_id: "article-1", offset: 50 } }, response: { article_id: "article-1", title: "Used article", version: 2, content: "Second actual chunk" } },
    { call: { tool: "read_article", parameters: { article_id: "unavailable" } }, response: null },
    { call: { tool: "read_article", parameters: { article_id: "wrong-id" } }, response: { article_id: "another-id", content: "Mismatched response" } },
  ],
};
beforeEach(() => vi.mocked(aiTestRequest).mockReset());
afterEach(cleanup);

it("edits the current source separately from the immutable run text and saves only that field with its revision", async () => {
  const onProfileSaved = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Latest saved instructions", revision: "current-revision" }).mockResolvedValueOnce({ text: "My revised instructions", revision: "new-revision" });
  render(<AgentTestInstructions {...props} suggestedText="Proposed fragment" onProfileSaved={onProfileSaved} />);
  const input = await screen.findByLabelText("Текст для следующих запусков");
  expect(input).toHaveValue("Latest saved instructions");
  expect(screen.getByText("Instructions at run time")).toBeInTheDocument();
  expect(screen.getByText("Proposed fragment")).toBeInTheDocument();
  expect(aiTestRequest).toHaveBeenCalledTimes(1);
  fireEvent.change(input, { target: { value: "My revised instructions" } });
  fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
  expect(await screen.findByText(/Изменения сохранены/)).toBeInTheDocument();
  const call = vi.mocked(aiTestRequest).mock.calls[1];
  expect(call.slice(0, 3)).toEqual([auth, "profile-1", "test-runs/run-1/sources/instructions"]);
  expect(call[3]?.method).toBe("PATCH");
  expect(JSON.parse(call[3]?.body as string)).toEqual({ text: "My revised instructions", expected_revision: "current-revision" });
  expect(onProfileSaved).toHaveBeenCalledWith(expect.objectContaining({ instructions: "My revised instructions" }), "instructions");
  expect(screen.getByText("Instructions at run time")).toBeInTheDocument();
});

it("preserves a new draft when reloading after a save conflict fails", async () => {
  vi.mocked(aiTestRequest)
    .mockResolvedValueOnce({ text: "Current instructions", revision: "one" })
    .mockResolvedValueOnce({ text: "Saved instructions", revision: "two" })
    .mockRejectedValueOnce({ status: 409 })
    .mockRejectedValueOnce(new Error("Unavailable"));
  render(<AgentTestInstructions {...props} />);
  fireEvent.change(await screen.findByLabelText("Текст для следующих запусков"), { target: { value: "Saved instructions" } });
  fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
  await screen.findByText(/Изменения сохранены/);
  expect(screen.queryByRole("button", { name: "Сравнить с текущей версией" })).not.toBeInTheDocument();

  fireEvent.change(screen.getByLabelText("Текст для следующих запусков"), { target: { value: "New unsaved instructions" } });
  fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
  fireEvent.click(await screen.findByRole("button", { name: "Сравнить с текущей версией" }));

  expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось загрузить текущую версию");
  expect(screen.queryByText(/Изменения сохранены/)).not.toBeInTheDocument();
  expect(screen.getByLabelText("Текст для следующих запусков")).toHaveValue("New unsaved instructions");
  expect(aiTestRequest).toHaveBeenCalledTimes(4);
});

it("shows only observed article chunks, preserves the article draft status, and never treats unread snapshot text as used", async () => {
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Complete current article", revision: "3", title: "Current article title", status: "draft" }).mockResolvedValueOnce({ text: "New article text", revision: "4", title: "Current article title", status: "draft" });
  const { container } = render(<AgentTestInstructions {...props} initialSource={{ target: "knowledge_article", articleId: "article-1" }} />);
  expect(screen.queryByText("Unread article")).not.toBeInTheDocument();
  expect(screen.queryByText("Unobserved content")).not.toBeInTheDocument();
  expect(screen.queryByText("Mismatched response")).not.toBeInTheDocument();
  expect(screen.getByText("Second actual chunk")).toBeInTheDocument();
  expect(screen.getByText("<img src=x onerror=alert(1)>")).toBeInTheDocument();
  expect(container.querySelector("img")).toBeNull();
  const input = await screen.findByLabelText("Текст для следующих запусков");
  expect(input).toHaveValue("Complete current article");
  expect(screen.getByText("Текущая версия 3 · Черновик")).toBeInTheDocument();
  fireEvent.change(input, { target: { value: "New article text" } });
  fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
  expect(await screen.findByText(/Черновик статьи сохранён/)).toBeInTheDocument();
  expect(vi.mocked(aiTestRequest).mock.calls[1][2]).toBe("test-runs/run-1/sources/article%3Aarticle-1");
  expect(JSON.parse(vi.mocked(aiTestRequest).mock.calls[1][3]?.body as string)).toEqual({ text: "New article text", expected_revision: "3" });
});

it("marks scenario articles and displays observed chunks without loading or editing published knowledge", () => {
  render(<AgentTestInstructions {...props} snapshot={{ ...props.snapshot, scenario: { knowledge_articles: [{ article_id: "article-1", title: "Fixture title", body: "Unread fixture remainder", version: 2 }] } }} initialSource={{ target: "knowledge_article", articleId: "article-1" }} />);
  expect(screen.getByRole("tab", { name: /Used article.*Статья внутри теста/ })).toBeInTheDocument();
  const source = screen.getByRole("region", { name: "Статья внутри теста" });
  expect(source).toHaveTextContent("Second actual chunk");
  expect(source).toHaveTextContent("откройте редактор теста и раздел «Статьи внутри теста»");
  expect(screen.queryByText("Unread fixture remainder")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Текст для следующих запусков")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Сохранить изменения" })).not.toBeInTheDocument();
  expect(aiTestRequest).not.toHaveBeenCalled();
});

it("preserves drafts across source tabs, supports keyboard selection, and cancels without saving", async () => {
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Current instructions", revision: "one" }).mockResolvedValueOnce({ text: "Current tool instructions", revision: "two" });
  render(<AgentTestInstructions {...props} />);
  fireEvent.change(await screen.findByLabelText("Текст для следующих запусков"), { target: { value: "Local draft" } });
  fireEvent.keyDown(screen.getByRole("tab", { name: "Инструкции агента" }), { key: "ArrowDown" });
  const tools = screen.getByRole("tabpanel", { name: "Инструкции инструментов" });
  expect(await within(tools).findByLabelText("Текст для следующих запусков")).toHaveValue("Current tool instructions");
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции агента" }));
  const instructions = screen.getByRole("tabpanel", { name: "Инструкции агента" });
  expect(within(instructions).getByLabelText("Текст для следующих запусков")).toHaveValue("Local draft");
  fireEvent.click(within(instructions).getByRole("button", { name: "Отмена" }));
  expect(within(instructions).getByLabelText("Текст для следующих запусков")).toHaveValue("Current instructions");
  expect(vi.mocked(aiTestRequest).mock.calls.every((call) => call[3]?.method !== "PATCH")).toBe(true);
});

it("reports unsaved changes across all visited sources until the last draft is cancelled, undone, or saved", async () => {
  const onDirtyChange = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Current instructions", revision: "one" }).mockResolvedValueOnce({ text: "Current tools", revision: "two" }).mockResolvedValueOnce({ text: "Saved tools", revision: "three" });
  const rendered = render(<AgentTestInstructions {...props} onDirtyChange={onDirtyChange} />);
  fireEvent.change(await screen.findByLabelText("Текст для следующих запусков"), { target: { value: "Instruction draft" } });
  expect(onDirtyChange.mock.calls).toEqual([[true]]);
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции инструментов" }));
  const tools = screen.getByRole("tabpanel", { name: "Инструкции инструментов" });
  fireEvent.change(await within(tools).findByLabelText("Текст для следующих запусков"), { target: { value: "Tool draft" } });
  expect(onDirtyChange.mock.calls).toEqual([[true]]);
  fireEvent.click(within(tools).getByRole("button", { name: "Отмена" }));
  expect(onDirtyChange.mock.calls).toEqual([[true]]);
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции агента" }));
  const instructions = screen.getByRole("tabpanel", { name: "Инструкции агента" });
  fireEvent.change(within(instructions).getByLabelText("Текст для следующих запусков"), { target: { value: "Current instructions" } });
  expect(onDirtyChange.mock.calls).toEqual([[true], [false]]);
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции инструментов" }));
  fireEvent.change(within(tools).getByLabelText("Текст для следующих запусков"), { target: { value: "Saved tools" } });
  fireEvent.click(within(tools).getByRole("button", { name: "Сохранить изменения" }));
  await within(tools).findByText(/Изменения сохранены/);
  expect(onDirtyChange.mock.calls).toEqual([[true], [false], [true], [false]]);
  fireEvent.change(within(tools).getByLabelText("Текст для следующих запусков"), { target: { value: "Abandoned draft" } });
  rendered.unmount();
  expect(onDirtyChange.mock.calls).toEqual([[true], [false], [true], [false], [true], [false]]);
});

it("synchronizes an applied source and its revision while preserving unrelated drafts and later manual edits", async () => {
  const onDirtyChange = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Current instructions", revision: "one" }).mockResolvedValueOnce({ text: "Current tools", revision: "two" }).mockResolvedValueOnce({ text: "Manual tools after applying", revision: "four" });
  const rendered = render(<AgentTestInstructions {...props} onDirtyChange={onDirtyChange} />);
  fireEvent.change(await screen.findByLabelText("Текст для следующих запусков"), { target: { value: "Instruction draft" } });
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции инструментов" }));
  const tools = screen.getByRole("tabpanel", { name: "Инструкции инструментов" });
  expect(await within(tools).findByLabelText("Текст для следующих запусков")).toHaveValue("Current tools");
  const savedSources = [{ source_key: "tool_instructions", text: "Applied tools", revision: "three" }];
  rendered.rerender(<AgentTestInstructions {...props} savedSources={savedSources} onDirtyChange={onDirtyChange} />);
  expect(within(tools).getByLabelText("Текст для следующих запусков")).toHaveValue("Applied tools");
  expect(within(tools).getByRole("button", { name: "Сохранить изменения" })).toBeDisabled();
  expect(onDirtyChange.mock.calls).toEqual([[true]]);
  fireEvent.change(within(tools).getByLabelText("Текст для следующих запусков"), { target: { value: "Manual tools after applying" } });
  rendered.rerender(<AgentTestInstructions {...props} savedSources={savedSources.map((source) => ({ ...source }))} onDirtyChange={onDirtyChange} />);
  expect(within(tools).getByLabelText("Текст для следующих запусков")).toHaveValue("Manual tools after applying");
  fireEvent.click(within(tools).getByRole("button", { name: "Сохранить изменения" }));
  await within(tools).findByText(/Изменения сохранены/);
  expect(JSON.parse(vi.mocked(aiTestRequest).mock.calls[2][3]?.body as string)).toEqual({ text: "Manual tools after applying", expected_revision: "three" });
  expect(onDirtyChange.mock.calls).toEqual([[true]]);
  fireEvent.click(screen.getByRole("tab", { name: "Инструкции агента" }));
  const instructions = screen.getByRole("tabpanel", { name: "Инструкции агента" });
  expect(within(instructions).getByLabelText("Текст для следующих запусков")).toHaveValue("Instruction draft");
  fireEvent.click(within(instructions).getByRole("button", { name: "Отмена" }));
  expect(onDirtyChange.mock.calls).toEqual([[true], [false]]);
});

it("updates the applied article title and status and ignores an earlier source response", async () => {
  let finish!: (value: unknown) => void;
  vi.mocked(aiTestRequest).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  render(<AgentTestInstructions {...props} initialSource={{ target: "knowledge_article", articleId: "article-1" }} savedSources={[{ source_key: "article:article-1", text: "Applied article text", revision: "4", title: "Applied article title", status: "draft" }]} />);
  expect(screen.getByLabelText("Текст для следующих запусков")).toHaveValue("Applied article text");
  expect(screen.getByRole("heading", { name: "Applied article title" })).toBeInTheDocument();
  expect(screen.getByText("Текущая версия 4 · Черновик")).toBeInTheDocument();
  expect(vi.mocked(aiTestRequest).mock.calls[0][3]?.signal?.aborted).toBe(true);
  await act(async () => finish({ text: "Earlier article text", revision: "3", status: "published" }));
  expect(screen.getByLabelText("Текст для следующих запусков")).toHaveValue("Applied article text");
  expect(screen.getByRole("button", { name: "Сохранить изменения" })).toBeDisabled();
});

it("saves tool instructions through their own source endpoint", async () => {
  const onProfileSaved = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Current tool instructions", revision: "tool-one" }).mockResolvedValueOnce({ text: "Updated tool instructions", revision: "tool-two" });
  render(<AgentTestInstructions {...props} initialSource={{ target: "tool_instructions" }} onProfileSaved={onProfileSaved} />);
  fireEvent.change(await screen.findByLabelText("Текст для следующих запусков"), { target: { value: "Updated tool instructions" } });
  fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
  await screen.findByText(/Изменения сохранены/);
  expect(vi.mocked(aiTestRequest).mock.calls[1][2]).toBe("test-runs/run-1/sources/tool_instructions");
  expect(JSON.parse(vi.mocked(aiTestRequest).mock.calls[1][3]?.body as string)).toEqual({ text: "Updated tool instructions", expected_revision: "tool-one" });
  expect(onProfileSaved).toHaveBeenCalledWith(expect.objectContaining({ tool_instructions: "Updated tool instructions" }), "tool_instructions");
});

it("keeps edits on a revision conflict and requires comparison with the latest source before retrying", async () => {
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Original", revision: "one" }).mockRejectedValueOnce({ status: 409 }).mockResolvedValueOnce({ text: "Other operator update", revision: "two" }).mockResolvedValueOnce({ text: "My merged update", revision: "three" });
  render(<AgentTestInstructions {...props} />);
  fireEvent.change(await screen.findByLabelText("Текст для следующих запусков"), { target: { value: "My draft" } });
  fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Источник изменился во время редактирования");
  expect(screen.getByLabelText("Текст для следующих запусков")).toHaveValue("My draft");
  expect(screen.getByRole("button", { name: "Сохранить изменения" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Сравнить с текущей версией" }));
  expect(await screen.findByText("Other operator update")).toBeInTheDocument();
  expect(screen.getByLabelText("Текст для следующих запусков")).toHaveValue("My draft");
  fireEvent.change(screen.getByLabelText("Текст для следующих запусков"), { target: { value: "My merged update" } });
  fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
  await screen.findByText(/Изменения сохранены/);
  expect(JSON.parse(vi.mocked(aiTestRequest).mock.calls[3][3]?.body as string)).toEqual({ text: "My merged update", expected_revision: "two" });
});

it("ignores late responses after switching to a different run", async () => {
  let finish!: (value: unknown) => void;
  vi.mocked(aiTestRequest).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; })).mockResolvedValueOnce({ text: "Second run source", revision: "second" });
  const rendered = render(<AgentTestInstructions {...props} />);
  rendered.rerender(<AgentTestInstructions {...props} runId="run-2" />);
  expect(await screen.findByLabelText("Текст для следующих запусков")).toHaveValue("Second run source");
  await act(async () => finish({ text: "Late first run source", revision: "first" }));
  expect(screen.getByLabelText("Текст для следующих запусков")).toHaveValue("Second run source");
  expect(vi.mocked(aiTestRequest).mock.calls[0][3]?.signal?.aborted).toBe(true);
});

it("keeps the article draft when recommendations select another source", async () => {
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Article current text", revision: "3", status: "published" }).mockResolvedValueOnce({ text: "Main current text", revision: "main" });
  const rendered = render(<AgentTestInstructions {...props} initialSource={{ target: "knowledge_article", articleId: "article-1" }} suggestedText="Article advice" />);
  fireEvent.change(await screen.findByLabelText("Текст для следующих запусков"), { target: { value: "Article local draft" } });
  rendered.rerender(<AgentTestInstructions {...props} initialSource={{ target: "agent_instructions" }} suggestedText="Main advice" />);
  const main = screen.getByRole("tabpanel", { name: "Инструкции агента" });
  expect(await within(main).findByLabelText("Текст для следующих запусков")).toHaveValue("Main current text");
  fireEvent.click(screen.getByRole("tab", { name: /Used article/ }));
  expect(within(screen.getByRole("tabpanel", { name: /Used article/ })).getByLabelText("Текст для следующих запусков")).toHaveValue("Article local draft");
  expect(aiTestRequest).toHaveBeenCalledTimes(2);
});

it("does not silently open agent instructions for an unavailable recommended article", () => {
  render(<AgentTestInstructions {...props} initialSource={{ target: "knowledge_article", articleId: "unread" }} suggestedText="Unusable advice" />);
  expect(screen.getByRole("status")).toHaveTextContent("Источник из рекомендации не был прочитан");
  expect(screen.queryByRole("tabpanel")).not.toBeInTheDocument();
  expect(screen.queryByText("Unusable advice")).not.toBeInTheDocument();
  expect(aiTestRequest).not.toHaveBeenCalled();
});

it("reports article edit permissions while retaining the observed run text", async () => {
  vi.mocked(aiTestRequest).mockRejectedValueOnce({ status: 403 });
  render(<AgentTestInstructions {...props} initialSource={{ target: "knowledge_article", articleId: "article-1" }} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("Для редактирования статьи нужны права управления базой знаний");
  expect(screen.getByText("Second actual chunk")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Сохранить изменения" })).not.toBeInTheDocument();
});

it("balances busy notifications for a pending save on unmount and never sends a reset from an idle editor", async () => {
  let finish!: (value: unknown) => void;
  const onBusyChange = vi.fn();
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Current text", revision: "one" }).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const rendered = render(<AgentTestInstructions {...props} onBusyChange={onBusyChange} />);
  fireEvent.change(await screen.findByLabelText("Текст для следующих запусков"), { target: { value: "New text" } });
  expect(onBusyChange).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
  expect(onBusyChange.mock.calls).toEqual([[true]]);
  rendered.unmount();
  expect(onBusyChange.mock.calls).toEqual([[true], [false]]);
  await act(async () => finish({ text: "New text", revision: "two" }));
  expect(onBusyChange.mock.calls).toEqual([[true], [false]]);
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Idle text", revision: "idle" });
  const idle = render(<AgentTestInstructions {...props} onBusyChange={onBusyChange} />);
  await screen.findByLabelText("Текст для следующих запусков");
  idle.unmount();
  expect(onBusyChange.mock.calls).toEqual([[true], [false]]);
});

it("drops prior current text immediately when authorization changes", async () => {
  let finish!: (value: unknown) => void;
  vi.mocked(aiTestRequest).mockResolvedValueOnce({ text: "Previous authorization text", revision: "one" }).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const rendered = render(<AgentTestInstructions {...props} />);
  expect(await screen.findByLabelText("Текст для следующих запусков")).toHaveValue("Previous authorization text");
  rendered.rerender(<AgentTestInstructions {...props} auth={{ kind: "access_token", token: "different-token" }} />);
  expect(screen.queryByLabelText("Текст для следующих запусков")).not.toBeInTheDocument();
  await act(async () => finish({ text: "New authorization text", revision: "two" }));
  expect(screen.getByLabelText("Текст для следующих запусков")).toHaveValue("New authorization text");
});
