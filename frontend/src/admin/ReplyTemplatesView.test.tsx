import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import type { ChatMessageEditorProps } from "./ChatMessageEditor";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";
import { usePageRoute } from "./page-route";
import { ReplyTemplatesView } from "./ReplyTemplatesView";

vi.mock("./ChatMessageEditor", () => ({
  default: ({ value, onChange, label, disabled, aiDraftSource }: ChatMessageEditorProps) => (
    <>
      <textarea aria-label={label} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
      {aiDraftSource && <button type="button" onClick={() => void aiDraftSource.generate("agent-1", value, new AbortController().signal).then(onChange)}>Draft with agent-1</button>}
    </>
  ),
}));

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  managementApiRequest: vi.fn(),
  listReplyTemplates: vi.fn(),
  createReplyTemplate: vi.fn(),
  updateReplyTemplate: vi.fn(),
  deleteReplyTemplate: vi.fn(),
  generateReplyTemplateDraft: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const inboxes: api.Inbox[] = [
  { id: "inbox-1", project_id: "project-1", name: "Support", status: "active", created_at: "2026-09-01T00:00:00Z" },
  { id: "inbox-2", project_id: "project-1", name: "Sales", status: "active", created_at: "2026-09-01T00:00:00Z" },
];
const first: api.ReplyTemplate = {
  id: "template-1", inbox_id: "inbox-1", title: "Проверка оплаты", body: "**Проверяю** поступление оплаты.", body_format: "markdown", created_at: "2026-09-01T00:00:00Z", updated_at: "2026-09-01T00:00:00Z",
};
const second: api.ReplyTemplate = { ...first, id: "template-2", title: "Завершение", body: "Спасибо за обращение!" };

/** Moves the page URL as Back/Forward or a pasted link would. */
function RouteTo({ segments }: { segments: string[] }) {
  const route = usePageRoute();
  return <button type="button" onClick={() => void route.navigate(segments)}>{`Open /${segments.join("/")}`}</button>;
}

function view(options: { canManage?: boolean; activeInboxId?: string | null; onInboxChange?: (id: string) => void; onDirtyChange?: (dirty: boolean) => void; segments?: string[]; moves?: string[][] } = {}) {
  const page = <ReplyTemplatesView auth={auth} inboxes={inboxes} activeInboxId={options.activeInboxId === undefined ? "inbox-1" : options.activeInboxId} onInboxChange={options.onInboxChange ?? vi.fn()} canManage={options.canManage ?? true} onDirtyChange={options.onDirtyChange} />;
  return <I18nContext.Provider value={createI18n("ru", vi.fn())}>{options.segments ? <MemoryPageRoute initialSegments={options.segments}>{page}<CurrentPageRoute />{options.moves?.map((move) => <RouteTo key={move.join("/")} segments={move} />)}</MemoryPageRoute> : page}</I18nContext.Provider>;
}

const pageRoute = () => screen.getByTestId("page-route");

describe("reply template management", () => {
  beforeEach(() => {
    vi.mocked(api.managementApiRequest).mockResolvedValue({ projects: [], departments: [] });
    vi.mocked(api.listReplyTemplates).mockResolvedValue([first, second]);
    vi.mocked(api.deleteReplyTemplate).mockResolvedValue(undefined);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("creates formatted replies, retains a failed edit, and confirms deletion", async () => {
    render(view());
    await screen.findByRole("textbox", { name: "Текст ответа" });
    fireEvent.click(screen.getByRole("button", { name: "Создать шаблон" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Название" }), { target: { value: "  Ожидание  " } });
    fireEvent.change(screen.getByRole("textbox", { name: "Текст ответа" }), { target: { value: "**Уточняю статус.**\n\n- Оплата\n- Выплата" } });
    const created = { ...first, id: "template-new", title: "Ожидание", body: "**Уточняю статус.**\n\n- Оплата\n- Выплата" };
    vi.mocked(api.createReplyTemplate).mockResolvedValueOnce(created);
    fireEvent.click(screen.getByRole("button", { name: "Сохранить шаблон" }));
    expect(await screen.findByText("Шаблон сохранён.")).toBeInTheDocument();
    expect(api.createReplyTemplate).toHaveBeenCalledExactlyOnceWith(auth, "inbox-1", { title: "Ожидание", body: created.body });
    expect(screen.getByRole("textbox", { name: "Текст ответа" })).toHaveValue(created.body);

    vi.mocked(api.updateReplyTemplate).mockRejectedValueOnce(new Error("Unavailable"));
    fireEvent.change(screen.getByRole("textbox", { name: "Текст ответа" }), { target: { value: "**Новый ответ**" } });
    fireEvent.click(screen.getByRole("button", { name: "Сохранить шаблон" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось сохранить шаблон");
    expect(screen.getByRole("textbox", { name: "Текст ответа" })).toHaveValue("**Новый ответ**");
    expect(api.updateReplyTemplate).toHaveBeenCalledExactlyOnceWith(auth, "inbox-1", "template-new", { title: "Ожидание", body: "**Новый ответ**" });

    fireEvent.click(screen.getByRole("button", { name: "Удалить шаблон" }));
    expect(api.deleteReplyTemplate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Отмена" }));
    expect(screen.getByRole("textbox", { name: "Текст ответа" })).toHaveValue("**Новый ответ**");
    fireEvent.click(screen.getByRole("button", { name: "Удалить шаблон" }));
    fireEvent.click(screen.getByRole("button", { name: "Удалить" }));
    expect(await screen.findByText("Шаблон удалён.")).toBeInTheDocument();
    expect(api.deleteReplyTemplate).toHaveBeenCalledExactlyOnceWith(auth, "inbox-1", "template-new");
    expect(screen.queryByRole("button", { name: "Ожидание" })).not.toBeInTheDocument();
  });

  it("offers an AI draft once the template has a title", async () => {
    const generated = "Здравствуйте! Обработка по этому направлению занимает до 30 минут.";
    vi.mocked(api.generateReplyTemplateDraft).mockResolvedValueOnce({ body: generated, body_format: "markdown" });
    render(view());
    await screen.findByRole("textbox", { name: "Текст ответа" });
    fireEvent.click(screen.getByRole("button", { name: "Создать шаблон" }));
    expect(screen.queryByRole("button", { name: "Draft with agent-1" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Название" }), { target: { value: "Обмен USDT — Monero" } });
    fireEvent.click(screen.getByRole("button", { name: "Draft with agent-1" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Текст ответа" })).toHaveValue(generated));
    expect(api.generateReplyTemplateDraft).toHaveBeenCalledExactlyOnceWith(auth, "inbox-1", "agent-1", "Обмен USDT — Monero", "", expect.any(AbortSignal));
    expect(api.createReplyTemplate).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Сохранить шаблон" })).toBeEnabled();
  });

  it("asks before discarding edits when selecting another reply or Inbox", async () => {
    const onInboxChange = vi.fn();
    render(view({ onInboxChange }));
    const editor = await screen.findByRole("textbox", { name: "Текст ответа" });
    fireEvent.change(editor, { target: { value: "Несохранённый ответ" } });
    fireEvent.click(screen.getByRole("button", { name: "Завершение" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Отменить несохранённые изменения?");
    fireEvent.click(screen.getByRole("button", { name: "Продолжить редактирование" }));
    expect(editor).toHaveValue("Несохранённый ответ");
    fireEvent.change(screen.getByRole("combobox", { name: "Inbox" }), { target: { value: "inbox-2" } });
    expect(onInboxChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Отменить изменения" }));
    expect(onInboxChange).toHaveBeenCalledExactlyOnceWith("inbox-2");
  });

  it("searches reply contents and exposes a safe read-only preview without management controls", async () => {
    render(view({ canManage: false }));
    expect(await screen.findByText("Проверяю", { selector: "strong" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Текст ответа" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Создать шаблон" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Сохранить шаблон" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Удалить шаблон" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Поиск шаблонов" }), { target: { value: "обращение" } });
    expect(screen.queryByRole("button", { name: "Проверка оплаты" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Завершение" }));
    expect(screen.getByRole("heading", { name: "Завершение" })).toBeInTheDocument();
    expect(api.createReplyTemplate).not.toHaveBeenCalled();
    expect(api.updateReplyTemplate).not.toHaveBeenCalled();
    expect(api.deleteReplyTemplate).not.toHaveBeenCalled();
  });

  it("opens the template named by the URL and keeps the URL current", async () => {
    const onInboxChange = vi.fn();
    const created = { ...first, id: "template-new", title: "Ожидание", body: "Уточняю статус." };
    vi.mocked(api.createReplyTemplate).mockResolvedValueOnce(created);
    render(view({ segments: ["template-2"], onInboxChange }));
    expect(await screen.findByRole("textbox", { name: "Название" })).toHaveValue("Завершение");
    expect(screen.getByRole("button", { name: "Завершение" })).toHaveAttribute("aria-current", "true");
    expect(pageRoute().textContent).toBe("template-2");

    fireEvent.click(screen.getByRole("button", { name: "Проверка оплаты" }));
    expect(screen.getByRole("textbox", { name: "Название" })).toHaveValue("Проверка оплаты");
    expect(pageRoute().textContent).toBe("template-1");
    fireEvent.click(screen.getByRole("button", { name: "Создать шаблон" }));
    expect(screen.getByRole("heading", { name: "Новый шаблон" })).toBeInTheDocument();
    expect(pageRoute().textContent).toBe("new");
    fireEvent.change(screen.getByRole("textbox", { name: "Название" }), { target: { value: "Ожидание" } });
    fireEvent.change(await screen.findByRole("textbox", { name: "Текст ответа" }), { target: { value: "Уточняю статус." } });
    fireEvent.click(screen.getByRole("button", { name: "Сохранить шаблон" }));
    expect(await screen.findByText("Шаблон сохранён.")).toBeInTheDocument();
    expect(pageRoute().textContent).toBe("template-new");
    expect(screen.getByRole("heading", { name: "Ожидание" })).toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: "Inbox" }), { target: { value: "inbox-2" } });
    expect(onInboxChange).toHaveBeenCalledExactlyOnceWith("inbox-2");
    expect(pageRoute().textContent).toBe("");
  });

  it.each([
    [{ segments: ["missing"] }, "", "Проверка оплаты"],
    [{ segments: ["template-2", "extra"] }, "template-2", "Завершение"],
    [{ segments: ["new"], canManage: false }, "", "Проверка оплаты"],
  ])("replaces the stale template URL %j with the template it shows", async (options, canonical, title) => {
    render(view(options));
    expect(await screen.findByRole("heading", { name: title })).toBeInTheDocument();
    await waitFor(() => expect(pageRoute().textContent).toBe(canonical));
  });

  it("discards unsaved edits when Back or Forward changes the URL", async () => {
    const onDirtyChange = vi.fn();
    render(view({ segments: [], onDirtyChange, moves: [["template-1"], ["template-2"]] }));
    const editor = await screen.findByRole("textbox", { name: "Текст ответа" });
    fireEvent.change(editor, { target: { value: "Несохранённый ответ" } });
    expect(onDirtyChange).toHaveBeenLastCalledWith(true);

    fireEvent.click(screen.getByRole("button", { name: "Open /template-1" }));
    expect(screen.getByRole("textbox", { name: "Текст ответа" })).toHaveValue(first.body);
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
    fireEvent.change(screen.getByRole("textbox", { name: "Текст ответа" }), { target: { value: "Несохранённый ответ" } });
    fireEvent.click(screen.getByRole("button", { name: "Завершение" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Отменить несохранённые изменения?");
    fireEvent.click(screen.getByRole("button", { name: "Open /template-2" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Название" })).toHaveValue("Завершение");
    expect(screen.getByRole("textbox", { name: "Текст ответа" })).toHaveValue(second.body);
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
  });

  it("aborts a stale Inbox request and keeps the new Inbox selection", async () => {
    let finishOld!: (items: api.ReplyTemplate[]) => void;
    vi.mocked(api.listReplyTemplates).mockImplementationOnce(() => new Promise((resolve) => { finishOld = resolve; }));
    const { rerender } = render(view());
    const oldSignal = vi.mocked(api.listReplyTemplates).mock.calls[0][2];
    const sales = { ...second, id: "sales-template", inbox_id: "inbox-2", title: "Продажи" };
    vi.mocked(api.listReplyTemplates).mockResolvedValueOnce([sales]);
    rerender(view({ activeInboxId: "inbox-2" }));
    expect(oldSignal?.aborted).toBe(true);
    expect(await screen.findByRole("textbox", { name: "Название" })).toHaveValue("Продажи");
    await act(async () => finishOld([first]));
    expect(screen.getByRole("textbox", { name: "Название" })).toHaveValue("Продажи");
    expect(screen.queryByRole("button", { name: /Проверка оплаты/ })).not.toBeInTheDocument();
    await waitFor(() => expect(api.listReplyTemplates).toHaveBeenCalledWith(auth, "inbox-2", expect.any(AbortSignal)));
  });
});
