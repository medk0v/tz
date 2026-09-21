/** @jsxImportSource preact */
/// <reference types="node" />

import { readFileSync } from "node:fs";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/preact";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./WidgetApp";
import * as api from "./api";
import * as notificationSound from "./widget-notification-sound";
import { normalizeWidgetTheme } from "./widget-theme";

const widgetStyles = readFileSync("src/widget-styles.css", "utf8");

const parentDescriptor = Object.getOwnPropertyDescriptor(window, "parent");
const { launcherConfig, widgetPresentation, widgetSession } = vi.hoisted(() => {
  const launcherConfig = {
    launcher_type: "icon_text" as const,
    position: "bottom_left" as const,
    label: "Talk to support",
    show_greeting: true,
    show_operator_profile: false,
    offset_x: 32,
    offset_y: 24,
    attention_animation: "pulse" as const,
    animation_interval_seconds: 5,
    proactive_invitation_enabled: false,
    proactive_invitation_delay_seconds: 15,
  };

  return {
    launcherConfig,
    widgetPresentation: {
      language: "en" as const,
      support_name: "Support",
      greeting: "Welcome. How can we help?",
      offline_message: "The team is away. Leave your message and contact details.",
      rating_prompt: "How was this conversation?",
      rating_thanks: "Thank you for your feedback.",
      proactive_invitation_message: "Need help choosing?",
      online_now: "Online now",
      offline_now: "Leave a message",
      message_placeholder: "Write a message…",
      contact_title: "Introduce yourself",
      contact_description: "Optional. Leave your name and email so we can contact you about this conversation.",
      contact_name: "Name",
      contact_name_placeholder: "Your name",
      contact_email: "Email",
      contact_email_placeholder: "you@example.com",
      contact_save: "Save details",
      contact_saving: "Saving…",
      contact_skip: "Continue anonymously",
      contact_error: "Could not save your contact details.",
      operators_online: true,
      attachments_enabled: false,
      launcher: launcherConfig,
      theme: {
        accent_color: "#F05A28",
        accent_text_color: "#FFFFFF",
        surface_color: "#FFFFFF",
        text_color: "#1B1B1D",
        dark: {
          accent_color: "#E9683D",
          accent_text_color: "#FFFFFF",
          surface_color: "#1C1E21",
          text_color: "#F2F3F4",
        },
        border: {
          enabled: true,
          width: 2 as const,
          light_color: "#E3E4E6",
          dark_color: "#51545A",
        },
        border_radius: 20,
        footer_text: null,
      },
    },
    widgetSession: {
      session_id: "00000000-0000-0000-0000-000000000001",
      token: "widget-token",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      language: "en" as const,
      support_name: "Support",
      greeting: "Welcome. How can we help?",
      offline_message: "The team is away. Leave your message and contact details.",
      rating_prompt: "How was this conversation?",
      rating_thanks: "Thank you for your feedback.",
      proactive_invitation_message: "Need help choosing?",
      online_now: "Online now",
      offline_now: "Leave a message",
      message_placeholder: "Write a message…",
      contact_title: "Introduce yourself",
      contact_description: "Optional. Leave your name and email so we can contact you about this conversation.",
      contact_name: "Name",
      contact_name_placeholder: "Your name",
      contact_email: "Email",
      contact_email_placeholder: "you@example.com",
      contact_save: "Save details",
      contact_saving: "Saving…",
      contact_skip: "Continue anonymously",
      contact_error: "Could not save your contact details.",
      operators_online: true,
      attachments_enabled: false,
      launcher: {
        launcher_type: "icon" as const,
        position: "bottom_right" as const,
        label: "Open support",
        show_greeting: true,
        show_operator_profile: false,
        offset_x: 20,
        offset_y: 20,
        attention_animation: "pulse" as const,
        animation_interval_seconds: 5,
        proactive_invitation_enabled: false,
        proactive_invitation_delay_seconds: 15,
      },
      theme: {
        accent_color: "#F05A28",
        accent_text_color: "#FFFFFF",
        surface_color: "#FFFFFF",
        text_color: "#1B1B1D",
        dark: {
          accent_color: "#E9683D",
          accent_text_color: "#FFFFFF",
          surface_color: "#1C1E21",
          text_color: "#F2F3F4",
        },
        border: {
          enabled: true,
          width: 2 as const,
          light_color: "#E3E4E6",
          dark_color: "#51545A",
        },
        border_radius: 20,
        footer_text: null,
      },
      contact: {
        display_name: null,
        email: null,
      },
      expires_at: "2099-08-01T00:00:00Z",
    },
  };
});

const russianInterfaceTranslation = {
  online_now: "Сейчас в сети",
  offline_now: "Оставьте сообщение",
  message_placeholder: "Напишите сообщение…",
  contact_title: "Представьтесь",
  contact_description: "Необязательно. Оставьте имя и email, чтобы мы могли связаться с вами по этому диалогу.",
  contact_name: "Имя",
  contact_name_placeholder: "Ваше имя",
  contact_email: "Email",
  contact_email_placeholder: "you@example.com",
  contact_save: "Сохранить",
  contact_saving: "Сохраняем…",
  contact_skip: "Продолжить анонимно",
  contact_error: "Не удалось сохранить контактные данные.",
};

vi.mock("./api", () => ({
  ApiRequestError: class ApiRequestError extends Error {
    constructor(message: string, readonly status: number) {
      super(message);
      this.name = "ApiRequestError";
    }
  },
  resolveAvatarUrl: vi.fn((value: string | null) => value),
  createWidgetSession: vi.fn().mockResolvedValue(widgetSession),
  getWidgetPresentation: vi.fn().mockResolvedValue(widgetPresentation),
  createWidgetConversation: vi.fn().mockResolvedValue({
    id: "00000000-0000-0000-0000-000000000003",
    inbox_id: "00000000-0000-0000-0000-000000000002",
    contact_id: "00000000-0000-0000-0000-000000000004",
    status: "new",
    last_message_sequence: 0,
    created_at: "2026-08-01T00:00:00Z",
    updated_at: "2026-08-01T00:00:00Z",
    operator: null,
    ai_agent: null,
    widget_attachments_enabled: false,
  }),
  listWidgetMessages: vi.fn().mockResolvedValue([]),
  markWidgetMessagesRead: vi.fn().mockResolvedValue(undefined),
  createWidgetRealtimeTicket: vi.fn().mockResolvedValue("ticket"),
  openRealtimeSocket: vi.fn().mockReturnValue({ close: vi.fn() }),
  sendWidgetMessage: vi.fn().mockResolvedValue({
    id: "00000000-0000-0000-0000-000000000005",
    conversation_id: "00000000-0000-0000-0000-000000000003",
    sequence: 1,
    direction: "inbound",
    kind: "text",
    author_kind: "contact",
    body: "Hello",
    status: "queued",
    created_at: "2026-08-01T00:00:00Z",
    attachments: [],
  }),
  uploadWidgetAttachment: vi.fn(),
  downloadWidgetAttachment: vi.fn(),
  rateResolution: vi.fn(),
  updateWidgetContact: vi.fn().mockResolvedValue({
    display_name: "Anna Petrova",
    email: "anna@example.com",
  }),
  updateWidgetPresence: vi.fn().mockResolvedValue({ operators_online: true }),
  updateWidgetDraft: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("./widget-notification-sound", () => ({
  enableWidgetNotificationSound: vi.fn().mockResolvedValue(undefined),
  playWidgetNotificationSound: vi.fn(),
  supportsWidgetNotificationSound: vi.fn().mockReturnValue(true),
}));

describe("Widget App", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    window.history.replaceState({}, "", "/widget.html?widget_id=00000000-0000-0000-0000-000000000010");
    sessionStorage.clear();
    localStorage.clear();
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    if (parentDescriptor) {
      Object.defineProperty(window, "parent", parentDescriptor);
    } else {
      Reflect.deleteProperty(window, "parent");
    }
  });

  describe("customer read receipts", () => {
    const reply: api.Message = {
      id: "visible-reply",
      conversation_id: "00000000-0000-0000-0000-000000000003",
      sequence: 1,
      direction: "outbound",
      author_kind: "operator",
      kind: "text",
      body: "A visible reply",
      status: "sent",
      created_at: "2026-09-02T00:00:00Z",
      attachments: [],
    };

    beforeEach(() => {
      vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
      vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
        if (this.classList.contains("widget-messages")) return new DOMRect(0, 0, 300, 400);
        return new DOMRect(10, this.dataset.readMessageId === "offscreen-reply" ? 500 : 100, 50, 20);
      });
    });

    afterEach(() => vi.restoreAllMocks());

    it("only acknowledges visible outgoing messages and retries failed receipts", async () => {
      vi.mocked(api.listWidgetMessages).mockResolvedValueOnce([
        reply,
        { ...reply, id: "offscreen-reply", sequence: 2 },
        { ...reply, id: "customer-message", direction: "inbound", author_kind: "contact", sequence: 3 },
        { ...reply, id: "failed-reply", status: "failed", sequence: 4 },
        { ...reply, id: "read-reply", status: "read", sequence: 5 },
      ]);
      vi.mocked(api.markWidgetMessagesRead).mockRejectedValueOnce(new Error("Offline"));
      render(<App />);
      await waitFor(() => expect(api.markWidgetMessagesRead).toHaveBeenCalledWith(
        "widget-token", reply.conversation_id, [reply.id],
      ));
      const container = document.querySelector(".widget-messages")!;
      fireEvent.scroll(container);
      await waitFor(() => expect(api.markWidgetMessagesRead).toHaveBeenCalledTimes(2));
      fireEvent.scroll(container);
      expect(api.markWidgetMessagesRead).toHaveBeenCalledTimes(2);
      expect(api.markWidgetMessagesRead).toHaveBeenLastCalledWith("widget-token", reply.conversation_id, [reply.id]);
    });

    it("waits for the embedded widget to open and the tab to become visible", async () => {
      const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
      const parentWindow = { postMessage: vi.fn() } as unknown as Window;
      Object.defineProperty(window, "parent", { configurable: true, value: parentWindow });
      vi.mocked(api.listWidgetMessages).mockResolvedValueOnce([reply]);
      render(<App />);
      fireEvent(window, new MessageEvent("message", {
        source: parentWindow,
        origin: "https://host.example",
        data: {
          type: "tz:widget-bootstrap", version: 1,
          embed_origin: "https://host.example", session: widgetSession,
        },
      }));
      const launcher = await screen.findByRole("button", { name: "Talk to support" });
      expect(api.markWidgetMessagesRead).not.toHaveBeenCalled();
      fireEvent.click(launcher);
      await screen.findByText(reply.body);
      expect(api.markWidgetMessagesRead).not.toHaveBeenCalled();
      visibility.mockReturnValue("visible");
      fireEvent(document, new Event("visibilitychange"));
      await waitFor(() => expect(api.markWidgetMessagesRead).toHaveBeenCalledExactlyOnceWith(
        "widget-token", reply.conversation_id, [reply.id],
      ));
    });
  });

  it("opens the collapsed widget when the operator sends the first message", async () => {
    const parentPostMessage = vi.fn();
    const parentWindow = { postMessage: parentPostMessage } as unknown as Window;
    Object.defineProperty(window, "parent", { configurable: true, value: parentWindow });
    render(<App />);
    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: widgetSession,
      },
    }));
    await screen.findByRole("button", { name: "Talk to support" });
    await waitFor(() => expect(api.openRealtimeSocket).toHaveBeenCalled());
    expect(document.querySelector(".widget-shell")).toBeNull();
    const message: api.Message = {
      id: "operator-first-message",
      conversation_id: "00000000-0000-0000-0000-000000000003",
      sequence: 1,
      direction: "outbound",
      kind: "text",
      author_kind: "operator",
      body: "Can I help you choose?",
      status: "sent",
      created_at: "2026-09-02T00:00:00Z",
      attachments: [],
    };
    vi.mocked(api.listWidgetMessages).mockResolvedValueOnce([message]);
    const receive = vi.mocked(api.openRealtimeSocket).mock.calls.at(-1)?.[1];
    expect(receive).toBeDefined();
    act(() => receive?.({
      event_id: "first-message-event",
      inbox_id: widgetSession.inbox_id,
      type: "message.created",
      aggregate_id: message.id,
      data: { conversation_id: message.conversation_id, message_id: message.id, direction: "outbound" },
    }));
    expect(await screen.findByText(message.body)).toBeInTheDocument();
    expect(parentPostMessage).toHaveBeenCalledWith(expect.objectContaining({ type: "tz:widget-resize", open: true }), "https://host.example");
  });

  it("renders operator Markdown replies and keeps plain and legacy replies literal", async () => {
    const reply: api.Message = {
      id: "formatted-operator-reply",
      conversation_id: "00000000-0000-0000-0000-000000000003",
      sequence: 1,
      direction: "outbound",
      kind: "text",
      author_kind: "operator",
      body: "**Reply ready**\n\n- Check the amount\n- Confirm the address\n\n[Open help](https://example.com/help)",
      body_format: "markdown",
      status: "sent",
      created_at: "2026-09-02T00:00:00Z",
      attachments: [],
    };
    const plainBody = "**Plain stays literal** [Plain link](https://example.com/plain)";
    const legacyBody = "**Legacy stays literal** [Legacy link](https://example.com/legacy)";
    vi.mocked(api.listWidgetMessages).mockResolvedValueOnce([
      reply,
      { ...reply, id: "plain-operator-reply", sequence: 2, body: plainBody, body_format: "plain" },
      { ...reply, id: "legacy-operator-reply", sequence: 3, body: legacyBody, body_format: undefined },
    ]);

    render(<App />);

    const bold = await screen.findByText("Reply ready");
    expect(bold.tagName).toBe("STRONG");
    const formattedReply = bold.closest("article");
    expect(formattedReply?.querySelectorAll("ul > li")).toHaveLength(2);
    expect(screen.getByText("Check the amount").tagName).toBe("LI");
    const link = screen.getByRole("link", { name: "Open help" });
    expect(link).toHaveAttribute("href", "https://example.com/help");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
    for (const body of [plainBody, legacyBody]) {
      const literalReply = screen.getByText(body).closest("article");
      expect(literalReply?.querySelector("strong, a")).toBeNull();
    }
  });

  it("switches to a conversation started by the operator after the previous one ended", async () => {
    render(<App />);
    await waitFor(() => expect(api.openRealtimeSocket).toHaveBeenCalled());
    const initial = await vi.mocked(api.createWidgetConversation).mock.results[0]?.value;
    vi.mocked(api.createWidgetConversation).mockResolvedValueOnce({ ...initial, id: "new-conversation" });
    const receive = vi.mocked(api.openRealtimeSocket).mock.calls.at(-1)?.[1];
    act(() => receive?.({
      event_id: "new-conversation-event",
      inbox_id: widgetSession.inbox_id,
      type: "conversation.created",
      aggregate_id: "new-conversation",
      data: { conversation_id: "new-conversation" },
    }));
    await waitFor(() => expect(api.listWidgetMessages).toHaveBeenCalledWith("widget-token", "new-conversation"));
    fireEvent.input(screen.getByLabelText("Message"), { target: { value: "Yes, please" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(api.sendWidgetMessage).toHaveBeenCalledWith("widget-token", "new-conversation", "Yes, please"));
  });

  it("starts a widget session and sends a customer message", async () => {
    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    expect(api.updateWidgetPresence).toHaveBeenCalledWith("widget-token");
    fireEvent.input(screen.getByLabelText("Message"), { target: { value: "Hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    const sentMessage = (await screen.findByText("Hello")).closest("article");
    expect(sentMessage?.querySelector(".widget-message-author")).toBeNull();
    expect(api.createWidgetSession).toHaveBeenCalledWith(
      "00000000-0000-0000-0000-000000000010",
      expect.any(String),
      undefined,
      expect.objectContaining({ schema_version: 1 }),
    );
  });

  it("keeps long unbroken messages inside the conversation viewport", () => {
    expect(widgetStyles).toMatch(
      /\.widget-messages\s*\{[^}]*overflow-x:\s*hidden;/s,
    );
    expect(widgetStyles).toMatch(
      /\.widget-message\s*\{[^}]*overflow-wrap:\s*anywhere;/s,
    );
  });

  it("applies the operator file-access policy change in realtime", async () => {
    const conversation: api.Conversation = {
      id: "00000000-0000-0000-0000-000000000003",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      status: "open",
      last_message_sequence: 0,
      created_at: "2026-08-15T00:00:00Z",
      updated_at: "2026-08-15T00:00:00Z",
      operator: null,
      ai_agent: null,
      widget_attachments_enabled: true,
    };
    vi.mocked(api.createWidgetConversation)
      .mockResolvedValueOnce(conversation)
      .mockResolvedValueOnce({ ...conversation, widget_attachments_enabled: false });
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementationOnce((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);

    const attachmentButton = await screen.findByRole("button", { name: "Attach a file" });
    await waitFor(() => expect(onRealtime).toBeDefined());
    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000072",
      inbox_id: conversation.inbox_id,
      contact_id: conversation.contact_id,
      type: "conversation.attachment_policy_changed",
      aggregate_id: conversation.id,
      data: { conversation_id: conversation.id, attachments_enabled: false },
    }));

    await waitFor(() => expect(screen.queryByRole("button", { name: "Attach a file" })).toBeNull());
    expect(attachmentButton).not.toBeInTheDocument();
  });

  it("lets the visitor save their name and email", async () => {
    render(<App />);

    expect(await screen.findByText("Introduce yourself")).toBeInTheDocument();
    fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "Anna Petrova" },
    });
    fireEvent.input(screen.getByLabelText("Email"), {
      target: { value: "Anna@Example.com" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save details" }));

    await waitFor(() => expect(api.updateWidgetContact).toHaveBeenCalledWith(
      "widget-token",
      { display_name: "Anna Petrova", email: "Anna@Example.com" },
    ));
    await waitFor(() => expect(screen.queryByText("Introduce yourself")).not.toBeInTheDocument());

    fireEvent.input(screen.getByLabelText("Message"), { target: { value: "Hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    const sentMessage = (await screen.findByText("Hello")).closest("article");
    expect(sentMessage?.querySelector(".widget-message-author")).toHaveTextContent("Anna Petrova");
  });

  it("uses the widget-configured contact and composer translations", async () => {
    vi.mocked(api.updateWidgetContact).mockRejectedValueOnce(new Error("internal error"));
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      online_now: "Available right now",
      message_placeholder: "Type your question here…",
      contact_title: "Tell us who you are",
      contact_description: "Share your details so we can follow up.",
      contact_name: "Full name",
      contact_name_placeholder: "Jane Example",
      contact_email: "Contact email",
      contact_email_placeholder: "jane@example.com",
      contact_save: "Keep details",
      contact_saving: "Keeping…",
      contact_skip: "Chat without details",
      contact_error: "Details could not be kept.",
    });

    render(<App />);

    expect(await screen.findByText("Tell us who you are")).toBeInTheDocument();
    expect(screen.getByText("Available right now")).toBeInTheDocument();
    expect(screen.getByText("Share your details so we can follow up.")).toBeInTheDocument();
    expect(screen.getByLabelText("Full name")).toHaveAttribute("placeholder", "Jane Example");
    expect(screen.getByLabelText("Contact email")).toHaveAttribute("placeholder", "jane@example.com");
    expect(screen.getByRole("button", { name: "Keep details" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Chat without details" })).toBeInTheDocument();
    expect(screen.getByLabelText("Message")).toHaveAttribute("placeholder", "Type your question here…");

    fireEvent.input(screen.getByLabelText("Full name"), { target: { value: "Jane Example" } });
    fireEvent.input(screen.getByLabelText("Contact email"), { target: { value: "jane@example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Keep details" }));
    expect(await screen.findByText("Details could not be kept.")).toBeInTheDocument();
  });

  it("does not label an anonymous Russian visitor", async () => {
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      language: "ru",
      ...russianInterfaceTranslation,
    });

    render(<App />);

    expect(await screen.findByText("Представьтесь")).toBeInTheDocument();
    fireEvent.input(screen.getByLabelText("Сообщение"), { target: { value: "Привет" } });
    fireEvent.click(screen.getByRole("button", { name: "Отправить" }));
    const sentMessage = (await screen.findByText("Hello")).closest("article");
    expect(sentMessage?.querySelector(".widget-message-author")).toBeNull();
  });

  it("lets the visitor dismiss the contact form and continue anonymously", async () => {
    render(<App />);

    expect(await screen.findByText("Introduce yourself")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Skip contact details" }));

    expect(screen.queryByText("Introduce yourself")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Message")).toBeInTheDocument();
    expect(localStorage.getItem(
      "tz:widget-contact-form:00000000-0000-0000-0000-000000000010",
    )).toBe("00000000-0000-0000-0000-000000000003");
    expect(api.updateWidgetContact).not.toHaveBeenCalled();
  });

  it("keeps the contact form dismissed only for the conversation where it was closed", async () => {
    const storageKey = "tz:widget-contact-form:00000000-0000-0000-0000-000000000010";
    localStorage.setItem(storageKey, "00000000-0000-0000-0000-000000000003");

    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    expect(screen.queryByText("Introduce yourself")).not.toBeInTheDocument();

    cleanup();
    vi.mocked(api.createWidgetConversation).mockResolvedValueOnce({
      id: "00000000-0000-0000-0000-000000000006",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      status: "new",
      last_message_sequence: 0,
      created_at: "2026-08-15T09:10:00Z",
      updated_at: "2026-08-15T09:10:00Z",
      operator: null,
      ai_agent: null,
      widget_attachments_enabled: false,
    });

    render(<App />);

    expect(await screen.findByText("Introduce yourself")).toBeInTheDocument();
    expect(localStorage.getItem(storageKey)).toBe(
      "00000000-0000-0000-0000-000000000003",
    );
  });

  it("updates the operator profile after realtime assignment and departure", async () => {
    const conversation = {
      id: "00000000-0000-0000-0000-000000000003",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      status: "new" as const,
      last_message_sequence: 0,
      created_at: "2026-08-01T00:00:00Z",
      updated_at: "2026-08-01T00:00:00Z",
      operator: null,
      ai_agent: null,
      widget_attachments_enabled: false,
    };
    vi.mocked(api.createWidgetConversation)
      .mockResolvedValueOnce(conversation)
      .mockResolvedValueOnce({
        ...conversation,
        status: "open",
        operator: {
          display_name: "Anna Petrova",
          avatar_url: "https://cdn.example/anna.jpg",
          joined_at: "2026-08-14T05:55:00Z",
        },
      })
      .mockResolvedValueOnce({ ...conversation, status: "open" });
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      launcher: { ...launcherConfig, show_operator_profile: true },
    });
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);
    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000050",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      type: "conversation.operator_joined",
      aggregate_id: conversation.id,
      data: { conversation_id: conversation.id },
    }));

    expect(await screen.findAllByText("Anna Petrova")).toHaveLength(2);
    expect(screen.getAllByText("Joined the chat")).toHaveLength(2);
    expect(screen.getByText("Operator Anna Petrova joined the chat")).toBeInTheDocument();
    await waitFor(() => expect(document.querySelector<HTMLImageElement>(
      ".widget-brand-mark img",
    )).toHaveAttribute("src", "https://cdn.example/anna.jpg"));
    await waitFor(() => expect(api.openRealtimeSocket).toHaveBeenCalledTimes(2));
    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000051",
      inbox_id: conversation.inbox_id,
      contact_id: conversation.contact_id,
      type: "conversation.operator_left",
      aggregate_id: conversation.id,
      data: { conversation_id: conversation.id },
    }));
    await waitFor(() => {
      expect(document.querySelector(".widget-title strong")).toHaveTextContent("Support");
      expect(document.querySelector(".widget-brand-mark--participant img")).not.toBeInTheDocument();
    });
  });

  it("shows Julia after the first customer message and keeps Leo's takeover in the timeline", async () => {
    const conversation: api.Conversation = {
      id: "00000000-0000-0000-0000-000000000003",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      status: "open",
      last_message_sequence: 1,
      created_at: "2026-08-14T05:51:00Z",
      updated_at: "2026-08-14T05:51:00Z",
      operator: null,
      ai_agent: null,
      widget_attachments_enabled: false,
    };
    const aiAgent: api.ConversationAiAgent = {
      display_name: "Julia",
      avatar_url: "https://cdn.example/julia.jpg",
      joined_at: "2026-08-14T05:52:00Z",
      active: true,
    };
    const customerMessage: api.Message = {
      id: "00000000-0000-4000-8000-000000000059",
      conversation_id: conversation.id,
      sequence: 1,
      direction: "inbound",
      kind: "text",
      author_kind: "contact",
      body: "hey",
      status: "queued",
      created_at: "2026-08-14T05:51:30Z",
      attachments: [],
    };
    const aiReply: api.Message = {
      id: "00000000-0000-4000-8000-000000000060",
      conversation_id: conversation.id,
      sequence: 2,
      direction: "outbound",
      kind: "text",
      author_kind: "ai",
      body: "I can help with your order.",
      status: "sent",
      created_at: "2026-08-14T05:53:00Z",
      attachments: [],
    };
    vi.mocked(api.createWidgetConversation)
      .mockResolvedValueOnce(conversation)
      .mockResolvedValueOnce({
        ...conversation,
        status: "open",
        ai_agent: aiAgent,
      })
      .mockResolvedValueOnce({
        ...conversation,
        status: "open",
        ai_agent: { ...aiAgent, active: false },
      })
      .mockResolvedValueOnce({
        ...conversation,
        status: "open",
        operator: {
          display_name: "Leo",
          avatar_url: "https://cdn.example/leo.jpg",
          joined_at: "2026-08-14T05:54:00Z",
        },
        ai_agent: { ...aiAgent, active: false },
      });
    vi.mocked(api.listWidgetMessages)
      .mockResolvedValueOnce([customerMessage])
      .mockResolvedValueOnce([customerMessage, aiReply]);
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      launcher: { ...launcherConfig, show_operator_profile: true },
    });
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);
    expect(await screen.findByText("hey")).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000061",
      inbox_id: conversation.inbox_id,
      contact_id: conversation.contact_id,
      type: "conversation.ai_joined",
      aggregate_id: conversation.id,
      data: { conversation_id: conversation.id },
    }));

    await waitFor(() => {
      expect(document.querySelector(".widget-title strong")).toHaveTextContent("Julia");
      expect(document.querySelector(".widget-title span")).toHaveTextContent("Joined the chat");
    });
    const initialTimeline = document.querySelectorAll(".widget-message, .widget-participant-event");
    expect(initialTimeline[0]).toHaveTextContent("hey");
    expect(initialTimeline[1]).toHaveTextContent("Operator Julia joined the chat");
    expect(document.querySelector<HTMLImageElement>(
      ".widget-brand-mark--participant img",
    )).toHaveAttribute("src", "https://cdn.example/julia.jpg");
    await waitFor(() => expect(api.openRealtimeSocket).toHaveBeenCalledTimes(2));

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000062",
      inbox_id: conversation.inbox_id,
      contact_id: conversation.contact_id,
      type: "message.created",
      aggregate_id: aiReply.id,
      data: {
        conversation_id: conversation.id,
        message_id: aiReply.id,
        direction: "outbound",
      },
    }));

    const aiReplyMessage = (await screen.findByText(aiReply.body)).closest("article");
    expect(aiReplyMessage?.querySelector(".widget-message-author")).toHaveTextContent(
      "Julia Operator",
    );
    expect(document.querySelector(".widget-shell")).not.toHaveTextContent("AI assistant");

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000063",
      inbox_id: conversation.inbox_id,
      contact_id: conversation.contact_id,
      type: "conversation.ai_left",
      aggregate_id: conversation.id,
      data: { conversation_id: conversation.id },
    }));

    await waitFor(() => {
      expect(document.querySelector(".widget-title strong")).toHaveTextContent("Support");
    });

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000064",
      inbox_id: conversation.inbox_id,
      contact_id: conversation.contact_id,
      type: "conversation.operator_joined",
      aggregate_id: conversation.id,
      data: { conversation_id: conversation.id },
    }));

    await waitFor(() => {
      expect(document.querySelector(".widget-title strong")).toHaveTextContent("Leo");
      expect(document.querySelector(".widget-title span")).toHaveTextContent("Joined the chat");
    });
    expect(document.querySelector<HTMLImageElement>(
      ".widget-brand-mark--participant img",
    )).toHaveAttribute("src", "https://cdn.example/leo.jpg");
    expect(screen.getByText("Operator Julia joined the chat")).toBeInTheDocument();
    expect(screen.getByText("Operator Leo joined the chat")).toBeInTheDocument();
  });

  it("keeps the generic support identity when operator profiles are hidden", async () => {
    const conversation = {
      id: "00000000-0000-0000-0000-000000000003",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      status: "new" as const,
      last_message_sequence: 0,
      created_at: "2026-08-01T00:00:00Z",
      updated_at: "2026-08-01T00:00:00Z",
      operator: null,
      ai_agent: null,
      widget_attachments_enabled: false,
    };
    vi.mocked(api.createWidgetConversation)
      .mockResolvedValueOnce(conversation)
      .mockResolvedValueOnce({
        ...conversation,
        status: "open",
        operator: {
          display_name: "Anna Petrova",
          avatar_url: "https://cdn.example/anna.jpg",
          joined_at: "2026-08-14T05:55:00Z",
        },
      });
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementationOnce((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);
    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000051",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      type: "conversation.operator_joined",
      aggregate_id: conversation.id,
      data: { conversation_id: conversation.id },
    }));

    await waitFor(() => expect(api.createWidgetConversation).toHaveBeenCalledTimes(2));
    expect(screen.queryByText("Anna Petrova")).not.toBeInTheDocument();
    expect(screen.queryByText("Joined the chat")).not.toBeInTheDocument();
    expect(screen.getAllByText("Support")).toHaveLength(2);
    expect(document.querySelector(".widget-brand-mark img")).toBeNull();
  });

  it("does not request a rating for an automatically resolved empty conversation", async () => {
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementationOnce((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);
    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000052",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      type: "conversation.resolved",
      aggregate_id: "00000000-0000-0000-0000-000000000003",
      data: {
        conversation_id: "00000000-0000-0000-0000-000000000003",
        automatic: true,
        reason: "empty_conversation_timeout",
      },
    }));

    await waitFor(() => expect(api.listWidgetMessages).toHaveBeenCalledTimes(2));
    expect(screen.queryByText("How was this conversation?")).not.toBeInTheDocument();
  });

  it("restores the message composer when an operator reopens the conversation", async () => {
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementationOnce((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });
    render(<App />);
    await screen.findByText("Welcome. How can we help?");
    await waitFor(() => expect(onRealtime).toBeDefined());

    const conversationId = "00000000-0000-0000-0000-000000000003";
    act(() => onRealtime?.({
      event_id: "resolved-event",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      type: "conversation.resolved",
      aggregate_id: conversationId,
      data: { conversation_id: conversationId, resolution_id: "resolution-1" },
    }));
    expect(await screen.findByText("How was this conversation?")).toBeInTheDocument();
    expect(screen.queryByLabelText("Message")).not.toBeInTheDocument();

    act(() => onRealtime?.({
      event_id: "reopened-event",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      type: "conversation.reopened",
      aggregate_id: conversationId,
      data: { conversation_id: conversationId },
    }));
    expect(await screen.findByLabelText("Message")).toBeEnabled();
    expect(screen.queryByText("How was this conversation?")).not.toBeInTheDocument();
    await waitFor(() => expect(api.createWidgetConversation).toHaveBeenCalledTimes(2));
  });

  it("lets the visitor rate and continue an automatic operator resolution", async () => {
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementationOnce((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);
    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000053",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      type: "conversation.resolved",
      aggregate_id: "00000000-0000-0000-0000-000000000003",
      data: {
        conversation_id: "00000000-0000-0000-0000-000000000003",
        resolution_id: null,
        rating_requested: false,
      },
    }));
    expect(screen.queryByText("How was this conversation?")).not.toBeInTheDocument();

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000054",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      contact_id: "00000000-0000-0000-0000-000000000004",
      type: "conversation.resolved",
      aggregate_id: "00000000-0000-0000-0000-000000000003",
      data: {
        conversation_id: "00000000-0000-0000-0000-000000000003",
        resolution_id: "00000000-0000-4000-8000-000000000055",
        automatic: true,
        rating_requested: true,
        reason: "operator_reply_timeout",
      },
    }));

    expect(await screen.findByText("How was this conversation?")).toBeInTheDocument();
    expect(screen.queryByLabelText("Message")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Continue conversation" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Rate 5 out of 5" }));
    fireEvent.click(screen.getByRole("button", { name: "Send feedback" }));

    await waitFor(() => expect(api.rateResolution).toHaveBeenCalledWith(
      "widget-token",
      "00000000-0000-4000-8000-000000000055",
      5,
      [],
      null,
    ));
    expect(await screen.findByText("Thank you for your feedback.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Continue conversation" }));

    expect(screen.queryByText("Thank you for your feedback.")).not.toBeInTheDocument();
    const messageField = screen.getByLabelText("Message");
    await waitFor(() => expect(messageField).toHaveFocus());
    fireEvent.input(messageField, { target: { value: "I still need help" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(api.sendWidgetMessage).toHaveBeenCalledWith(
      "widget-token",
      "00000000-0000-0000-0000-000000000003",
      "I still need help",
    ));
  });

  it("keeps the message sound preference in the widget", async () => {
    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    const soundButton = screen.getByRole("button", { name: "Message sounds" });
    expect(soundButton).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(soundButton);
    expect(soundButton).toHaveAttribute("aria-pressed", "false");
    expect(localStorage.getItem(
      "tz:widget-message-sounds:00000000-0000-0000-0000-000000000010",
    )).toBe("off");

    fireEvent.click(soundButton);
    expect(soundButton).toHaveAttribute("aria-pressed", "true");
    expect(notificationSound.enableWidgetNotificationSound).toHaveBeenCalledOnce();
    expect(localStorage.getItem(
      "tz:widget-message-sounds:00000000-0000-0000-0000-000000000010",
    )).toBe("on");
  });

  it("plays once only for a new operator realtime message", async () => {
    const oldReply: api.Message = {
      id: "00000000-0000-4000-8000-000000000030",
      conversation_id: "00000000-0000-0000-0000-000000000003",
      sequence: 1,
      direction: "outbound",
      kind: "text",
      author_kind: "operator",
      body: "Earlier operator reply",
      status: "sent",
      created_at: "2026-08-09T06:50:00Z",
      attachments: [],
    };
    const liveReply: api.Message = {
      ...oldReply,
      id: "00000000-0000-4000-8000-000000000031",
      sequence: 2,
      body: "New operator reply",
      created_at: "2026-08-09T07:00:00Z",
    };
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.listWidgetMessages)
      .mockResolvedValueOnce([oldReply])
      .mockResolvedValueOnce([oldReply, liveReply]);
    vi.mocked(api.openRealtimeSocket).mockImplementationOnce((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);

    expect(await screen.findByText("Earlier operator reply")).toBeInTheDocument();
    const messageList = document.querySelector(".widget-messages") as HTMLDivElement;
    const scrollTo = vi.fn();
    Object.defineProperty(messageList, "scrollHeight", {
      configurable: true,
      value: 720,
    });
    Object.defineProperty(messageList, "scrollTo", {
      configurable: true,
      value: scrollTo,
    });
    await waitFor(() => expect(onRealtime).toBeDefined());
    expect(notificationSound.playWidgetNotificationSound).not.toHaveBeenCalled();

    const replyEvent: api.RealtimeEvent = {
      event_id: "00000000-0000-4000-8000-000000000041",
      inbox_id: "00000000-0000-0000-0000-000000000002",
      type: "message.created",
      aggregate_id: liveReply.id,
      data: {
        message_id: liveReply.id,
        conversation_id: liveReply.conversation_id,
        direction: "outbound",
      },
    };
    act(() => onRealtime?.(replyEvent));

    expect(notificationSound.playWidgetNotificationSound).toHaveBeenCalledOnce();
    const newOperatorReply = (await screen.findByText("New operator reply")).closest("article");
    await waitFor(() => expect(scrollTo).toHaveBeenCalledWith({ top: 720, behavior: "smooth" }));
    expect(newOperatorReply).toHaveClass("widget-message--entering");
    fireEvent(newOperatorReply as HTMLElement, new Event("animationend", { bubbles: true }));
    await waitFor(() => expect(screen.getByText("New operator reply").closest("article"))
      .not.toHaveClass("widget-message--entering"));

    act(() => onRealtime?.(replyEvent));
    expect(notificationSound.playWidgetNotificationSound).toHaveBeenCalledOnce();

    fireEvent.click(screen.getByRole("button", { name: "Message sounds" }));
    act(() => onRealtime?.({
      ...replyEvent,
      event_id: "00000000-0000-4000-8000-000000000042",
      aggregate_id: "00000000-0000-4000-8000-000000000032",
      data: {
        ...replyEvent.data,
        message_id: "00000000-0000-4000-8000-000000000032",
      },
    }));
    act(() => onRealtime?.({
      ...replyEvent,
      event_id: "00000000-0000-4000-8000-000000000043",
      aggregate_id: "00000000-0000-4000-8000-000000000033",
      data: {
        ...replyEvent.data,
        message_id: "00000000-0000-4000-8000-000000000033",
        direction: "inbound",
      },
    }));
    expect(notificationSound.playWidgetNotificationSound).toHaveBeenCalledOnce();
  });

  it("applies the dark palette and configured border selected by the host site", async () => {
    window.history.replaceState(
      {},
      "",
      "/widget.html?widget_id=00000000-0000-0000-0000-000000000010&theme=dark",
    );

    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    const stage = document.querySelector(".widget-stage");
    expect(stage).toHaveAttribute("data-theme", "dark");
    expect(stage).toHaveStyle({
      "--widget-accent": "#E9683D",
      "--widget-surface": "#1C1E21",
      "--widget-text": "#F2F3F4",
      "--widget-background": "#202225",
      "--widget-control": "#1C1E21",
      "--widget-divider": "#36383A",
      "--widget-online": "#27A857",
      "--widget-danger": "#FF8B82",
      "--widget-rating": "#F3B72F",
      "--widget-border-width": "2px",
      "--widget-border-color": "#51545A",
    });
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.style.colorScheme).toBe("dark");
  });

  describe("reply typing effect", () => {
    const oldReply: api.Message = {
      id: "old-typed-reply",
      conversation_id: "00000000-0000-0000-0000-000000000003",
      sequence: 1,
      direction: "outbound",
      kind: "text",
      author_kind: "operator",
      body: "Earlier answer",
      status: "sent",
      created_at: "2026-09-10T07:00:00Z",
      attachments: [],
    };

    afterEach(() => vi.restoreAllMocks());

    async function start(replyTypingEffect = true, embedded = false) {
      vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
        ...widgetPresentation,
        theme: { ...widgetPresentation.theme, reply_typing_effect: replyTypingEffect },
      });
      vi.mocked(api.listWidgetMessages).mockResolvedValueOnce([oldReply]);
      if (embedded) {
        const parentWindow = { postMessage: vi.fn() } as unknown as Window;
        Object.defineProperty(window, "parent", { configurable: true, value: parentWindow });
        render(<App />);
        fireEvent(window, new MessageEvent("message", {
          source: parentWindow,
          origin: "https://host.example",
          data: { type: "tz:widget-bootstrap", version: 1, embed_origin: "https://host.example", session: widgetSession },
        }));
        fireEvent.click(await screen.findByRole("button", { name: "Talk to support" }));
      } else {
        render(<App />);
      }
      await screen.findByText(oldReply.body);
      await waitFor(() => expect(api.openRealtimeSocket).toHaveBeenCalled());
      const receive = vi.mocked(api.openRealtimeSocket).mock.calls.at(-1)![1];
      return (reply: api.Message) => {
        vi.mocked(api.listWidgetMessages).mockResolvedValueOnce([oldReply, reply]);
        act(() => receive({
          event_id: `event-${reply.id}`,
          inbox_id: widgetSession.inbox_id,
          type: "message.created",
          aggregate_id: reply.id,
          data: { message_id: reply.id, conversation_id: reply.conversation_id, direction: reply.direction },
        }));
      };
    }

    it.each(["operator", "ai"] as const)("reveals a new %s reply once and keeps history immediate", async (authorKind) => {
      const receive = await start();
      expect(document.querySelector(".message-typed-character")).toBeNull();
      const reply = { ...oldReply, id: "new-typed-reply", sequence: 2, body: "New answer", author_kind: authorKind };
      receive(reply);
      await waitFor(() => expect(document.querySelector(".message-typed-character")).not.toBeNull());
      expect(screen.getByText(oldReply.body).closest("article")?.querySelector(".message-typed-character")).toBeNull();
      const character = document.querySelector(".message-typed-character")!;
      const article = character.closest("article");
      fireEvent(character, new Event("animationend", { bubbles: true }));
      expect(article).toHaveClass("widget-message--entering");
      receive(reply);
      await waitFor(() => expect(api.listWidgetMessages).toHaveBeenCalledTimes(3));
      expect(document.querySelector(".message-typed-character")).toBe(character);
      receive(reply);
      await waitFor(() => expect(api.listWidgetMessages).toHaveBeenCalledTimes(4));
      expect(document.querySelector(".message-typed-character")).toBe(character);
      expect(screen.getByText(reply.body)).toBeInTheDocument();
    });

    it.each(["disabled", "reduced motion", "hidden page", "visitor"])("shows the full reply immediately for %s", async (reason) => {
      if (reason === "reduced motion") {
        vi.stubGlobal("matchMedia", vi.fn((query: string) => ({ matches: query.includes("prefers-reduced-motion") })));
      }
      if (reason === "hidden page") vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
      const receive = await start(reason !== "disabled");
      const reply: api.Message = {
        ...oldReply, id: "immediate-reply", sequence: 2, body: "Immediate answer",
        ...(reason === "visitor" ? { direction: "inbound", author_kind: "contact" } : {}),
      };
      receive(reply);
      await screen.findByText(reply.body);
      expect(document.querySelector(".message-typed-character")).toBeNull();
    });

    it("does not restart the effect when the visitor closes and reopens the chat", async () => {
      const receive = await start(true, true);
      receive({ ...oldReply, id: "close-during-typing", sequence: 2, body: "Complete answer" });
      await waitFor(() => expect(document.querySelector(".message-typed-character")).not.toBeNull());
      fireEvent.click(screen.getByRole("button", { name: "Close customer support chat" }));
      fireEvent.click(await screen.findByRole("button", { name: "Talk to support" }));
      await screen.findByText("Complete answer");
      expect(document.querySelector(".message-typed-character")).toBeNull();
    });
  });

  it("uses the selected font for the widget and falls back to it when site fonts are disabled", async () => {
    const parentWindow = { postMessage: vi.fn() } as unknown as Window;
    Object.defineProperty(window, "parent", { configurable: true, value: parentWindow });
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      theme: { ...widgetPresentation.theme, font_family: "georgia", use_site_font: true },
    });
    render(<App />);
    const fontMessage = { type: "tz:widget-site-font", version: 1, font: { family: "Verdana", faces: [] } };
    fireEvent(window, new MessageEvent("message", { source: parentWindow, origin: "https://host.example", data: fontMessage }));
    fireEvent(window, new MessageEvent("message", {
      source: parentWindow, origin: "https://host.example",
      data: { type: "tz:widget-bootstrap", version: 1, embed_origin: "https://host.example", session: widgetSession },
    }));
    await screen.findByRole("button", { name: "Talk to support" });
    const stage = document.querySelector(".widget-stage") as HTMLElement;
    expect(stage.style.fontFamily).toContain("Georgia");
    expect(stage.style.fontFamily).not.toContain("Verdana");
    await waitFor(() => expect(parentWindow.postMessage).toHaveBeenCalledWith(
      { type: "tz:widget-site-font-request", version: 1 }, "https://host.example",
    ));
    for (const source of [parentWindow, window]) {
      fireEvent(window, new MessageEvent("message", { source, origin: source === parentWindow ? "https://attacker.example" : "https://host.example", data: fontMessage }));
    }
    expect(stage.style.fontFamily).not.toContain("Verdana");
    fireEvent(window, new MessageEvent("message", { source: parentWindow, origin: "https://host.example", data: fontMessage }));
    await waitFor(() => expect(stage.style.fontFamily).toMatch(/Verdana.*Georgia/));

    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      theme: { ...widgetPresentation.theme, font_family: "courier_new", use_site_font: false },
    });
    fireEvent.focus(window);
    await waitFor(() => expect(stage.style.fontFamily).toContain("Courier New"));
    expect(stage.style.fontFamily).not.toContain("Verdana");
    fireEvent(window, new MessageEvent("message", { source: parentWindow, origin: "https://host.example", data: fontMessage }));
    expect(stage.style.fontFamily).not.toContain("Verdana");
  });

  it.each(["light", "dark"] as const)("applies independent controls and chosen icons in the %s palette", async (scheme) => {
    const parentWindow = { postMessage: vi.fn() } as unknown as Window;
    Object.defineProperty(window, "parent", { configurable: true, value: parentWindow });
    window.history.replaceState({}, "", `/widget.html?widget_id=00000000-0000-0000-0000-000000000010&theme=${scheme}`);
    const theme = normalizeWidgetTheme(widgetSession.theme);
    const colors = {
      status_text_color: "#112233",
      launcher_background_color: "#223344",
      launcher_icon_color: "#334455",
      input_background_color: "#445566",
      input_text_color: "#556677",
      input_placeholder_color: "#667788",
      footer_text_color: "#778899",
      sound_icon_color: "#8899AA",
      close_icon_color: "#99AABB",
      send_background_color: "#AABBCC",
      send_icon_color: "#BBCCDD",
    };
    const palette = scheme === "dark" ? theme.dark : theme;
    palette.component_colors = { ...palette.component_colors, ...colors };
    theme.launcher_icon = "headphones";
    theme.send_icon = "arrow_up";
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({ ...widgetPresentation, theme });

    render(<App />);
    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: { ...widgetSession, theme },
      },
    }));

    const launcher = await screen.findByRole("button", { name: "Talk to support" });
    expect(launcher.querySelector("svg")).toHaveAttribute("data-widget-icon", "headphones");
    fireEvent.click(launcher);
    await screen.findByRole("textbox", { name: "Message" });
    const stage = document.querySelector(".widget-stage");
    expect(stage).toHaveStyle({
      "--widget-status-text": "#112233",
      "--widget-launcher-background": "#223344",
      "--widget-launcher-icon": "#334455",
      "--widget-input-background": "#445566",
      "--widget-input-text": "#556677",
      "--widget-input-placeholder": "#667788",
      "--widget-footer-text": "#778899",
      "--widget-sound-icon": "#8899AA",
      "--widget-close-icon": "#99AABB",
      "--widget-send-background": "#AABBCC",
      "--widget-send-icon": "#BBCCDD",
      "--widget-control": palette.component_colors.control_background_color,
      "--widget-accent": palette.accent_color,
    });
    const sendButton = screen.getByRole("button", { name: "Send" });
    expect(sendButton.querySelector("svg")).toHaveAttribute("data-widget-icon", "arrow_up");
    expect(sendButton).toBeDisabled();
    fireEvent.input(screen.getByRole("textbox", { name: "Message" }), { target: { value: "Hello" } });
    expect(sendButton).toBeEnabled();
    expect(document.querySelector(".widget-sound-button")).toBeInTheDocument();
    expect(document.querySelector(".widget-close-button")).toBeInTheDocument();
  });

  it("switches an embedded widget palette without reloading", async () => {
    const parentWindow = { postMessage: vi.fn() } as unknown as Window;
    Object.defineProperty(window, "parent", {
      configurable: true,
      value: parentWindow,
    });

    render(<App />);
    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: widgetSession,
      },
    }));

    await screen.findByRole("button", { name: "Talk to support" });
    const stage = document.querySelector(".widget-stage");
    expect(stage).toHaveAttribute("data-theme", "light");

    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: { type: "tz:widget-theme", version: 1, theme: "dark" },
    }));

    await waitFor(() => expect(stage).toHaveAttribute("data-theme", "dark"));
    expect(document.documentElement.style.colorScheme).toBe("dark");

    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://attacker.example",
      data: { type: "tz:widget-theme", version: 1, theme: "light" },
    }));
    expect(stage).toHaveAttribute("data-theme", "dark");
  });

  it("smoothly scrolls to and animates the latest message after a successful send", async () => {
    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    const messageList = document.querySelector(".widget-messages") as HTMLDivElement;
    const scrollTo = vi.fn();
    Object.defineProperty(messageList, "scrollHeight", {
      configurable: true,
      value: 640,
    });
    Object.defineProperty(messageList, "scrollTo", {
      configurable: true,
      value: scrollTo,
    });

    fireEvent.input(screen.getByLabelText("Message"), { target: { value: "Latest" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    const sentMessage = (await screen.findByText("Hello")).closest("article");
    await waitFor(() => expect(scrollTo).toHaveBeenCalledWith({ top: 640, behavior: "smooth" }));
    expect(sentMessage).toHaveClass("widget-message--entering");
    fireEvent(sentMessage as HTMLElement, new Event("animationend", { bubbles: true }));
    await waitFor(() => expect(screen.getByText("Hello").closest("article"))
      .not.toHaveClass("widget-message--entering"));
  });

  it("animates typing and in-flight send feedback", async () => {
    let resolveSend!: (message: api.Message) => void;
    vi.mocked(api.sendWidgetMessage).mockReturnValueOnce(new Promise((resolve) => {
      resolveSend = resolve;
    }));
    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    const messageField = screen.getByLabelText("Message");
    fireEvent.input(messageField, { target: { value: "Animated send" } });

    expect(messageField.closest("form")).toHaveClass("widget-composer--typing");
    expect(screen.getByRole("button", { name: "Send" })).toHaveClass("widget-send-button--ready");
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(await screen.findByRole("button", { name: "Sending…" })).toHaveClass(
      "widget-send-button--sending",
    );
    expect(document.querySelectorAll(".widget-sending-dots b")).toHaveLength(3);
    act(() => resolveSend({
      id: "00000000-0000-4000-8000-000000000020",
      conversation_id: "00000000-0000-0000-0000-000000000003",
      sequence: 2,
      direction: "inbound",
      kind: "text",
      author_kind: "contact",
      body: "Animated send",
      status: "queued",
      created_at: "2026-08-09T07:00:00Z",
      attachments: [],
    }));

    expect(await screen.findByText("Animated send")).toBeInTheDocument();
  });

  it("shows real operator availability and refreshes it with the widget heartbeat", async () => {
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      operators_online: false,
      offline_now: "Send us a note",
    });
    vi.mocked(api.updateWidgetPresence).mockResolvedValueOnce({ operators_online: false });

    render(<App />);

    expect(await screen.findByText("Send us a note")).toBeInTheDocument();
    expect(await screen.findByText(
      "The team is away. Leave your message and contact details.",
    )).toBeInTheDocument();
    expect(screen.queryByText("Welcome. How can we help?")).not.toBeInTheDocument();
    expect(screen.getByText("Offline")).toHaveClass("widget-availability--offline");
    expect(document.querySelector(".availability-dot--offline")).not.toBeNull();

    fireEvent(document, new Event("visibilitychange"));

    expect(await screen.findByText("Online now")).toBeInTheDocument();
    expect(screen.getByText("Online")).not.toHaveClass("widget-availability--offline");
    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
  });

  it("publishes the current visitor draft for operators", async () => {
    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    fireEvent.input(screen.getByLabelText("Message"), {
      target: { value: "I am still typing" },
    });

    await waitFor(() => expect(api.updateWidgetDraft).toHaveBeenCalledWith(
      "widget-token",
      "00000000-0000-0000-0000-000000000003",
      "I am still typing",
    ));
    expect(screen.queryByText(/operator.*typing/i)).not.toBeInTheDocument();
  });

  it("sends with Enter and keeps Shift+Enter available for a new line", async () => {
    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    const messageField = screen.getByLabelText("Message");

    fireEvent.input(messageField, { target: { value: "Sent with Enter" } });
    fireEvent.keyDown(messageField, { key: "Enter", code: "Enter" });

    await waitFor(() => expect(api.sendWidgetMessage).toHaveBeenCalledWith(
      "widget-token",
      "00000000-0000-0000-0000-000000000003",
      "Sent with Enter",
    ));

    vi.mocked(api.sendWidgetMessage).mockClear();
    fireEvent.input(messageField, { target: { value: "First line" } });
    fireEvent.keyDown(messageField, { key: "Enter", code: "Enter", shiftKey: true });

    expect(api.sendWidgetMessage).not.toHaveBeenCalled();
  });

  it("renders Russian UI when lang=ru is provided", async () => {
    window.history.replaceState({}, "", "/widget.html?widget_id=00000000-0000-0000-0000-000000000010&lang=ru-RU");
    vi.mocked(api.createWidgetSession).mockResolvedValueOnce({
      ...widgetSession,
      language: "ru",
      ...russianInterfaceTranslation,
      greeting: "Здравствуйте! Чем мы можем помочь?",
      launcher: { ...widgetSession.launcher, label: "Напишите нам" },
    });
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      language: "ru",
      ...russianInterfaceTranslation,
      greeting: "Здравствуйте! Чем мы можем помочь?",
      launcher: { ...launcherConfig, label: "Напишите нам" },
    });

    render(<App />);

    expect(await screen.findByText("Здравствуйте! Чем мы можем помочь?")).toBeInTheDocument();
    expect(screen.getAllByText("Support")).toHaveLength(2);
    expect(screen.getByLabelText("Сообщение")).toHaveAttribute("placeholder", "Напишите сообщение…");
    expect(api.createWidgetSession).toHaveBeenCalledWith(
      "00000000-0000-0000-0000-000000000010",
      expect.any(String),
      "ru-ru",
      expect.objectContaining({ schema_version: 1 }),
    );
  });

  it("shows the localized contact instructions instead of the greeting while offline", async () => {
    window.history.replaceState({}, "", "/widget.html?widget_id=00000000-0000-0000-0000-000000000010&lang=ru");
    vi.mocked(api.createWidgetSession).mockResolvedValueOnce({
      ...widgetSession,
      language: "ru",
      ...russianInterfaceTranslation,
      greeting: "Здравствуйте! Чем мы можем помочь?",
      offline_message: "Операторы офлайн. Оставьте сообщение, номер заявки и контакты.",
      operators_online: false,
      launcher: { ...widgetSession.launcher, label: "Напишите нам" },
    });
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      language: "ru",
      ...russianInterfaceTranslation,
      greeting: "Здравствуйте! Чем мы можем помочь?",
      offline_message: "Операторы офлайн. Оставьте сообщение, номер заявки и контакты.",
      operators_online: false,
      launcher: { ...launcherConfig, label: "Напишите нам" },
    });
    vi.mocked(api.updateWidgetPresence).mockResolvedValueOnce({ operators_online: false });

    render(<App />);

    expect(await screen.findByText(
      "Операторы офлайн. Оставьте сообщение, номер заявки и контакты.",
    )).toBeInTheDocument();
    expect(screen.queryByText("Здравствуйте! Чем мы можем помочь?")).not.toBeInTheDocument();
  });

  it("replaces low-level network failures with a useful retry state", async () => {
    vi.mocked(api.getWidgetPresentation).mockRejectedValueOnce(new TypeError("Load failed"));

    render(<App />);

    expect(await screen.findByText("Could not open the widget.")).toBeInTheDocument();
    expect(screen.queryByText("Load failed")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
  });

  it("refreshes the saved theme when the page regains focus", async () => {
    render(<App />);
    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();

    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      theme: {
        ...widgetPresentation.theme,
        accent_color: "#444444",
        border_radius: 28,
      },
    });
    fireEvent.focus(window);

    await waitFor(() => expect(document.querySelector(".widget-stage")).toHaveStyle({
      "--widget-accent": "#444444",
      "--widget-radius": "28px",
    }));
  });

  it("renders custom footer text from widget settings", async () => {
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      theme: { ...widgetPresentation.theme, footer_text: "Secure support channel" },
    });

    render(<App />);

    expect(await screen.findByText("Secure support channel")).toBeInTheDocument();
    expect(screen.queryByText("Private conversation · Powered by Tzomet")).not.toBeInTheDocument();
  });

  it("hides the footer when an empty footer is saved", async () => {
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      theme: { ...widgetPresentation.theme, footer_text: "" },
    });

    render(<App />);

    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    expect(document.querySelector(".widget-footer")).toBeNull();
  });

  it("uses the configured support name and can open an empty chat without a greeting", async () => {
    const quietLauncher = {
      ...launcherConfig,
      show_greeting: false,
    };
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      support_name: "Anna Petrova",
      launcher: quietLauncher,
    });

    render(<App />);

    expect(await screen.findByText("Anna Petrova")).toBeInTheDocument();
    expect(await screen.findByLabelText("Message")).toBeInTheDocument();
    expect(screen.queryByText("Welcome. How can we help?")).not.toBeInTheDocument();
    expect(document.querySelector(".widget-welcome")).toBeNull();
  });

  it("shows a localized automatic invitation after its delay despite old stored state", async () => {
    const automaticLauncher = {
      ...launcherConfig,
      proactive_invitation_enabled: true,
      proactive_invitation_delay_seconds: 1,
    };
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      support_name: "Anna Petrova",
      proactive_invitation_message: "Need help choosing?",
      launcher: automaticLauncher,
    });
    vi.mocked(api.listWidgetMessages).mockResolvedValueOnce([]);
    const parentPostMessage = vi.fn();
    const parentWindow = { postMessage: parentPostMessage } as unknown as Window;
    Object.defineProperty(window, "parent", {
      configurable: true,
      value: parentWindow,
    });
    localStorage.setItem(
      "tz:widget-conversation-opened:00000000-0000-0000-0000-000000000003",
      "yes",
    );
    localStorage.setItem(
      "tz:widget-proactive-invitation-dismissed:00000000-0000-0000-0000-000000000003",
      "yes",
    );

    render(<App />);
    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: {
          ...widgetSession,
          proactive_invitation_message: "Need help choosing?",
          launcher: automaticLauncher,
        },
      },
    }));

    await screen.findByRole("button", { name: "Talk to support" });
    expect(screen.queryByText("Need help choosing?")).not.toBeInTheDocument();
    expect(await screen.findByText(
      "Need help choosing?",
      {},
      { timeout: 2_500 },
    )).toBeInTheDocument();
    expect(screen.getByText("Anna Petrova")).toBeInTheDocument();
    await waitFor(() => expect(parentPostMessage).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "tz:widget-resize",
        open: false,
        call_to_action_visible: true,
      }),
      "https://host.example",
    ));
    expect(api.sendWidgetMessage).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Dismiss support invitation" }));

    expect(screen.queryByText("Need help choosing?")).not.toBeInTheDocument();
  });

  it("does not show the automatic invitation while every operator is offline", async () => {
    const automaticLauncher = {
      ...launcherConfig,
      proactive_invitation_enabled: true,
      proactive_invitation_delay_seconds: 1,
    };
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      operators_online: false,
      proactive_invitation_message: "Need help choosing?",
      launcher: automaticLauncher,
    });
    vi.mocked(api.updateWidgetPresence).mockResolvedValueOnce({ operators_online: false });
    const parentPostMessage = vi.fn();
    const parentWindow = { postMessage: parentPostMessage } as unknown as Window;
    Object.defineProperty(window, "parent", {
      configurable: true,
      value: parentWindow,
    });

    render(<App />);
    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: {
          ...widgetSession,
          operators_online: false,
          proactive_invitation_message: "Need help choosing?",
          launcher: automaticLauncher,
        },
      },
    }));

    await screen.findByRole("button", { name: "Talk to support" });
    await act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 1_200));
    });

    expect(screen.queryByText("Need help choosing?")).not.toBeInTheDocument();
    expect(parentPostMessage).not.toHaveBeenCalledWith(
      expect.objectContaining({ call_to_action_visible: true }),
      "https://host.example",
    );
  });

  it("does not show the automatic invitation for a conversation with messages", async () => {
    const automaticLauncher = {
      ...launcherConfig,
      proactive_invitation_enabled: true,
      proactive_invitation_delay_seconds: 1,
    };
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      proactive_invitation_message: "Need help choosing?",
      launcher: automaticLauncher,
    });
    vi.mocked(api.listWidgetMessages).mockResolvedValueOnce([{
      id: "00000000-0000-0000-0000-000000000005",
      conversation_id: "00000000-0000-0000-0000-000000000003",
      sequence: 1,
      direction: "inbound",
      kind: "text",
      author_kind: "contact",
      body: "Hello",
      status: "queued",
      created_at: "2026-08-01T00:00:00Z",
      attachments: [],
    }]);
    const parentPostMessage = vi.fn();
    const parentWindow = { postMessage: parentPostMessage } as unknown as Window;
    Object.defineProperty(window, "parent", {
      configurable: true,
      value: parentWindow,
    });

    render(<App />);
    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: {
          ...widgetSession,
          proactive_invitation_message: "Need help choosing?",
          launcher: automaticLauncher,
        },
      },
    }));

    await screen.findByRole("button", { name: "Talk to support" });
    await act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 1_200));
    });

    expect(screen.queryByText("Need help choosing?")).not.toBeInTheDocument();
    expect(parentPostMessage).not.toHaveBeenCalledWith(
      expect.objectContaining({ call_to_action_visible: true }),
      "https://host.example",
    );
  });

  it("uses lift instead of pulse only while the greeting is hidden", async () => {
    const hiddenLauncher = {
      ...launcherConfig,
      show_greeting: false,
      attention_animation: "pulse" as const,
    };
    vi.mocked(api.getWidgetPresentation).mockResolvedValueOnce({
      ...widgetPresentation,
      launcher: hiddenLauncher,
    });
    const parentWindow = { postMessage: vi.fn() } as unknown as Window;
    Object.defineProperty(window, "parent", {
      configurable: true,
      value: parentWindow,
    });

    render(<App />);
    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: {
          ...widgetSession,
          launcher: hiddenLauncher,
        },
      },
    }));

    const launcherButton = await screen.findByRole("button", { name: "Talk to support" });
    await waitFor(() => expect(launcherButton).toHaveClass("widget-launcher--attention-lift"));
    expect(launcherButton).not.toHaveClass("widget-launcher--attention-pulse");
  });

  it("waits for a verified parent bootstrap when embedded", async () => {
    const animationFrames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      animationFrames.push(callback);
      return animationFrames.length;
    });
    const parentPostMessage = vi.fn();
    const parentWindow = { postMessage: parentPostMessage } as unknown as Window;
    Object.defineProperty(window, "parent", {
      configurable: true,
      value: parentWindow,
    });

    render(<App />);
    expect(parentPostMessage).toHaveBeenCalledWith(
      { type: "tz:widget-ready", version: 1 },
      "*",
    );
    expect(document.querySelector(".widget-launcher")).toBeNull();

    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://attacker.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: widgetSession,
      },
    }));
    expect(api.getWidgetPresentation).not.toHaveBeenCalled();
    expect(document.querySelector(".widget-launcher")).toBeNull();

    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: {
        type: "tz:widget-bootstrap",
        version: 1,
        embed_origin: "https://host.example",
        session: widgetSession,
      },
    }));

    const launcherButton = await screen.findByRole("button", {
      name: "Talk to support",
    });
    expect(launcherButton).toHaveClass("widget-launcher--icon_text");
    await waitFor(() => expect(launcherButton).toHaveClass("widget-launcher--attention-pulse"));
    expect(launcherButton).toHaveTextContent("Talk to support");
    expect(launcherButton.querySelector("svg")).not.toBeNull();
    await waitFor(() => expect(parentPostMessage).toHaveBeenCalledWith(
      {
        type: "tz:widget-resize",
        version: 1,
        open: false,
        call_to_action_visible: false,
        launcher: launcherConfig,
      },
      "https://host.example",
    ));
    fireEvent.click(launcherButton);
    expect(await screen.findByText("Welcome. How can we help?")).toBeInTheDocument();
    const messageList = document.querySelector(".widget-messages") as HTMLDivElement;
    const scrollTo = vi.fn();
    Object.defineProperty(messageList, "scrollHeight", {
      configurable: true,
      value: 720,
    });
    Object.defineProperty(messageList, "scrollTo", {
      configurable: true,
      value: scrollTo,
    });
    await waitFor(() => expect(animationFrames.length).toBeGreaterThan(0));
    animationFrames.splice(0).forEach((callback) => callback(0));
    expect(scrollTo).toHaveBeenCalledWith({ top: 720, behavior: "auto" });
    expect(api.createWidgetSession).not.toHaveBeenCalled();
    expect(parentPostMessage).toHaveBeenCalledWith(
      {
        type: "tz:widget-resize",
        version: 1,
        open: true,
        call_to_action_visible: false,
        launcher: launcherConfig,
      },
      "https://host.example",
    );

    fireEvent(window, new MessageEvent("message", {
      source: parentWindow,
      origin: "https://host.example",
      data: { type: "tz:widget-close", version: 1 },
    }));
    expect(document.querySelector(".widget-shell")).toHaveClass("widget-shell--closing");
  });
});
