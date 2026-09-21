import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import type { ChatMessageEditorProps } from "./ChatMessageEditor";
import { ConversationsView } from "./ConversationsView";
import { MemoryPageRoute } from "./MemoryPageRoute";

vi.mock("./ChatMessageEditor", () => ({
  default: ({ value, onChange, label, disabled }: ChatMessageEditorProps) => (
    <textarea aria-label={label} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
  ),
}));

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  listInboxConversationPage: vi.fn(),
  listOperatorMessages: vi.fn(),
  getVisitorIntelligence: vi.fn(),
  markOperatorConversationRead: vi.fn(),
  sendOperatorMessage: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const inbox: api.Inbox = {
  id: "inbox-1", project_id: "project-1", name: "Support", status: "active", created_at: "2026-09-01T00:00:00Z",
};
const alice: api.OperatorConversation = {
  id: "alice-current", inbox_id: inbox.id, contact_id: "alice-contact",
  contact: { display_name: "Alice", email: "alice@example.com", is_blocked: false },
  status: "open", subject: "Current question", last_message_sequence: 2,
  created_at: "2026-09-01T00:00:00Z", updated_at: "2026-09-01T00:01:00Z",
  operator: { display_name: "Andrew", avatar_url: null, joined_at: "2026-09-01T00:00:00Z" },
  ai_agent: null, assigned_to_me: true, unread_customer_messages: 3,
  awaiting_reply_since: "2026-09-01T00:01:00Z", contact_conversation_count: 2,
  widget_attachments_enabled: false, channel: { id: "widget-channel", name: "Website", kind: "widget" },
};
const bob: api.OperatorConversation = {
  ...alice, id: "bob-current", contact_id: "bob-contact",
  contact: { display_name: "Bob", email: null, is_blocked: false },
  unread_customer_messages: 5, awaiting_reply_since: null, contact_conversation_count: 1,
};
const past: api.OperatorConversation = {
  ...alice, id: "alice-past", status: "resolved", subject: "Previous request", unread_customer_messages: 0,
  awaiting_reply_since: null, assigned_to_me: false, operator: null,
  channel: { id: "email-channel", name: "Email", kind: "imap_smtp" },
};
const telegramContact: api.OperatorConversation = {
  ...alice,
  id: "telegram-current",
  contact_id: "telegram-contact",
  contact: { display_name: "Ginotix", email: null, is_blocked: false },
  subject: "Telegram question",
  channel: { id: "telegram-channel", name: "Customer Support Bot", kind: "telegram_bot" },
  telegram: {
    username: "ginotix",
    user_id: "7795261514",
    chat_id: "7795261514",
    language_code: "en",
  },
};

function message(conversationId: string, body: string): api.Message {
  return {
    id: `${conversationId}-message`, conversation_id: conversationId, sequence: 3,
    direction: "outbound", author_kind: "operator", kind: "text", body, body_format: "markdown",
    status: "sent", created_at: "2026-09-01T00:02:00Z", attachments: [],
  };
}

function renderInbox(linkedConversationId: string | null = null) {
  return render(
    <I18nContext.Provider value={createI18n("en", vi.fn())}>
      <MemoryPageRoute initialSegments={linkedConversationId ? [inbox.id, linkedConversationId] : []}>
        <ConversationsView
          auth={auth} canSendAsAi={false} canReplyConversations canCloseConversations
          inboxes={[inbox]} activeInboxId={inbox.id} realtimeEvent={null} realtimeSyncRevision={0}
          notificationsChanging={false} notificationsEnabled={false}
          notificationPermission="default" onNotificationsChange={vi.fn().mockResolvedValue(undefined)}
          onInboxChange={vi.fn()}
        />
      </MemoryPageRoute>
    </I18nContext.Provider>,
  );
}

function conversationList() {
  return within(screen.getByRole("region", { name: "Conversation list" }));
}

async function openConversation(name: string) {
  fireEvent.click(await conversationList().findByRole("button", { name: new RegExp(name) }));
  return screen.findByRole("textbox", { name: "Reply" });
}

describe("conversation Inbox workflow", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(api.listInboxConversationPage).mockResolvedValue({ items: [alice, bob], has_more: false });
    vi.mocked(api.listOperatorMessages).mockResolvedValue([]);
    vi.mocked(api.getVisitorIntelligence).mockResolvedValue(null);
    vi.mocked(api.markOperatorConversationRead).mockResolvedValue(undefined);
  });

  afterEach(cleanup);

  it("requests the reply queue on the server and keeps read-but-unanswered conversations in it", async () => {
    vi.mocked(api.listInboxConversationPage).mockImplementation(async (_auth, _inboxId, options) => ({
      items: options?.needsReply ? [alice] : [alice, bob], has_more: false,
    }));
    renderInbox();
    await conversationList().findByRole("button", { name: /Bob/ });

    fireEvent.click(screen.getByRole("button", { name: "Need reply" }));
    await waitFor(() => expect(api.listInboxConversationPage).toHaveBeenLastCalledWith(
      auth, inbox.id, expect.objectContaining({ needsReply: true, status: "active" }),
    ));
    expect(conversationList().queryByRole("button", { name: /Bob/ })).not.toBeInTheDocument();
    const editor = await openConversation("Alice");
    await waitFor(() => expect(api.markOperatorConversationRead).toHaveBeenCalledWith(auth, alice.id));
    await waitFor(() => expect(screen.queryByLabelText("Unread customer messages: 3")).not.toBeInTheDocument());
    expect(conversationList().getByRole("button", { name: /Alice/ })).toHaveAttribute("aria-current", "true");
    expect(editor).toBeEnabled();
    expect(screen.getByRole("button", { name: "Need reply" })).toHaveAttribute("aria-pressed", "true");
  });

  it("removes an answered row from the reply queue while keeping its chat open for resolution", async () => {
    let sent = false;
    const waitingBob = { ...bob, awaiting_reply_since: alice.awaiting_reply_since };
    const answeredAlice = { ...alice, awaiting_reply_since: null, unread_customer_messages: 0 };
    vi.mocked(api.listInboxConversationPage).mockImplementation(async (_auth, _inboxId, options) => ({
      items: options?.includeConversationId === alice.id && options.status === "all"
        ? [answeredAlice, past]
        : sent && options?.needsReply ? [waitingBob] : [alice, waitingBob],
      has_more: false,
    }));
    vi.mocked(api.sendOperatorMessage).mockImplementation(async () => {
      sent = true;
      return message(alice.id, "Answer sent");
    });
    vi.mocked(api.listOperatorMessages).mockImplementation(async (_auth, id) => (
      sent && id === alice.id ? [message(alice.id, "Answer sent")] : []
    ));
    renderInbox();
    await conversationList().findByRole("button", { name: /Alice/ });
    fireEvent.click(screen.getByRole("button", { name: "Need reply" }));
    fireEvent.change(await openConversation("Alice"), { target: { value: "Answer sent" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(conversationList().queryByRole("button", { name: /Alice/ })).not.toBeInTheDocument());
    expect(conversationList().getByRole("button", { name: /Bob/ })).not.toHaveAttribute("aria-current", "true");
    const chat = within(screen.getByRole("region", { name: "Conversation messages" }));
    expect(chat.getByText("Alice")).toBeInTheDocument();
    expect(chat.getByText("Answer sent")).toBeInTheDocument();
    expect(chat.getByRole("button", { name: "Resolve" })).toBeEnabled();
    expect(chat.getByRole("textbox", { name: "Reply" })).toHaveValue("");
    expect(screen.getByRole("button", { name: "Need reply" })).toHaveAttribute("aria-pressed", "true");
    expect(api.listInboxConversationPage).toHaveBeenCalledWith(
      auth, inbox.id, expect.objectContaining({ includeConversationId: alice.id, status: "all" }),
    );
  });

  it("opens contact-scoped history and selects a past conversation across the current filters", async () => {
    vi.mocked(api.listInboxConversationPage).mockImplementation(async (_auth, _inboxId, options) => ({
      items: options?.contactId ? [alice, past] : options?.includeConversationId === past.id ? [alice, past, bob] : [alice, bob],
      has_more: false,
    }));
    vi.mocked(api.listOperatorMessages).mockImplementation(async (_auth, id) => id === past.id ? [message(id, "Past transcript")] : []);
    renderInbox();
    await openConversation("Alice");
    fireEvent.change(screen.getByRole("combobox", { name: "Conversation channel" }), { target: { value: alice.channel.id } });
    fireEvent.click(screen.getByRole("button", { name: "Mine" }));

    const detailsToggle = screen.getByRole("button", { name: "Details" });
    expect(detailsToggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(detailsToggle);
    expect(detailsToggle).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(screen.getByRole("button", { name: "Hide contact details" }));
    expect(detailsToggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(detailsToggle);
    fireEvent.click(screen.getByRole("button", { name: "Conversations: 2" }));

    await waitFor(() => expect(api.listInboxConversationPage).toHaveBeenCalledWith(
      auth, inbox.id, expect.objectContaining({ contactId: alice.contact_id, status: "all" }),
    ));
    fireEvent.click(await screen.findByRole("button", { name: /Previous request/ }));
    expect(await screen.findByText("Past transcript")).toBeInTheDocument();
    await waitFor(() => expect(api.listInboxConversationPage).toHaveBeenCalledWith(
      auth, inbox.id, expect.objectContaining({
        includeConversationId: past.id, status: "all", needsReply: undefined, mine: undefined, channelId: undefined,
      }),
    ));
    expect(screen.getByRole("combobox", { name: "Conversation status" })).toHaveValue("all");
    expect(screen.getByRole("combobox", { name: "Conversation channel" })).toHaveValue("all");
    expect(screen.getByRole("button", { name: "Details" })).toHaveAttribute("aria-expanded", "false");
    expect(screen.getByRole("textbox", { name: "Reply" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Reopen" })).toBeInTheDocument();
  });

  it("shows all available Telegram identity metadata in contact details", async () => {
    vi.mocked(api.listInboxConversationPage).mockResolvedValue({
      items: [telegramContact], has_more: false,
    });
    renderInbox();
    await openConversation("Ginotix");
    fireEvent.click(screen.getByRole("button", { name: "Details" }));

    const details = screen.getByRole("complementary", { name: "Contact" });
    expect(within(details).getByText("@ginotix")).toBeInTheDocument();
    expect(within(details).getByText("Telegram user ID")).toBeInTheDocument();
    expect(within(details).getAllByText("7795261514")).toHaveLength(2);
    expect(within(details).getByText("Telegram chat ID")).toBeInTheDocument();
    expect(within(details).getByText("Telegram language")).toBeInTheDocument();
    expect(within(details).getByText("en")).toBeInTheDocument();
  });

  it("keeps drafts per conversation and clears the sent draft after switching during a send", async () => {
    let finishSending!: (sent: api.Message) => void;
    vi.mocked(api.sendOperatorMessage).mockImplementation(() => new Promise((resolve) => { finishSending = resolve; }));
    renderInbox();

    fireEvent.change(await openConversation("Alice"), { target: { value: "Alice draft" } });
    const bobEditor = await openConversation("Bob");
    expect(bobEditor).toHaveValue("");
    fireEvent.change(bobEditor, { target: { value: "Bob draft" } });
    expect(await openConversation("Alice")).toHaveValue("Alice draft");
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(api.sendOperatorMessage).toHaveBeenCalledWith(auth, alice.id, "Alice draft", "operator", "markdown");
    expect(await openConversation("Bob")).toHaveValue("Bob draft");

    await act(async () => finishSending(message(alice.id, "Alice draft")));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Reply" })).toBeEnabled());
    expect(screen.getByRole("textbox", { name: "Reply" })).toHaveValue("Bob draft");
    expect(await openConversation("Alice")).toHaveValue("");
  });

  it("loads an explicitly requested resolved conversation without an active-status filter", async () => {
    vi.mocked(api.listInboxConversationPage).mockImplementation(async (_auth, _inboxId, options) => ({
      items: options?.status === "active" ? [] : [past], has_more: false,
    }));
    renderInbox(past.id);

    expect(await screen.findByRole("button", { name: "Reopen" })).toBeInTheDocument();
    expect(api.listInboxConversationPage).toHaveBeenCalledWith(
      auth, inbox.id, expect.objectContaining({ includeConversationId: past.id, status: "all" }),
    );
  });
});
