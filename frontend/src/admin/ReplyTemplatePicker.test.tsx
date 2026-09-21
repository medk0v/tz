import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import { ReplyTemplatePicker, type ReplyTemplatePickerProps } from "./ReplyTemplatePicker";

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  listReplyTemplates: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const templates: api.ReplyTemplate[] = [
  { id: "payment", inbox_id: "support", title: "Проверка оплаты", body: "**Проверяю** поступление перевода.", body_format: "markdown", created_at: "2026-09-03T00:00:00Z", updated_at: "2026-09-03T00:00:00Z" },
  { id: "welcome", inbox_id: "support", title: "Приветствие", body: "Здравствуйте! Чем можем помочь?", body_format: "markdown", created_at: "2026-09-03T00:00:00Z", updated_at: "2026-09-03T00:00:00Z" },
];

function picker(props: Partial<ReplyTemplatePickerProps> = {}) {
  return (
    <I18nContext.Provider value={createI18n("ru", vi.fn())}>
      <ReplyTemplatePicker auth={auth} inboxId="support" disabled={false} onInsert={vi.fn(() => true)} {...props} />
    </I18nContext.Provider>
  );
}

async function openPicker() {
  fireEvent.click(screen.getByRole("button", { name: "Шаблоны" }));
  await screen.findByRole("button", { name: "Вставить «Проверка оплаты»" });
}

describe("reply template picker", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listReplyTemplates).mockResolvedValue(templates);
  });
  afterEach(cleanup);

  it("searches titles and reply text, inserts the original Markdown, and does not submit the chat", async () => {
    const onInsert = vi.fn(() => true);
    const onSubmit = vi.fn((event) => event.preventDefault());
    render(<form onSubmit={onSubmit}>{picker({ onInsert })}</form>);
    await openPicker();
    expect(api.listReplyTemplates).toHaveBeenCalledWith(auth, "support", expect.any(AbortSignal));
    const search = screen.getByRole("searchbox", { name: "Поиск шаблонов" });
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "  ПРИВЕТ  " } });
    expect(screen.getByRole("button", { name: "Вставить «Приветствие»" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Вставить «Проверка оплаты»" })).not.toBeInTheDocument();
    fireEvent.change(search, { target: { value: "поступление" } });
    const choice = screen.getByRole("button", { name: "Вставить «Проверка оплаты»" });
    expect(choice).toHaveTextContent("Проверяю поступление перевода.");
    expect(choice).not.toHaveTextContent("**");
    fireEvent.click(choice);
    expect(onInsert).toHaveBeenCalledExactlyOnceWith(templates[0].body);
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("reports a failed load and reloads on retry", async () => {
    vi.mocked(api.listReplyTemplates).mockRejectedValueOnce(new Error("Unavailable"));
    render(picker());
    fireEvent.click(screen.getByRole("button", { name: "Шаблоны" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось загрузить шаблоны.");
    fireEvent.click(screen.getByRole("button", { name: "Повторить" }));
    expect(await screen.findByRole("button", { name: "Вставить «Приветствие»" })).toBeInTheDocument();
    expect(api.listReplyTemplates).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("distinguishes an empty Inbox from a search without matches", async () => {
    vi.mocked(api.listReplyTemplates).mockResolvedValueOnce([]);
    render(picker());
    fireEvent.click(screen.getByRole("button", { name: "Шаблоны" }));
    expect(await screen.findByText("Шаблонов ответов пока нет.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Закрыть выбор шаблона" }));
    await openPicker();
    fireEvent.change(screen.getByRole("searchbox", { name: "Поиск шаблонов" }), { target: { value: "несуществующий" } });
    expect(screen.getByRole("status")).toHaveTextContent("Шаблоны не найдены.");
  });

  it("closes with Escape, restores trigger focus, and aborts the outstanding load", async () => {
    let resolveLoad!: (items: api.ReplyTemplate[]) => void;
    vi.mocked(api.listReplyTemplates).mockImplementationOnce(() => new Promise((resolve) => { resolveLoad = resolve; }));
    render(picker());
    const trigger = screen.getByRole("button", { name: "Шаблоны" });
    fireEvent.click(trigger);
    const signal = vi.mocked(api.listReplyTemplates).mock.calls[0][2];
    fireEvent.keyDown(screen.getByRole("searchbox", { name: "Поиск шаблонов" }), { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    expect(signal?.aborted).toBe(true);
    await act(async () => resolveLoad(templates));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });

  it("keeps the picker open when insertion exceeds the reply limit", async () => {
    const onInsert = vi.fn(() => false);
    render(picker({ onInsert }));
    await openPicker();
    fireEvent.click(screen.getByRole("button", { name: "Вставить «Проверка оплаты»" }));
    expect(screen.getByRole("alert")).toHaveTextContent("С этим шаблоном ответ превысит 10 000 символов.");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    onInsert.mockReturnValue(true);
    fireEvent.click(screen.getByRole("button", { name: "Вставить «Приветствие»" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("replaces the template scope when the Inbox changes during an outstanding load", async () => {
    let resolveOldInbox!: (items: api.ReplyTemplate[]) => void;
    vi.mocked(api.listReplyTemplates).mockImplementationOnce(() => new Promise((resolve) => { resolveOldInbox = resolve; }));
    vi.mocked(api.listReplyTemplates).mockResolvedValueOnce([{ ...templates[1], inbox_id: "sales" }]);
    const { rerender } = render(picker());
    fireEvent.click(screen.getByRole("button", { name: "Шаблоны" }));
    const oldSignal = vi.mocked(api.listReplyTemplates).mock.calls[0][2];
    rerender(picker({ inboxId: "sales" }));
    expect(await screen.findByRole("button", { name: "Вставить «Приветствие»" })).toBeInTheDocument();
    expect(api.listReplyTemplates).toHaveBeenLastCalledWith(auth, "sales", expect.any(AbortSignal));
    expect(oldSignal?.aborted).toBe(true);
    await act(async () => resolveOldInbox([templates[0]]));
    expect(screen.queryByRole("button", { name: "Вставить «Проверка оплаты»" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Вставить «Приветствие»" })).toBeInTheDocument();
  });

  it("does not load templates while disabled and removes an open picker when disabled", async () => {
    const { rerender } = render(picker({ disabled: true }));
    const trigger = screen.getByRole("button", { name: "Шаблоны" });
    expect(trigger).toBeDisabled();
    fireEvent.click(trigger);
    expect(api.listReplyTemplates).not.toHaveBeenCalled();
    rerender(picker());
    await openPicker();
    rerender(picker({ disabled: true }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(trigger).toHaveAttribute("aria-expanded", "false");
  });
});
