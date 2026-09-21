import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import { ConversationsView } from "./ConversationsView";
import { MemoryPageRoute } from "./MemoryPageRoute";
import type { ChatMessageEditorProps } from "./ChatMessageEditor";

vi.mock("./ChatMessageEditor", () => ({
  default: ({ value, onChange, label, disabled }: ChatMessageEditorProps) => (
    <textarea aria-label={label} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
  ),
}));

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  getVisitorIntelligence: vi.fn().mockResolvedValue(null),
  listInboxConversations: vi.fn(),
  listInboxConversationPage: vi.fn(),
  listOperatorMessages: vi.fn().mockResolvedValue([]),
  reopenConversation: vi.fn(),
  setContactBlocked: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const inbox: api.Inbox = {
  id: "inbox-1",
  project_id: "project-1",
  name: "Support",
  status: "active",
  created_at: "2026-09-01T00:00:00Z",
};
const resolved: api.OperatorConversation = {
  id: "conversation-1",
  inbox_id: inbox.id,
  contact_id: "contact-1",
  contact: { display_name: "Customer", email: null, is_blocked: false },
  status: "resolved",
  subject: "Payment question",
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

async function openResolvedConversation(canCloseConversations = true, canManageContacts = false) {
  render(
    <I18nContext.Provider value={createI18n("ru", vi.fn())}>
      <ConversationsView
        auth={auth}
        canSendAsAi={false}
        canReplyConversations
        canCloseConversations={canCloseConversations}
        canManageContacts={canManageContacts}
        inboxes={[inbox]}
        activeInboxId={inbox.id}
        realtimeEvent={null}
        realtimeSyncRevision={0}
        notificationsChanging={false}
        notificationsEnabled={false}
        notificationPermission="default"
        onNotificationsChange={vi.fn().mockResolvedValue(undefined)}
        onInboxChange={vi.fn()}
      />
    </I18nContext.Provider>,
  );
  fireEvent.change(screen.getByRole("combobox", { name: "Состояние диалога" }), {
    target: { value: "resolved" },
  });
  fireEvent.click(await screen.findByRole("button", { name: /Payment question/ }));
}

describe("reopening conversations", () => {
  beforeEach(() => {
    vi.mocked(api.listInboxConversations).mockResolvedValue([resolved]);
    vi.mocked(api.listInboxConversationPage).mockImplementation(async (...args) => ({
      items: await api.listInboxConversations(...args), has_more: false,
    }));
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("updates the contact blacklist from the conversation sidebar", async () => {
    vi.mocked(api.setContactBlocked).mockResolvedValue({ id: resolved.contact_id, is_blocked: true });
    await openResolvedConversation(true, true);
    fireEvent.click(screen.getByRole("button", { name: "Добавить в ЧС" }));
    expect(await screen.findByText("В чёрном списке")).toBeInTheDocument();
    expect(api.setContactBlocked).toHaveBeenCalledWith(auth, resolved.contact_id, true);
    expect(screen.getByRole("button", { name: "Убрать из ЧС" })).toBeInTheDocument();
  });

  it("opens an explicitly selected visitor conversation before its first message", async () => {
    const visitorConversation = { ...resolved, status: "open" as const, last_message_sequence: 0 };
    vi.mocked(api.listInboxConversations).mockResolvedValue([
      visitorConversation,
      { ...visitorConversation, id: "another-visitor", subject: "Unstarted visitor" },
    ]);
    render(
      <I18nContext.Provider value={createI18n("ru", vi.fn())}>
        <MemoryPageRoute initialSegments={[inbox.id, visitorConversation.id]}>
          <ConversationsView
            auth={auth}
            canSendAsAi={false}
            canReplyConversations
            canCloseConversations
            inboxes={[inbox]}
            activeInboxId={inbox.id}
            realtimeEvent={null}
            realtimeSyncRevision={0}
            notificationsChanging={false}
            notificationsEnabled={false}
            notificationPermission="default"
            onNotificationsChange={vi.fn().mockResolvedValue(undefined)}
            onInboxChange={vi.fn()}
          />
        </MemoryPageRoute>
      </I18nContext.Provider>,
    );
    await waitFor(() => expect(screen.getByLabelText("Ответ")).toBeEnabled());
    expect(api.listInboxConversations).toHaveBeenCalledWith(auth, inbox.id, expect.objectContaining({ includeConversationId: visitorConversation.id }));
    expect(screen.queryByText("Unstarted visitor")).not.toBeInTheDocument();
  });

  it("returns a resolved conversation to active and enables the assigned operator's reply", async () => {
    let finishReopening!: () => void;
    vi.mocked(api.reopenConversation).mockImplementationOnce(() => new Promise<void>((resolve) => {
      finishReopening = resolve;
    }));
    await openResolvedConversation();
    expect(screen.getByLabelText("Ответ")).toBeDisabled();
    const reopen = screen.getByRole("button", { name: "Вернуть в работу" });
    fireEvent.click(reopen);
    expect(reopen).toBeDisabled();
    expect(api.reopenConversation).toHaveBeenCalledExactlyOnceWith(auth, resolved.id);

    vi.mocked(api.listInboxConversations).mockResolvedValue([{ ...resolved, status: "open" }]);
    await act(async () => finishReopening());

    await waitFor(() => expect(screen.getByLabelText("Ответ")).toBeEnabled());
    expect(screen.getByRole("combobox", { name: "Состояние диалога" })).toHaveValue("active");
    expect(screen.getByRole("button", { name: /Payment question/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Завершить" })).toBeEnabled();
  });

  it("keeps the resolved conversation and shows an error when reopening fails", async () => {
    vi.mocked(api.reopenConversation).mockRejectedValueOnce(new Error("Unavailable"));
    await openResolvedConversation();
    fireEvent.click(screen.getByRole("button", { name: "Вернуть в работу" }));

    expect(await screen.findByText("Не удалось вернуть диалог в работу.")).toBeInTheDocument();
    expect(screen.getByLabelText("Ответ")).toBeDisabled();
    expect(screen.getByRole("combobox", { name: "Состояние диалога" })).toHaveValue("resolved");
    expect(screen.getByRole("button", { name: "Вернуть в работу" })).toBeEnabled();
  });

  it("does not show reopening without permission to close conversations", async () => {
    await openResolvedConversation(false);
    expect(screen.queryByRole("button", { name: "Вернуть в работу" })).not.toBeInTheDocument();
    expect(api.reopenConversation).not.toHaveBeenCalled();
  });
});
