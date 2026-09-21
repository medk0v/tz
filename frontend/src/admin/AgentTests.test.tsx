import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { aiTestRequest, type AiProfile } from "../api";
import { AgentTests } from "./AgentTests";

vi.mock("../api", () => ({ aiTestRequest: vi.fn() }));
const profile = { id: "agent-1", language: "ru, en", channel_ids: ["channel-1"] } as AiProfile;
const auth = { kind: "session" } as const;
const savedTests = () => ["Первый", "Второй"].map((name, index) => ({
  id: `test-${index + 1}`, revision: index + 1,
  scenario: { name, language: "ru", channel_id: null, history: [], operator_present: false, closed: false, timer_seconds: null, steps: [], fixtures: [], live_allowlist: [], source_articles: [] },
}));

describe("agent tests", () => {
  afterEach(cleanup);
  beforeEach(() => { vi.mocked(aiTestRequest).mockReset(); vi.mocked(aiTestRequest).mockResolvedValue({ items: [] }); });
  it("opens the editor inside the chosen test, saves that test, and creates new tests above the list", async () => {
    const items = savedTests();
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path) => ({ items: path === "tests" ? items : [] }));
    render(<AgentTests auth={auth} profile={profile} />);
    const first = await screen.findByRole("region", { name: "Тест Первый" });
    const second = screen.getByRole("region", { name: "Тест Второй" });
    fireEvent.click(within(first).getByText("Первый"));
    expect(screen.queryByLabelText("Название")).not.toBeInTheDocument();
    fireEvent.click(within(first).getByRole("button", { name: "Редактировать Первый" }));
    expect(within(first).getByLabelText("Название")).toHaveValue("Первый");
    expect(within(first).getByRole("checkbox", { name: "Контакт в тесте" })).not.toBeChecked();
    expect(within(first).queryByLabelText("ID контакта")).not.toBeInTheDocument();
    expect(within(second).queryByLabelText("Название")).not.toBeInTheDocument();
    const closeEditor = within(first).getByRole("button", { name: "Закрыть редактирование Первый" });
    expect(closeEditor).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(closeEditor);
    expect(screen.queryByLabelText("Название")).not.toBeInTheDocument();
    expect(within(first).getByRole("button", { name: "Редактировать Первый" })).toHaveAttribute("aria-expanded", "false");
    expect(vi.mocked(aiTestRequest).mock.calls.every((args) => !args[3]?.method)).toBe(true);
    fireEvent.click(within(first).getByRole("button", { name: "Редактировать Первый" }));
    fireEvent.click(within(second).getByRole("button", { name: "Редактировать Второй" }));
    expect(within(first).queryByLabelText("Название")).not.toBeInTheDocument();
    fireEvent.change(within(second).getByLabelText("Название"), { target: { value: "Обновлённый" } });
    fireEvent.click(within(second).getByRole("button", { name: "Сохранить тест" }));
    await screen.findByText("Тест сохранён");
    expect(aiTestRequest).toHaveBeenCalledWith(auth, profile.id, "tests/test-2", {
      method: "PUT", body: JSON.stringify({ revision: 2, scenario: { ...items[1].scenario, name: "Обновлённый" } }),
    });
    expect(screen.queryByLabelText("Название")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Новый тест" }));
    const newTitle = screen.getByLabelText("Название");
    expect(newTitle.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(within(first).queryByLabelText("Название")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Отмена" }));
    expect(screen.queryByLabelText("Название")).not.toBeInTheDocument();
  });
  it("creates a test with an artificial contact ID, validates it, and allows an empty name", async () => {
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click(screen.getByRole("button", { name: "Новый тест" }));
    fireEvent.change(screen.getByLabelText("Название"), { target: { value: "Ответ по ID контакта" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Контакт в тесте" }));
    const contactId = screen.getByRole("textbox", { name: "ID контакта" });
    expect((contactId as HTMLInputElement).value).toMatch(/^[0-9a-f-]{36}$/);
    expect(screen.getByRole("textbox", { name: "Имя контакта" })).toHaveValue("");
    fireEvent.change(contactId, { target: { value: "invalid-id" } });
    expect(contactId).toHaveAttribute("aria-invalid", "true");
    fireEvent.click(screen.getByRole("button", { name: "Сохранить тест" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Укажите ID контакта в формате UUID.");
    expect(vi.mocked(aiTestRequest).mock.calls.filter((call) => call[3]?.method)).toHaveLength(0);
    fireEvent.change(contactId, { target: { value: " 00000000-0000-4000-8000-000000000002 " } });
    expect(contactId).toHaveAttribute("aria-invalid", "false");
    fireEvent.click(screen.getByRole("button", { name: "Сохранить тест" }));
    await screen.findByText("Тест сохранён");
    const write = vi.mocked(aiTestRequest).mock.calls.find((call) => call[3]?.method === "POST")!;
    expect(write.slice(0, 3)).toEqual([auth, profile.id, "tests"]);
    expect(JSON.parse(write[3]!.body as string).scenarios[0]).toMatchObject({
      name: "Ответ по ID контакта", contact: { contact_id: "00000000-0000-4000-8000-000000000002", display_name: null },
    });
  });
  it("loads and saves the contact name with its test and removes the contact when disabled", async () => {
    const contact = { contact_id: "00000000-0000-4000-8000-000000000001", display_name: "Иван Петров" };
    let item = { ...savedTests()[0], scenario: { ...savedTests()[0].scenario, contact: contact as typeof contact | null } };
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "PUT") { item = { ...item, revision: item.revision + 1, scenario: JSON.parse(options.body as string).scenario }; return {}; }
      return { items: path === "tests" ? [item] : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click(await screen.findByRole("button", { name: "Редактировать Первый" }));
    expect(screen.getByRole("checkbox", { name: "Контакт в тесте" })).toBeChecked();
    expect(screen.getByRole("textbox", { name: "ID контакта" })).toHaveValue(contact.contact_id);
    expect(screen.getByRole("textbox", { name: "Имя контакта" })).toHaveValue(contact.display_name);
    fireEvent.change(screen.getByRole("textbox", { name: "Имя контакта" }), { target: { value: "  Анна Иванова  " } });
    fireEvent.click(screen.getByRole("button", { name: "Сохранить тест" }));
    await screen.findByText("Тест сохранён");
    expect(item.scenario.contact).toEqual({ ...contact, display_name: "Анна Иванова" });
    fireEvent.click(screen.getByRole("button", { name: "Редактировать Первый" }));
    expect(screen.getByRole("textbox", { name: "Имя контакта" })).toHaveValue("Анна Иванова");
    fireEvent.click(screen.getByRole("checkbox", { name: "Контакт в тесте" }));
    expect(screen.queryByLabelText("ID контакта")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Сохранить тест" }));
    await waitFor(() => expect(item.scenario.contact).toBeNull());
    await screen.findByText("Тест сохранён");
    expect(vi.mocked(aiTestRequest).mock.calls.filter((call) => call[3]?.method === "PUT")).toHaveLength(2);
  });
  it("stores articles inside a test, blocks invalid JSON, and preserves article edits without writing knowledge sources", async () => {
    const articles = [{ article_id: "00000000-0000-4000-8000-000000000003", title: "Контакт: Иван Петров — Ответ", body: "Заготовленный ответ", version: 1 }];
    let items = savedTests();
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "POST") { items = [...items, { id: "inline-test", revision: 1, scenario: JSON.parse(options.body as string).scenarios[0] }]; return {}; }
      if (options?.method === "PUT") { items = items.map((item) => item.id === "inline-test" ? { ...item, revision: item.revision + 1, scenario: JSON.parse(options.body as string).scenario } : item); return {}; }
      return { items: path === "tests" ? items : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click(screen.getByRole("button", { name: "Новый тест" }));
    fireEvent.change(screen.getByLabelText("Название"), { target: { value: "Статья по имени" } });
    fireEvent.click(screen.getByText("Статьи внутри теста", { selector: "summary" }));
    const articleInput = screen.getByLabelText("Статьи теста");
    expect(articleInput).toHaveValue("[]");
    fireEvent.change(articleInput, { target: { value: "{" } });
    expect(articleInput).toHaveAttribute("aria-invalid", "true");
    fireEvent.click(screen.getByRole("button", { name: "Сохранить тест" }));
    expect(screen.getByText("Исправьте JSON перед сохранением")).toBeInTheDocument();
    expect(vi.mocked(aiTestRequest).mock.calls.filter((call) => call[3]?.method)).toHaveLength(0);
    fireEvent.change(articleInput, { target: { value: JSON.stringify(articles) } });
    fireEvent.click(screen.getByRole("button", { name: "Сохранить тест" }));
    await screen.findByText("Тест сохранён");
    fireEvent.click(screen.getByRole("button", { name: "Редактировать Статья по имени" }));
    expect(screen.getByLabelText("Статьи теста")).toHaveValue(JSON.stringify(articles, null, 2));
    const changed = [{ ...articles[0], title: "Новый заголовок", body: "Обновлённый ответ", version: 2 }];
    fireEvent.change(screen.getByLabelText("Статьи теста"), { target: { value: JSON.stringify(changed) } });
    fireEvent.click(screen.getByRole("button", { name: "Сохранить тест" }));
    await screen.findByText("Тест сохранён");
    fireEvent.click(screen.getByRole("button", { name: "Редактировать Статья по имени" }));
    expect(screen.getByLabelText("Статьи теста")).toHaveValue(JSON.stringify(changed, null, 2));
    const writes = vi.mocked(aiTestRequest).mock.calls.filter((call) => call[3]?.method);
    expect(writes.map((call) => [call[2], call[3]?.method])).toEqual([["tests", "POST"], ["tests/inline-test", "PUT"]]);
    expect(JSON.parse(writes[0][3]!.body as string).scenarios[0].knowledge_articles).toEqual(articles);
    expect(JSON.parse(writes[1][3]!.body as string).scenario.knowledge_articles).toEqual(changed);
  });
  it("explains a temporary save lock, preserves the draft, and resumes saving once the lock clears", async () => {
    const items = savedTests();
    let finishSave!: (value: unknown) => void;
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "PUT") return new Promise((resolve) => { finishSave = resolve; });
      return { items: path === "tests" ? items : [] };
    });
    const rendered = render(<AgentTests auth={auth} profile={profile} />);
    const first = await screen.findByRole("region", { name: "Тест Первый" });
    fireEvent.click(within(first).getByRole("button", { name: "Редактировать Первый" }));
    const title = within(first).getByRole("textbox", { name: "Название" });
    fireEvent.change(title, { target: { value: "Сохранить после ожидания" } });
    const reason = "Сохраняется инструкция. Дождитесь завершения.";
    rendered.rerender(<AgentTests auth={auth} profile={profile} disabledReason={reason} />);
    const blockedSave = within(first).getByRole("button", { name: "Подождите…" });
    expect(blockedSave).toBeDisabled();
    expect(within(first).getByRole("status")).toHaveTextContent(reason);
    expect(title).toHaveValue("Сохранить после ожидания");
    fireEvent.click(blockedSave);
    expect(vi.mocked(aiTestRequest).mock.calls.filter((call) => call[3]?.method)).toHaveLength(0);
    rendered.rerender(<AgentTests auth={auth} profile={profile} />);
    expect(within(first).queryByText(reason)).not.toBeInTheDocument();
    expect(within(first).getByRole("textbox", { name: "Название" })).toBe(title);
    expect(title).toHaveValue("Сохранить после ожидания");
    const save = within(first).getByRole("button", { name: "Сохранить тест" });
    expect(save).toBeEnabled();
    fireEvent.click(save);
    const pendingSave = within(first).getByRole("button", { name: "Подождите…" });
    expect(pendingSave).toBeDisabled();
    expect(within(first).getByRole("status")).toHaveTextContent("Дождитесь завершения текущей операции с тестами.");
    fireEvent.click(pendingSave);
    expect(vi.mocked(aiTestRequest).mock.calls.filter((call) => call[3]?.method === "PUT")).toHaveLength(1);
    await act(async () => finishSave({}));
    await screen.findByText("Тест сохранён");
    expect(aiTestRequest).toHaveBeenCalledWith(auth, profile.id, "tests/test-1", {
      method: "PUT", body: JSON.stringify({ revision: items[0].revision, scenario: { ...items[0].scenario, name: "Сохранить после ожидания" } }),
    });
    expect(screen.queryByRole("button", { name: "Подождите…" })).not.toBeInTheDocument();
  });
  it.each([400, 409])("shows the exact API %s save error next to its button and preserves the draft during retry", async (status) => {
    const items = savedTests();
    const detail = status === 400 ? "invalid scenario language" : "scenario was changed or deleted; reload it";
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "PUT") {
        if (status === 409 || JSON.parse(options.body as string).scenario.language === "") throw Object.assign(new Error(detail), { status });
        return {};
      }
      return { items: path === "tests" ? items : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    const first = await screen.findByRole("region", { name: "Тест Первый" });
    fireEvent.click(within(first).getByRole("button", { name: "Редактировать Первый" }));
    const title = within(first).getByRole("textbox", { name: "Название" });
    fireEvent.change(title, { target: { value: "Несохранённый сценарий" } });
    const language = within(first).getByRole("textbox", { name: "Язык" });
    if (status === 400) fireEvent.change(language, { target: { value: "" } });
    const save = within(first).getByRole("button", { name: "Сохранить тест" });
    const heading = save.closest<HTMLElement>(".agent-test-editor-heading")!;
    fireEvent.click(save);
    const error = await within(heading).findByRole("alert");
    expect(error).toHaveTextContent(detail);
    expect(screen.getAllByText(detail)).toHaveLength(1);
    expect(save).toBeEnabled();
    expect(error.id).not.toBe("");
    expect(save.getAttribute("aria-describedby")?.split(/\s+/)).toContain(error.id);
    expect(save).toHaveAccessibleDescription(detail);
    expect(within(first).getByRole("textbox", { name: "Название" })).toBe(title);
    expect(title).toHaveValue("Несохранённый сценарий");
    expect(language).toHaveValue(status === 400 ? "" : "ru");
    expect(screen.queryByText("Тест сохранён")).not.toBeInTheDocument();
    if (status === 400) fireEvent.change(language, { target: { value: "ru" } });
    fireEvent.click(save);
    if (status === 400) {
      await screen.findByText("Тест сохранён");
      expect(screen.queryByText(detail)).not.toBeInTheDocument();
    } else {
      expect(await within(heading).findByRole("alert")).toHaveTextContent(detail);
      expect(title).toHaveValue("Несохранённый сценарий");
      expect(screen.queryByText("Тест сохранён")).not.toBeInTheDocument();
    }
    const writes = vi.mocked(aiTestRequest).mock.calls.filter((call) => call[3]?.method === "PUT");
    expect(writes).toHaveLength(2);
    expect(writes[1]).toEqual([auth, profile.id, "tests/test-1", {
      method: "PUT", body: JSON.stringify({ revision: items[0].revision, scenario: { ...items[0].scenario, name: "Несохранённый сценарий" } }),
    }]);
    expect(writes.map((call) => JSON.parse(call[3]?.body as string).revision)).toEqual([items[0].revision, items[0].revision]);
  });
  it("clears a previous scenario save error when opening another editor", async () => {
    const items = savedTests();
    const detail = "scenario was changed or deleted; reload it";
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "PUT") throw new Error(detail);
      return { items: path === "tests" ? items : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click(await screen.findByRole("button", { name: "Редактировать Первый" }));
    fireEvent.click(screen.getByRole("button", { name: "Сохранить тест" }));
    await screen.findByText(detail);
    fireEvent.click(screen.getByRole("button", { name: "Редактировать Второй" }));
    expect(screen.getByRole("textbox", { name: "Название" })).toHaveValue("Второй");
    expect(screen.queryByText(detail)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Сохранить тест" })).not.toHaveAccessibleDescription(detail);
  });
  it("keeps results under their test by ID and preserves history when a test is deleted", async () => {
    let items = savedTests();
    const history = items.map((item, index) => ({
      id: `run-${index + 1}`, scenario_id: item.id, status: index ? "behavior_error" : "passed", created_at: "2026-09-05T10:00:00Z",
      snapshot: { scenario: { name: "Старое одинаковое название" } }, result: {},
    }));
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "DELETE") { items = items.filter((item) => `tests/${item.id}` !== path); return {}; }
      return { items: path === "tests" ? items : history };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    const first = await screen.findByRole("region", { name: "Тест Первый" });
    const second = screen.getByRole("region", { name: "Тест Второй" });
    expect(screen.queryByRole("region", { name: /^История запусков:/ })).not.toBeInTheDocument();
    const firstEye = within(first).getByRole("button", { name: "Показать историю запусков: Первый" });
    const secondEye = within(second).getByRole("button", { name: "Показать историю запусков: Второй" });
    expect(firstEye).toHaveAttribute("aria-expanded", "false");
    expect(within(first).getByRole("button", { name: "Запустить Первый" }).nextElementSibling).toBe(firstEye);
    expect(firstEye.nextElementSibling).toBe(within(first).getByRole("button", { name: "Редактировать Первый" }));
    fireEvent.click(firstEye);
    expect(firstEye).toHaveAttribute("aria-expanded", "true");
    expect(within(second).queryByRole("region", { name: "История запусков: Второй" })).not.toBeInTheDocument();
    fireEvent.click(secondEye);
    expect(within(first).getByRole("img", { name: "Пройдено" })).toBeInTheDocument();
    expect(within(first).queryByRole("img", { name: "Ошибка поведения" })).not.toBeInTheDocument();
    expect(within(second).getByRole("img", { name: "Ошибка поведения" })).toBeInTheDocument();
    expect(within(second).queryByRole("img", { name: "Пройдено" })).not.toBeInTheDocument();
    fireEvent.click(firstEye);
    expect(within(first).queryByRole("region", { name: "История запусков: Первый" })).not.toBeInTheDocument();
    expect(within(second).getByRole("region", { name: "История запусков: Второй" })).toBeInTheDocument();
    expect(vi.mocked(aiTestRequest).mock.calls.every((args) => !args[3]?.method)).toBe(true);
    const clear = screen.getByRole("button", { name: "Очистить историю запуска тестов" });
    expect(clear.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(within(first).getByLabelText("Действия с тестом Первый"));
    fireEvent.click(within(first).getByRole("button", { name: "Удалить Первый" }));
    await screen.findByText("Удалено тестов: 1. История запусков сохранена");
    expect(screen.queryByRole("region", { name: "Тест Первый" })).not.toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Запуски удалённых тестов" })).getByRole("img", { name: "Пройдено" })).toBeInTheDocument();
    expect(within(second).getByRole("img", { name: "Ошибка поведения" })).toBeInTheDocument();
  });
  it("launches without mode selection and shows the run under its test", async () => {
    const items = savedTests();
    let started = false;
    const run = { id: "run-2", scenario_id: "test-2", status: "passed", created_at: "2026-09-05T10:00:00Z", snapshot: { scenario: items[1].scenario }, result: {} };
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "POST") { started = true; return { id: run.id }; }
      return { items: path === "tests" ? items : started ? [run] : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    const second = await screen.findByRole("region", { name: "Тест Второй" });
    expect(screen.queryByRole("combobox", { name: "Режим запуска" })).not.toBeInTheDocument();
    expect(within(second).queryByRole("region", { name: "История запусков: Второй" })).not.toBeInTheDocument();
    fireEvent.click(within(second).getByRole("button", { name: "Запустить Второй" }));
    await screen.findByText("Запуски завершены");
    expect(within(second).getByRole("img", { name: "Пройдено" })).toBeInTheDocument();
    expect(within(second).getByRole("button", { name: "Скрыть историю запусков: Второй" })).toHaveAttribute("aria-expanded", "true");
    expect(within(screen.getByRole("region", { name: "Тест Первый" })).queryByRole("img", { name: "Пройдено" })).not.toBeInTheDocument();
    expect(aiTestRequest).toHaveBeenCalledWith(auth, profile.id, "test-runs", { method: "POST", body: JSON.stringify({ scenario_id: "test-2" }) });
  });

  it("clears the launch progress notice when starting a test fails", async () => {
    const items = savedTests();
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "POST") throw new Error("Не удалось запустить тест");
      return { items: path === "tests" ? items : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click(await screen.findByRole("button", { name: "Запустить Первый" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось запустить тест");
    expect(screen.queryByText(/Запуск 1 из 1/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Запустить Первый" })).toBeEnabled();
  });
  it.each(["history", "workspace"])("preserves source drafts when hiding and reopening the %s without reloading or saving them", async (panel) => {
    const items = savedTests();
    const run = { id: "draft-run", scenario_id: items[0].id, scenario_revision: 1, status: "passed", created_at: "2026-09-09T10:00:00Z", snapshot: { scenario: items[0].scenario, profile: { instructions: "Instructions at run time" } }, result: { trace: [] } };
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path) => {
      if (path === "tests") return { items };
      if (path === "test-runs") return { items: [run] };
      if (path === "test-runs/draft-run") return run;
      if (path === "test-runs/draft-run/sources/instructions") return { text: "Current saved instructions", revision: "current-revision" };
      throw new Error(`Unexpected path: ${path}`);
    });
    render(<AgentTests auth={auth} profile={profile} />);
    const first = await screen.findByRole("region", { name: "Тест Первый" });
    expect(aiTestRequest).toHaveBeenCalledTimes(2);
    fireEvent.click(within(first).getByRole("button", { name: "Показать историю запусков: Первый" }));
    expect(aiTestRequest).toHaveBeenCalledTimes(2);
    fireEvent.click(first.querySelector(".agent-test-run-summary")!);
    fireEvent.click(within(first).getByRole("button", { name: "Использованные инструкции" }));
    const editor = await within(first).findByRole("textbox", { name: "Текст для следующих запусков" });
    fireEvent.change(editor, { target: { value: "Unsaved source draft" } });
    const instructionsTrigger = within(first).getByRole("button", { name: "Использованные инструкции" });
    fireEvent.click(within(first).getByRole("button", { name: panel === "history" ? "Скрыть историю запусков: Первый" : "Закрыть инструкции и рекомендации" }));
    expect(within(first).queryByRole("region", { name: panel === "history" ? "История запусков: Первый" : "Инструкции и рекомендации" })).not.toBeInTheDocument();
    expect(editor).not.toBeVisible();
    if (panel === "workspace") expect(instructionsTrigger).toHaveFocus();
    fireEvent.click(panel === "history" ? within(first).getByRole("button", { name: "Показать историю запусков: Первый" }) : instructionsTrigger);
    expect(within(first).getByRole("textbox", { name: "Текст для следующих запусков" })).toBe(editor);
    expect(editor).toHaveValue("Unsaved source draft");
    expect(editor).toBeVisible();
    expect(vi.mocked(aiTestRequest).mock.calls.filter((call) => call[2] === "test-runs/draft-run/sources/instructions")).toHaveLength(1);
    expect(vi.mocked(aiTestRequest).mock.calls.every((call) => !call[3]?.method)).toBe(true);
  });
  it.each([false, true])("clears only one test's completed history and preserves results on failure (failure: %s)", async (failClear) => {
    const items = savedTests();
    let history = items.map((item, index) => ({
      id: `run-${index + 1}`, scenario_id: item.id, status: index ? "behavior_error" : "passed", created_at: "2026-09-05T10:00:00Z",
      snapshot: { scenario: item.scenario }, result: {},
    }));
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "DELETE") {
        if (failClear) throw new Error("Не удалось очистить историю");
        history = history.filter((run) => `tests/${run.scenario_id}/runs` !== path);
        return { deleted: 1 };
      }
      return { items: path === "tests" ? items : history };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    const first = await screen.findByRole("region", { name: "Тест Первый" });
    const second = screen.getByRole("region", { name: "Тест Второй" });
    fireEvent.click(within(first).getByRole("button", { name: "Показать историю запусков: Первый" }));
    fireEvent.click(within(second).getByRole("button", { name: "Показать историю запусков: Второй" }));
    fireEvent.click(within(first).getByRole("button", { name: "Очистить историю запусков: Первый" }));
    if (failClear) {
      expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось очистить историю");
      expect(within(first).getByRole("img", { name: "Пройдено" })).toBeInTheDocument();
    } else {
      await screen.findByText("История завершённых запусков теста «Первый» очищена");
      expect(within(first).queryByRole("img", { name: "Пройдено" })).not.toBeInTheDocument();
      expect(within(first).getByText("Запусков пока нет.")).toBeInTheDocument();
      expect(within(first).getByRole("button", { name: "Очистить историю запусков: Первый" })).toBeDisabled();
    }
    expect(within(second).getByRole("img", { name: "Ошибка поведения" })).toBeInTheDocument();
    expect(within(first).getByRole("button", { name: "Запустить Первый" })).toBeEnabled();
    expect(vi.mocked(aiTestRequest).mock.calls.filter((args) => args[3]?.method === "DELETE").map((args) => args[2])).toEqual(["tests/test-1/runs"]);
  });
  it("can show an empty test history without launching a test", async () => {
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path) => ({ items: path === "tests" ? savedTests() : [] }));
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click(await screen.findByRole("button", { name: "Показать историю запусков: Первый" }));
    expect(screen.getByText("Запусков пока нет.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Очистить историю запусков: Первый" })).toBeDisabled();
    expect(vi.mocked(aiTestRequest).mock.calls.every((args) => !args[3]?.method)).toBe(true);
  });
  it("loads used instructions only on demand and retries failed trace and source loads separately", async () => {
    const run = { id: "saved-run", status: "passed", created_at: "2026-09-09T10:00:00Z", snapshot: { scenario: { name: "Проверка инструкции" }, profile: { instructions: "Инструкция в запуске" } }, result: { trace: [] } };
    let detailAttempts = 0;
    let sourceAttempts = 0;
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path) => {
      if (path === "test-runs/saved-run") {
        if (++detailAttempts === 1) throw new Error("Unavailable");
        return run;
      }
      if (path === "test-runs/saved-run/sources/instructions") {
        if (++sourceAttempts === 1) throw new Error("Forbidden");
        return { text: "Текущая инструкция", revision: "current-1" };
      }
      return { items: path === "test-runs" ? [run] : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click((await screen.findByRole("img", { name: "Пройдено" })).closest("summary")!);
    expect(detailAttempts).toBe(0);
    expect(sourceAttempts).toBe(0);
    expect(screen.getByRole("button", { name: "Рекомендации по инструкциям" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Использованные инструкции" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось загрузить трассировку и инструкции");
    expect(sourceAttempts).toBe(0);
    fireEvent.click(screen.getByRole("button", { name: "Повторить загрузку" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось загрузить текущую версию");
    expect(screen.getByRole("region", { name: "Текст в запуске" })).toHaveTextContent("Инструкция в запуске");
    expect(screen.queryByRole("textbox", { name: "Текст для следующих запусков" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Повторить загрузку" }));
    expect(await screen.findByRole("textbox", { name: "Текст для следующих запусков" })).toHaveValue("Текущая инструкция");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(detailAttempts).toBe(2);
    expect(sourceAttempts).toBe(2);
    expect(vi.mocked(aiTestRequest).mock.calls.every((call) => !call[3]?.method)).toBe(true);
  });
  it.each([false, true])("discards run details across auth changes and permits a fresh load (old load pending: %s)", async (pending) => {
    const nextAuth = { kind: "session" } as const;
    const run = { id: "scoped-run", status: "passed", created_at: "2026-09-09T10:00:00Z", snapshot: { scenario: { name: "Доступ к инструкциям" } }, result: {} };
    const detail = (session: "old" | "new") => ({
      ...run, snapshot: { ...run.snapshot, profile: { instructions: `${session} recorded instructions` } },
      result: { trace: [{ step: 0, at: 0, call: { tool: "http", parameters: { method: "GET", url: `https://${session}.example.test/status` } } }] },
    });
    let finishOld!: (value: unknown) => void;
    vi.mocked(aiTestRequest).mockImplementation(async (requestAuth, _profile, path) => {
      if (path === "test-runs/scoped-run") return requestAuth === auth && pending
        ? new Promise((resolve) => { finishOld = resolve; })
        : detail(requestAuth === auth ? "old" : "new");
      if (path === "test-runs/scoped-run/sources/instructions") return {
        text: requestAuth === auth ? "old current instructions" : "new current instructions", revision: "saved-revision",
      };
      return { items: path === "test-runs" ? [run] : [] };
    });
    const rendered = render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click((await screen.findByRole("img", { name: "Пройдено" })).closest("summary")!);
    fireEvent.click(screen.getByRole("button", { name: "Использованные инструкции" }));
    if (pending) expect(screen.getByRole("button", { name: "Загружаем трассировку…" })).toBeDisabled();
    else {
      expect(await screen.findByRole("textbox", { name: "Текст для следующих запусков" })).toHaveValue("old current instructions");
      expect(screen.getByRole("region", { name: "Текст в запуске" })).toHaveTextContent("old recorded instructions");
      fireEvent.click(screen.getByText("Трассировка (1)"));
      expect(screen.getByText("GET https://old.example.test/status")).toBeInTheDocument();
    }
    const oldRequest = vi.mocked(aiTestRequest).mock.calls.find((call) => call[0] === auth && call[2] === "test-runs/scoped-run")!;
    rendered.rerender(<AgentTests auth={nextAuth} profile={profile} />);
    expect(oldRequest[3]?.signal?.aborted).toBe(true);
    if (pending) await act(async () => finishOld(detail("old")));
    expect(screen.queryByText("old recorded instructions", { selector: "pre" })).not.toBeInTheDocument();
    expect(screen.queryByDisplayValue("old current instructions")).not.toBeInTheDocument();
    expect(screen.queryByText("GET https://old.example.test/status")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("img", { name: "Пройдено" }).closest("summary")!);
    expect(screen.getByRole("button", { name: "Загрузить трассу и снимок" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Использованные инструкции" }));
    expect(await screen.findByRole("textbox", { name: "Текст для следующих запусков" })).toHaveValue("new current instructions");
    expect(screen.getByRole("region", { name: "Текст в запуске" })).toHaveTextContent("new recorded instructions");
    fireEvent.click(screen.getByText("Трассировка (1)"));
    expect(screen.getByText("GET https://new.example.test/status")).toBeInTheDocument();
    expect(vi.mocked(aiTestRequest).mock.calls.filter((call) => call[2] === "test-runs/scoped-run").map((call) => call[0])).toEqual([auth, nextAuth]);
  });
  it("shows AI verdicts and explanations and keeps manual edits scoped to each step", async () => {
    const reviews = [
      { expected_meaning: "Сообщить paused без выдуманной причины", verdict: "pass", source: "ai", note: "Статус приостановлена указан верно; причина не выдумана, предложена помощь специалиста." },
      { expected_meaning: "Не объявлять заявку завершённой", verdict: "fail", source: "ai", note: "Ответ ошибочно объявляет заявку завершённой." },
    ];
    const history = [{ id: "run-1", status: "behavior_error", created_at: "2026-09-05T10:00:00Z", snapshot: { scenario: { name: "Приостановленная заявка" } }, result: {
      steps: reviews.map((_, index) => ({ message: `Вопрос ${index + 1}`, reply: index ? "Заявка завершена" : "Заявка приостановлена", at: index, operator_present: false, closed: false, timer_remaining: null })),
      semantic_review: reviews,
    } }];
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "POST") {
        const input = JSON.parse(options.body as string);
        reviews[input.step] = { ...reviews[input.step], verdict: input.verdict, note: input.note, source: "human" };
        return {};
      }
      return { items: path === "test-runs" ? history : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click((await screen.findByRole("img", { name: "Ошибка поведения" })).closest("summary")!);
    expect(screen.getAllByText("Оценка ИИ:")).toHaveLength(2);
    expect(screen.getByRole("img", { name: "Смысл верный" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Смысл неверный" })).toBeInTheDocument();
    expect(screen.getByText(reviews[0].note, { selector: "p" })).toBeInTheDocument();
    expect(screen.getByText(reviews[1].note, { selector: "p" })).toBeInTheDocument();
    const sections = screen.getAllByText("Изменить оценку вручную").map((summary) => {
      fireEvent.click(summary);
      return within(summary.closest("section")!);
    });
    fireEvent.change(sections[0].getByLabelText("Обоснование ручной оценки"), { target: { value: "Проверено вручную: смысл верный." } });
    expect(sections[1].getByLabelText("Обоснование ручной оценки")).toHaveValue(reviews[1].note);
    fireEvent.click(sections[0].getByRole("button", { name: "Смысл верный" }));
    await screen.findByText("Оценка сохранена");
    expect(aiTestRequest).toHaveBeenCalledWith(auth, profile.id, "test-runs/run-1/review", { method: "POST", body: JSON.stringify({ step: 0, verdict: "pass", note: "Проверено вручную: смысл верный." }) });
    expect(screen.getByText("Ручная оценка:")).toBeInTheDocument();
    expect(screen.getByText("Проверено вручную: смысл верный.", { selector: "p" })).toBeInTheDocument();
    expect(sections[1].getByLabelText("Обоснование ручной оценки")).toHaveValue(reviews[1].note);
  });
  it.each([false, true])("selects all and handles bulk deletion (failure: %s)", async (failSecond) => {
    const items = ["Первый", "Второй"].map((name, index) => ({
      id: `test-${index + 1}`, revision: 1, scenario: { name, language: "ru", steps: [] },
    }));
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "DELETE") {
        if (failSecond && path === "tests/test-2") throw new Error("Ошибка удаления");
        return {};
      }
      return { items: path === "tests" ? items : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    await screen.findByLabelText("Выбрать Первый");
    fireEvent.click(screen.getByRole("button", { name: "Выделить все" }));
    expect(screen.getByLabelText("Выбрать Первый")).toBeChecked();
    expect(screen.getByLabelText("Выбрать Второй")).toBeChecked();
    expect(screen.getByRole("button", { name: "Выбранные (2)" })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Снять выделение" }));
    expect(screen.getByLabelText("Выбрать Первый")).not.toBeChecked();
    expect(screen.getByRole("button", { name: "Удалить выбранные (0)" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Выделить все" }));
    fireEvent.click(screen.getByRole("button", { name: "Удалить выбранные (2)" }));
    await screen.findByText(`Удалено тестов: ${failSecond ? 1 : 2}. История запусков сохранена`);
    expect(screen.queryByLabelText("Выбрать Первый")).not.toBeInTheDocument();
    if (failSecond) {
      expect(screen.getByText("Ошибка удаления")).toBeInTheDocument();
      expect(screen.getByLabelText("Выбрать Второй")).toBeChecked();
      expect(screen.getByRole("button", { name: "Удалить выбранные (1)" })).toBeEnabled();
    } else {
      expect(screen.getByText("Тестов пока нет.")).toBeInTheDocument();
    }
    const deletions = vi.mocked(aiTestRequest).mock.calls.filter((args) => args[3]?.method === "DELETE");
    expect(deletions.map((args) => args[2])).toEqual(["tests/test-1", "tests/test-2"]);
  });
  it("allows selected scenarios to be deleted while an existing run continues and preserves its snapshot", async () => {
    const initialItems = savedTests();
    let items = [...initialItems, { ...initialItems[1], id: "test-3", scenario: { ...initialItems[1].scenario, name: "Третий" } }];
    const history = initialItems.map((item, index) => ({
      id: `run-${index + 1}`, scenario_id: item.id, scenario_revision: item.revision,
      status: index === 0 ? "running" : "passed", created_at: "2026-09-09T10:00:00Z",
      snapshot: { scenario: item.scenario }, result: {},
    }));
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "DELETE") { items = items.filter((item) => `tests/${item.id}` !== path); return {}; }
      return { items: path === "tests" ? items : history };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    await screen.findByLabelText("Выбрать Первый");
    expect(screen.getByRole("button", { name: "Удалить выбранные (0)" })).toBeDisabled();
    fireEvent.click(screen.getByLabelText("Выбрать Первый"));
    expect(screen.getByRole("button", { name: "Удалить выбранные (1)" })).toBeEnabled();
    fireEvent.click(screen.getByLabelText("Действия с тестом Первый"));
    expect(screen.getByRole("button", { name: "Удалить Первый" })).toBeEnabled();
    fireEvent.click(screen.getByLabelText("Выбрать Второй"));
    expect(screen.getByRole("button", { name: "Удалить выбранные (2)" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Выбранные (2)" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Удалить выбранные (2)" }));
    await screen.findByText("Удалено тестов: 2. История запусков сохранена");
    expect(screen.queryByLabelText("Выбрать Первый")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Выбрать Второй")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Выбрать Третий")).not.toBeChecked();
    const savedHistory = screen.getByRole("region", { name: "Запуски удалённых тестов" });
    expect(within(savedHistory).getByText("Первый")).toBeInTheDocument();
    expect(within(savedHistory).getByText("Второй")).toBeInTheDocument();
    expect(within(savedHistory).getByRole("img", { name: "Пройдено" })).toBeInTheDocument();
    const activeSummary = within(savedHistory).getByText("Выполняется").closest("summary")!;
    fireEvent.click(activeSummary);
    const activeRun = within(activeSummary.closest("details")!);
    fireEvent.click(activeRun.getByLabelText("Действия с запуском"));
    expect(activeRun.getByRole("button", { name: "Удалить запуск" })).toBeDisabled();
    expect(vi.mocked(aiTestRequest).mock.calls.filter((call) => call[3]?.method === "DELETE").map((call) => call[2])).toEqual(["tests/test-1", "tests/test-2"]);
  });
  it("blocks scenario deletion throughout a locally started launch and enables it again after completion", async () => {
    const items = savedTests();
    const completed = { id: "local-run", scenario_id: items[0].id, scenario_revision: 1, status: "passed", created_at: "2026-09-09T10:00:00Z", snapshot: { scenario: items[0].scenario }, result: {} };
    let finishStart!: (value: { id: string }) => void;
    let finishPoll!: (value: { items: typeof completed[] }) => void;
    let started = false;
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (path === "tests") return { items };
      if (path === "test-runs" && options?.method === "POST") return new Promise<{ id: string }>((resolve) => { finishStart = resolve; });
      if (path === "test-runs" && started) return new Promise<{ items: typeof completed[] }>((resolve) => { finishPoll = resolve; });
      return { items: [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click(await screen.findByLabelText("Выбрать Первый"));
    fireEvent.click(screen.getByLabelText("Действия с тестом Первый"));
    const singleDelete = screen.getByRole("button", { name: "Удалить Первый" });
    const bulkDelete = screen.getByRole("button", { name: "Удалить выбранные (1)" });
    expect(singleDelete).toBeEnabled();
    expect(bulkDelete).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Выбранные (1)" }));
    expect(singleDelete).toBeDisabled();
    expect(bulkDelete).toBeDisabled();
    await act(async () => { started = true; finishStart({ id: completed.id }); });
    expect(finishPoll).toBeTypeOf("function");
    expect(singleDelete).toBeDisabled();
    expect(bulkDelete).toBeDisabled();
    expect(vi.mocked(aiTestRequest).mock.calls.some((call) => call[3]?.method === "DELETE")).toBe(false);
    await act(async () => finishPoll({ items: [completed] }));
    await screen.findByText("Запуски завершены");
    expect(singleDelete).toBeEnabled();
    expect(bulkDelete).toBeEnabled();
  });
  it.each([false, true])("deletes run history (clear all: %s)", async (clearAll) => {
    let history = [{ id: "run-1", status: "passed", created_at: "2026-09-05T10:00:00Z", snapshot: { scenario: { name: "Приветствие" } }, result: {} }];
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (options?.method === "DELETE") { history = []; return { deleted: 1 }; }
      return { items: path === "test-runs" ? history : [] };
    });
    render(<AgentTests auth={auth} profile={profile} />);
    const summary = (await screen.findByRole("img", { name: "Пройдено" })).closest("summary")!;
    if (!clearAll) {
      fireEvent.click(summary);
      fireEvent.click(screen.getByLabelText("Действия с запуском"));
    }
    fireEvent.click(screen.getByRole("button", { name: clearAll ? "Очистить историю запуска тестов" : "Удалить запуск" }));
    await screen.findByText(clearAll ? "История завершённых запусков очищена" : "Запуск удалён из истории");
    expect(screen.queryByRole("img", { name: "Пройдено" })).not.toBeInTheDocument();
    const deletions = vi.mocked(aiTestRequest).mock.calls.filter((args) => args[3]?.method === "DELETE");
    expect(deletions.map((args) => args[2])).toEqual([clearAll ? "test-runs" : "test-runs/run-1"]);
  });
  it("protects active runs from deletion", async () => {
    vi.mocked(aiTestRequest).mockImplementation(async (_auth, _profile, path) => ({ items: path === "test-runs" ? [{ id: "active", status: "running", snapshot: { scenario: { name: "Активный" } }, result: {} }] : [] }));
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click((await screen.findByText("Выполняется")).closest("summary")!);
    fireEvent.click(screen.getByLabelText("Действия с запуском"));
    expect(screen.getByRole("button", { name: "Удалить запуск" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Очистить историю запуска тестов" })).toBeDisabled();
  });
  it("bulk-adds one saved scenario per nonempty question without submitting agent settings", async () => {
    const submitted = vi.fn((event: React.FormEvent) => event.preventDefault());
    render(<form onSubmit={submitted}><AgentTests auth={auth} profile={profile} /></form>);
    await screen.findByText("Тестов пока нет.");
    fireEvent.change(screen.getByLabelText("Каждый вопрос с новой строки"), { target: { value: "Первый вопрос\n\nВторой вопрос" } });
    fireEvent.click(screen.getByRole("button", { name: "Добавить вопросы" }));
    await waitFor(() => expect(aiTestRequest).toHaveBeenCalledWith(auth, profile.id, "tests", expect.objectContaining({ method: "POST" })));
    const call = vi.mocked(aiTestRequest).mock.calls.find((args) => args[3]?.method === "POST");
    const body = JSON.parse(call?.[3]?.body as string);
    expect(body.scenarios.map((item: { steps: { message: string }[] }) => item.steps[0].message)).toEqual(["Первый вопрос", "Второй вопрос"]);
    expect(submitted).not.toHaveBeenCalled();
  });
  it("creates a multi-step scenario and rejects malformed advanced JSON before saving", async () => {
    render(<AgentTests auth={auth} profile={profile} />);
    fireEvent.click(screen.getByRole("button", { name: "Новый тест" }));
    fireEvent.change(screen.getByLabelText("Название"), { target: { value: "Оператор" } });
    fireEvent.click(screen.getByRole("button", { name: "Добавить шаг" }));
    expect(screen.getByText("Шаг 2")).toBeInTheDocument();
    const fixtures = screen.getByLabelText("Подготовленные ответы");
    fireEvent.change(fixtures, { target: { value: "invalid json" } });
    const save = screen.getByRole("button", { name: "Сохранить тест" });
    fireEvent.click(save);
    const error = await within(save.closest<HTMLElement>(".agent-test-editor-heading")!).findByRole("alert");
    expect(error).toHaveTextContent("Исправьте JSON перед сохранением");
    expect(save.getAttribute("aria-describedby")?.split(/\s+/)).toContain(error.id);
    expect(save).toHaveAccessibleDescription("Исправьте JSON перед сохранением");
    expect(screen.getByLabelText("Название")).toHaveValue("Оператор");
    expect(fixtures).toBeInTheDocument();
    expect(fixtures).toHaveValue("invalid json");
    expect(vi.mocked(aiTestRequest).mock.calls.some((args) => args[3]?.method === "POST")).toBe(false);
  });
});
