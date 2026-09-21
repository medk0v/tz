import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import ChatMessageEditor from "./ChatMessageEditor";

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  listReplyTemplates: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const template: api.ReplyTemplate = {
  id: "payment", inbox_id: "support", title: "Проверка оплаты",
  body: "**Проверяю оплату.**\n\n- Уточняю поступление.\n- Сообщу результат.",
  body_format: "markdown", created_at: "2026-09-03T00:00:00Z", updated_at: "2026-09-03T00:00:00Z",
};

function ChatDraft({ initialValue, onChange, onSubmit, disabled = false }: { initialValue: string; onChange: (body: string) => void; onSubmit: () => void; disabled?: boolean }) {
  const [draft, setDraft] = useState(initialValue);
  return (
    <I18nContext.Provider value={createI18n("ru", vi.fn())}>
      <form onSubmit={(event) => { event.preventDefault(); onSubmit(); }}>
        <ChatMessageEditor value={draft} onChange={(body) => { setDraft(body); onChange(body); }} label="Ответ"
          placeholder="Напишите сообщение" templateSource={{ auth, inboxId: "support" }} disabled={disabled} />
      </form>
    </I18nContext.Provider>
  );
}

async function chooseTemplate() {
  fireEvent.click(screen.getByRole("button", { name: "Шаблоны" }));
  fireEvent.click(await screen.findByRole("button", { name: "Вставить «Проверка оплаты»" }));
}

describe("reply templates in the chat editor", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listReplyTemplates).mockResolvedValue([template]);
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("keeps the existing draft, inserts rich template content, and waits for explicit sending", async () => {
    const onChange = vi.fn();
    const onSubmit = vi.fn();
    render(<ChatDraft initialValue="Здравствуйте!" onChange={onChange} onSubmit={onSubmit} />);
    await chooseTemplate();
    const editor = screen.getByRole("textbox", { name: "Ответ" });
    await waitFor(() => expect(editor).toHaveTextContent("Проверяю оплату."));
    expect(editor).toHaveTextContent("Здравствуйте!");
    expect(editor.querySelector("strong")).toHaveTextContent("Проверяю оплату.");
    expect(editor.querySelectorAll("li")).toHaveLength(2);
    expect(onChange).toHaveBeenLastCalledWith(expect.stringContaining("**Проверяю оплату.**"));
    expect(onChange).toHaveBeenLastCalledWith(expect.stringContaining("Здравствуйте!"));
    expect(onChange).toHaveBeenLastCalledWith("Здравствуйте!\n\n**Проверяю оплату.**\n\n* Уточняю поступление.\n* Сообщу результат.");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("leaves a long draft unchanged when the template would exceed 10,000 characters", async () => {
    const onChange = vi.fn();
    const onSubmit = vi.fn();
    const draft = "А".repeat(9_980);
    render(<ChatDraft initialValue={draft} onChange={onChange} onSubmit={onSubmit} />);
    await chooseTemplate();
    expect(screen.getByRole("alert")).toHaveTextContent("С этим шаблоном ответ превысит 10 000 символов.");
    expect(screen.getByRole("textbox", { name: "Ответ" }).textContent).toBe(draft);
    expect(onChange).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("allows a 10,000-character template in an empty draft", async () => {
    const body = "А".repeat(10_000);
    vi.mocked(api.listReplyTemplates).mockResolvedValue([{ ...template, body }]);
    const onChange = vi.fn();
    render(<ChatDraft initialValue="" onChange={onChange} onSubmit={vi.fn()} />);
    await chooseTemplate();
    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(body));
    expect(screen.getByRole("textbox", { name: "Ответ" }).textContent).toBe(body);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("appends a reply after an existing list without turning it into another list item", async () => {
    const onChange = vi.fn();
    render(<ChatDraft initialValue="- Уже уточнил реквизиты." onChange={onChange} onSubmit={vi.fn()} />);
    await chooseTemplate();
    const editor = screen.getByRole("textbox", { name: "Ответ" });
    await waitFor(() => expect(editor.querySelector("strong")).toHaveTextContent("Проверяю оплату."));
    const insertedParagraph = editor.querySelector("strong")!.closest("p");
    expect(insertedParagraph?.parentElement).toBe(editor);
    expect(editor.querySelector("li")).toHaveTextContent("Уже уточнил реквизиты.");
    expect(editor.querySelectorAll("li")).toHaveLength(3);
    expect(editor.querySelectorAll("ul")).toHaveLength(2);
    expect(onChange).toHaveBeenLastCalledWith(expect.stringContaining("Уже уточнил реквизиты."));
  });

  it("undoes template insertion in one step and restores the original draft", async () => {
    const onChange = vi.fn();
    render(<ChatDraft initialValue="Черновик." onChange={onChange} onSubmit={vi.fn()} />);
    await chooseTemplate();
    const editor = screen.getByRole("textbox", { name: "Ответ" });
    await waitFor(() => expect(editor).toHaveTextContent("Проверяю оплату."));
    fireEvent.click(screen.getByLabelText(/Отменить/));
    await waitFor(() => expect(editor.textContent).toBe("Черновик."));
    expect(onChange).toHaveBeenLastCalledWith("Черновик.");
  });

  it("restores the caret after searching templates without dropping surrounding draft text", async () => {
    vi.mocked(api.listReplyTemplates).mockResolvedValue([{ ...template, body: "Ответ." }]);
    const onChange = vi.fn();
    render(<ChatDraft initialValue="До. После." onChange={onChange} onSubmit={vi.fn()} />);
    const editor = screen.getByRole("textbox", { name: "Ответ" });
    await act(async () => {
      editor.focus();
      const text = screen.getByText("До. После.").firstChild!;
      const range = document.createRange();
      range.setStart(text, 4);
      range.collapse(true);
      window.getSelection()?.removeAllRanges();
      window.getSelection()?.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
    });
    fireEvent.click(screen.getByRole("button", { name: "Шаблоны" }));
    await screen.findByRole("button", { name: "Вставить «Проверка оплаты»" });
    fireEvent.change(screen.getByRole("searchbox", { name: "Поиск шаблонов" }), { target: { value: "Ответ" } });
    fireEvent.click(screen.getByRole("button", { name: "Вставить «Проверка оплаты»" }));
    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(expect.stringMatching(/^До\.\s*Ответ\.\s*После\.$/)));
  });

  it("does not insert a queued template after the conversation becomes read-only", async () => {
    const onChange = vi.fn();
    const onSubmit = vi.fn();
    const props = { initialValue: "Черновик", onChange, onSubmit };
    const { rerender } = render(<ChatDraft {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Шаблоны" }));
    const choice = await screen.findByRole("button", { name: "Вставить «Проверка оплаты»" });
    const frames: FrameRequestCallback[] = [];
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => { frames.push(callback); return frames.length; });
    fireEvent.click(choice);
    rerender(<ChatDraft {...props} disabled />);
    await act(async () => { frames.forEach((callback) => callback(0)); });
    expect(screen.getByRole("textbox", { name: "Ответ" })).toHaveAttribute("contenteditable", "false");
    expect(screen.getByRole("textbox", { name: "Ответ" }).textContent).toBe("Черновик");
    expect(onChange).not.toHaveBeenCalled();
  });
});
