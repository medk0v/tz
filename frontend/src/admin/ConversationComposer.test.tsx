import { createRef } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { OperatorConversation } from "../api";
import { createI18n, I18nContext } from "../i18n";
import type { ChatMessageEditorProps } from "./ChatMessageEditor";
import { ConversationComposer, type ConversationComposerProps } from "./ConversationComposer";

vi.mock("./ChatMessageEditor", () => ({
  default: ({ value, onChange, label, disabled, templateSource, aiDraftSource }: ChatMessageEditorProps) => (
    <>
      <textarea aria-label={label} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
      {templateSource && <button type="button">Reply templates</button>}
      {aiDraftSource && <button type="button">AI suggestions</button>}
    </>
  ),
}));

const conversation: OperatorConversation = {
  id: "conversation-1",
  inbox_id: "inbox-1",
  contact_id: "contact-1",
  contact: { display_name: "Customer", email: null, is_blocked: false },
  status: "open",
  subject: null,
  last_message_sequence: 1,
  created_at: "2026-09-01T00:00:00Z",
  updated_at: "2026-09-01T00:00:00Z",
  operator: { display_name: "Andrew", avatar_url: null, joined_at: "2026-09-01T00:00:00Z" },
  ai_agent: null,
  assigned_to_me: true,
  unread_customer_messages: 0,
  widget_attachments_enabled: false,
  channel: { id: "channel-1", name: "Website", kind: "widget" },
};

function composer(props: ConversationComposerProps) {
  return (
    <I18nContext.Provider value={createI18n("ru", vi.fn())}>
      <ConversationComposer {...props} />
    </I18nContext.Provider>
  );
}

describe("conversation composer", () => {
  afterEach(cleanup);

  it("keeps reply and attachment controls working and only shows an active visitor draft", async () => {
    const props: ConversationComposerProps = {
      auth: { kind: "session" },
      conversation,
      channelLabel: <span>Website</span>,
      draft: "Готово",
      visitorDraft: "",
      canReply: true,
      canReplyAsAi: false,
      canReplyAsOperator: true,
      sending: false,
      uploading: false,
      attachmentInputRef: createRef<HTMLInputElement>(),
      onDraftChange: vi.fn(),
      onSubmit: vi.fn((event) => event.preventDefault()),
      onAttachmentChange: vi.fn(),
    };
    const { rerender } = render(composer(props));
    const editor = await screen.findByRole("textbox", { name: "Ответ" });
    expect(screen.queryByText("Пользователь пишет")).not.toBeInTheDocument();

    fireEvent.change(editor, { target: { value: "Новый ответ" } });
    expect(props.onDraftChange).toHaveBeenCalledWith("Новый ответ");
    fireEvent.click(screen.getByRole("button", { name: "Отправить" }));
    expect(props.onSubmit).toHaveBeenCalledOnce();

    const file = new File(["receipt"], "receipt.png", { type: "image/png" });
    fireEvent.change(props.attachmentInputRef.current!, { target: { files: [file] } });
    expect(props.onAttachmentChange).toHaveBeenCalledOnce();

    rerender(composer({ ...props, visitorDraft: "Можно уточнить?", sending: true }));
    expect(screen.getByText("Пользователь пишет")).toBeInTheDocument();
    expect(screen.getByText("Можно уточнить?")).toBeInTheDocument();
    expect(editor).toBeDisabled();
    expect(screen.getByRole("button", { name: "Отправляем…" })).toBeDisabled();

    rerender(composer({ ...props, canReply: false, conversation: { ...conversation, assigned_to_me: false } }));
    expect(editor).toBeDisabled();
    expect(screen.getByRole("button", { name: "Отправить" })).toBeDisabled();
    expect(props.attachmentInputRef.current).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Andrew");
    expect(screen.queryByText("Пользователь пишет")).not.toBeInTheDocument();
  });

  it("enforces Telegram's text message limit", async () => {
    const onSubmit = vi.fn((event) => event.preventDefault());
    render(composer({
      auth: { kind: "session" },
      conversation: {
        ...conversation,
        channel: { ...conversation.channel, kind: "telegram_bot" },
      },
      channelLabel: <span>Telegram</span>,
      draft: "x".repeat(4_097),
      visitorDraft: "",
      canReply: true,
      canReplyAsAi: false,
      canReplyAsOperator: true,
      sending: false,
      uploading: false,
      attachmentInputRef: createRef<HTMLInputElement>(),
      onDraftChange: vi.fn(),
      onSubmit,
      onAttachmentChange: vi.fn(),
    }));

    expect(await screen.findByRole("alert")).toHaveTextContent("4096");
    fireEvent.click(screen.getByRole("button", { name: "Отправить" }));
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("allows demo text replies while showing reply helpers and disabling uploads", async () => {
    const onSubmit = vi.fn((event) => event.preventDefault());
    render(composer({
      auth: { kind: "session" },
      isDemo: true,
      conversation,
      channelLabel: <span>Website</span>,
      draft: "Демо-ответ",
      visitorDraft: "",
      canReply: true,
      canReplyAsAi: false,
      canReplyAsOperator: true,
      sending: false,
      uploading: false,
      attachmentInputRef: createRef<HTMLInputElement>(),
      onDraftChange: vi.fn(),
      onSubmit,
      onAttachmentChange: vi.fn(),
    }));

    expect(await screen.findByRole("textbox", { name: "Ответ" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Reply templates" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "AI suggestions" })).toBeInTheDocument();
    expect(document.querySelector('input[type="file"]')).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Отправить" }));
    expect(onSubmit).toHaveBeenCalledOnce();
  });
});
