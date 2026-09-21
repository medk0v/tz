import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createI18n, I18nContext } from "../i18n";
import ChatMessageEditor, { type ChatMessageEditorProps } from "./ChatMessageEditor";

function editor(props: Partial<ChatMessageEditorProps> = {}) {
  return (
    <I18nContext.Provider value={createI18n("ru", vi.fn())}>
      <ChatMessageEditor value="" onChange={vi.fn()} label="Ответ" placeholder="Напишите сообщение" {...props} />
    </I18nContext.Provider>
  );
}

describe("chat message editor", () => {
  afterEach(cleanup);

  it("shows formatted text and a localized chat toolbar without changing the draft on mount", async () => {
    const onChange = vi.fn();
    render(editor({ value: "**Важно** и *курсив*\n\n- Первый пункт\n\n> Цитата", onChange }));
    expect(await screen.findByText("Важно")).toHaveProperty("tagName", "STRONG");
    expect(screen.getByText("курсив")).toHaveProperty("tagName", "EM");
    expect(screen.getByText("Первый пункт").closest("ul")).toBeInTheDocument();
    expect(screen.getByText("Цитата").closest("blockquote")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Жирный" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Добавить ссылку" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ответ" })).toHaveAttribute("contenteditable", "true");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("formats selected text through the toolbar and exports Markdown", async () => {
    const onChange = vi.fn();
    render(editor({ value: "Выделить", onChange }));
    const textbox = screen.getByRole("textbox", { name: "Ответ" });
    const text = screen.getByText("Выделить");
    await act(async () => {
      textbox.focus();
      const range = document.createRange();
      range.selectNodeContents(text);
      window.getSelection()?.removeAllRanges();
      window.getSelection()?.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
    });
    fireEvent.click(screen.getByRole("radio", { name: "Жирный" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledWith("**Выделить**"));
  });

  it("clears the visual editor after sending and follows the read-only state", async () => {
    const { rerender } = render(editor({ value: "**Готово**" }));
    expect(screen.getByText("Готово")).toBeInTheDocument();
    rerender(editor({ value: "", disabled: true }));
    await waitFor(() => expect(screen.queryByText("Готово")).not.toBeInTheDocument());
    expect(screen.getByRole("textbox", { name: "Ответ" })).toHaveAttribute("contenteditable", "false");
    rerender(editor({ value: "Новый ответ", disabled: false }));
    expect(await screen.findByText("Новый ответ")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Ответ" })).toHaveAttribute("contenteditable", "true");
  });

  it("keeps an empty list available for typing without making an empty message sendable", async () => {
    const onChange = vi.fn();
    render(editor({ onChange }));
    await act(async () => screen.getByRole("textbox", { name: "Ответ" }).focus());
    fireEvent.click(screen.getByRole("radio", { name: "Маркированный список" }));
    await waitFor(() => expect(onChange).toHaveBeenCalledWith(""));
    expect(screen.getByRole("textbox", { name: "Ответ" }).querySelector("ul")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Стиль текста" })).toHaveTextContent("Стиль текста");
  });

  it("keeps the block selector labeled when the caret is in a list item", async () => {
    const onChange = vi.fn();
    render(editor({ value: "- Первый пункт", onChange }));
    const textbox = screen.getByRole("textbox", { name: "Ответ" });
    await act(async () => {
      textbox.focus();
      const range = document.createRange();
      range.selectNodeContents(screen.getByText("Первый пункт"));
      range.collapse(true);
      window.getSelection()?.removeAllRanges();
      window.getSelection()?.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
    });
    const select = screen.getByRole("combobox", { name: "Стиль текста" });
    expect(select).toHaveTextContent("Стиль текста");
    expect(textbox.querySelector("ul")).toHaveTextContent("Первый пункт");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("sends with Ctrl or Command plus Enter while ordinary Enter and composition do not submit", () => {
    const submit = vi.fn((event) => event.preventDefault());
    render(<form onSubmit={submit}>{editor({ value: "Ответ" })}</form>);
    const textbox = screen.getByRole("textbox", { name: "Ответ" });
    fireEvent.keyDown(textbox, { key: "Enter" });
    fireEvent.keyDown(textbox, { key: "Enter", ctrlKey: true, isComposing: true });
    expect(submit).not.toHaveBeenCalled();
    fireEvent.keyDown(textbox, { key: "Enter", ctrlKey: true });
    fireEvent.keyDown(textbox, { key: "Enter", metaKey: true });
    expect(submit).toHaveBeenCalledTimes(2);
  });

  it("does not interpret raw HTML as executable editor content", () => {
    const { container } = render(editor({ value: '<img src="x" onerror="alert(1)"><script>alert(2)</script>' }));
    expect(container.querySelector("script, img, [onerror]")).not.toBeInTheDocument();
  });
});
