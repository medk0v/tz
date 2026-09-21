import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createRef, useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import { conversationReplySource, replyTemplateDraftSource, type AiDraftSource } from "./ai-draft-sources";
import ChatMessageEditor from "./ChatMessageEditor";
import { ConversationComposer } from "./ConversationComposer";

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  generateReplySuggestion: vi.fn(),
  generateReplyTemplateDraft: vi.fn(),
  listReplySuggestionAgents: vi.fn(),
  listReplyTemplateDraftAgents: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const agent: api.ReplySuggestionAgent = {
  id: "agent-1",
  name: "Order assistant",
  avatar_url: null,
};

function ChatDraft({
  initialValue = "Здравствуйте!",
  source = conversationReplySource(auth, "conversation-1"),
  onChange,
  onSubmit,
}: {
  initialValue?: string;
  source?: AiDraftSource;
  onChange: (body: string) => void;
  onSubmit: () => void;
}) {
  const [draft, setDraft] = useState(initialValue);
  return (
    <I18nContext.Provider value={createI18n("ru", vi.fn())}>
      <form onSubmit={(event) => { event.preventDefault(); onSubmit(); }}>
        <ChatMessageEditor
          value={draft}
          onChange={(body) => { setDraft(body); onChange(body); }}
          label="Ответ"
          placeholder="Напишите сообщение"
          aiDraftSource={source}
        />
      </form>
    </I18nContext.Provider>
  );
}

describe("AI reply suggestions in the chat editor", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listReplySuggestionAgents).mockResolvedValue([agent]);
    vi.mocked(api.generateReplySuggestion).mockResolvedValue({
      body: "**Проверьте** раздел Активы и нажмите Снять.",
      body_format: "markdown",
    });
  });

  afterEach(cleanup);

  it("generates a reply as the active AI agent before an operator joins", async () => {
    const onDraftChange = vi.fn();
    const onSubmit = vi.fn((event) => event.preventDefault());
    render(
      <I18nContext.Provider value={createI18n("ru", vi.fn())}>
        <ConversationComposer
          auth={auth}
          conversation={{
            id: "conversation-1",
            inbox_id: "inbox-1",
            contact_id: "contact-1",
            contact: { display_name: "Customer", email: null, is_blocked: false },
            status: "open",
            subject: null,
            last_message_sequence: 1,
            created_at: "2026-09-20T00:00:00Z",
            updated_at: "2026-09-20T00:00:00Z",
            operator: null,
            ai_agent: { display_name: "Sophie", avatar_url: null, joined_at: "2026-09-20T00:00:00Z", active: true },
            assigned_to_me: false,
            unread_customer_messages: 0,
            widget_attachments_enabled: false,
            channel: { id: "channel-1", name: "Website", kind: "widget" },
          }}
          channelLabel="Website"
          draft=""
          visitorDraft=""
          canReply
          canReplyAsAi
          canReplyAsOperator={false}
          sending={false}
          uploading={false}
          attachmentInputRef={createRef<HTMLInputElement>()}
          onDraftChange={onDraftChange}
          onSubmit={onSubmit}
          onAttachmentChange={vi.fn()}
        />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Сгенерировать ИИ-ответ" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Order assistant" }));

    await waitFor(() => expect(onDraftChange).toHaveBeenLastCalledWith(
      "**Проверьте** раздел Активы и нажмите Снять.",
    ));
    expect(screen.getByRole("textbox", { name: "Ответ от имени ИИ-агента Sophie" })).toHaveTextContent(
      "Проверьте раздел Активы и нажмите Снять.",
    );
    expect(api.generateReplySuggestion).toHaveBeenCalledWith(auth, "conversation-1", agent.id, "", expect.any(AbortSignal));
    expect(screen.getByRole("status")).toHaveTextContent("Агент останется подключён к диалогу.");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("lets the operator choose an agent and inserts the generated draft without sending it", async () => {
    const onChange = vi.fn();
    const onSubmit = vi.fn();
    render(<ChatDraft onChange={onChange} onSubmit={onSubmit} />);

    fireEvent.click(screen.getByRole("button", { name: "Сгенерировать ИИ-ответ" }));
    const choice = await screen.findByRole("menuitem", { name: "Order assistant" });
    expect(api.listReplySuggestionAgents).toHaveBeenCalledWith(
      auth,
      "conversation-1",
      expect.any(AbortSignal),
    );
    fireEvent.click(choice);

    const editor = screen.getByRole("textbox", { name: "Ответ" });
    await waitFor(() => expect(editor).toHaveTextContent("Проверьте раздел Активы и нажмите Снять."));
    expect(editor).toHaveTextContent("Здравствуйте!");
    expect(editor.querySelector("strong")).toHaveTextContent("Проверьте");
    expect(api.generateReplySuggestion).toHaveBeenCalledWith(
      auth,
      "conversation-1",
      agent.id,
      "Здравствуйте!",
      expect.any(AbortSignal),
    );
    expect(onChange).toHaveBeenLastCalledWith(
      "Здравствуйте!\n\n**Проверьте** раздел Активы и нажмите Снять.",
    );
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("requests a new reply when the operator draft is empty", async () => {
    const onChange = vi.fn();
    render(<ChatDraft initialValue="" onChange={onChange} onSubmit={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Сгенерировать ИИ-ответ" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Order assistant" }));

    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(
      "**Проверьте** раздел Активы и нажмите Снять.",
    ));
    expect(api.generateReplySuggestion).toHaveBeenCalledWith(
      auth,
      "conversation-1",
      agent.id,
      "",
      expect.any(AbortSignal),
    );
  });

  it("keeps the agent menu open when generation fails so the operator can retry", async () => {
    vi.mocked(api.generateReplySuggestion)
      .mockRejectedValueOnce(new Error("Unavailable"))
      .mockResolvedValueOnce({ body: "Ответ после повтора.", body_format: "markdown" });
    const onChange = vi.fn();
    render(<ChatDraft onChange={onChange} onSubmit={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Сгенерировать ИИ-ответ" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Order assistant" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось сгенерировать ответ.");
    fireEvent.click(screen.getByRole("menuitem", { name: "Order assistant" }));

    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith(
      "Здравствуйте!\n\nОтвет после повтора.",
    ));
    expect(api.generateReplySuggestion).toHaveBeenCalledTimes(2);
  });

  it("drafts a reply template from its title with the agents available for templates", async () => {
    vi.mocked(api.listReplyTemplateDraftAgents).mockResolvedValue([agent]);
    vi.mocked(api.generateReplyTemplateDraft).mockResolvedValue({
      body: "Обработка занимает до **30 минут**.",
      body_format: "markdown",
    });
    const onChange = vi.fn();
    render(<ChatDraft initialValue="" source={replyTemplateDraftSource(auth, "inbox-1", "Обмен USDT")} onChange={onChange} onSubmit={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Сгенерировать с ИИ" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Order assistant" }));

    await waitFor(() => expect(onChange).toHaveBeenLastCalledWith("Обработка занимает до **30 минут**."));
    expect(api.listReplyTemplateDraftAgents).toHaveBeenCalledWith(auth, "inbox-1", expect.any(AbortSignal));
    expect(api.generateReplyTemplateDraft).toHaveBeenCalledWith(auth, "inbox-1", agent.id, "Обмен USDT", "", expect.any(AbortSignal));
    expect(api.listReplySuggestionAgents).not.toHaveBeenCalled();
  });

  it("explains when no agent can draft a template", async () => {
    vi.mocked(api.listReplyTemplateDraftAgents).mockResolvedValue([]);
    render(<ChatDraft source={replyTemplateDraftSource(auth, "inbox-1", "Обмен USDT")} onChange={vi.fn()} onSubmit={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Сгенерировать с ИИ" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Нет доступных ИИ-агентов.");
  });
});
