import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import * as api from "./api";
import * as departmentApi from "./department-api";
import * as taskOrchestrationApi from "./task-orchestration-api";
import * as operatorNotifications from "./admin/operator-notifications";
import type { BrowserNotificationPermission } from "./admin/operator-notifications";
import type { ChatMessageEditorProps } from "./admin/ChatMessageEditor";


vi.mock("./task-orchestration-api", () => ({
  listTasks: vi.fn().mockResolvedValue([]),
}));

vi.mock("./department-api", () => ({
  listDepartments: vi.fn().mockResolvedValue({ items: [], default_department_id: null, director_enabled: true, can_access_director: false, inbox_options: [] }),
  selectDepartment: vi.fn(),
  createDepartment: vi.fn(),
  updateDepartment: vi.fn(),
  getDirectorOverview: vi.fn(),
}));

vi.mock("./admin/ChatMessageEditor", () => ({
  default: ({ value, onChange, label, disabled }: ChatMessageEditorProps) => (
    <textarea aria-label={label} value={value} disabled={disabled} onChange={(event) => onChange(event.target.value)} />
  ),
}));
import {
} from "./entry-mode";

const defaultProjectId = "00000000-0000-0000-0000-000000000003";
const scopedCabinetPath = (path: string, projectId = defaultProjectId) =>
  `/cabinet/p/${projectId}${path.slice("/cabinet".length)}`;

const { widgetChannel } = vi.hoisted(() => ({
  widgetChannel: {
    id: "00000000-0000-4000-8000-000000000010",
    name: "Website widget",
    kind: "widget",
  },
}));

vi.mock("./admin/operator-notifications", () => ({
  DEFAULT_OPERATOR_NOTIFICATION_SOUND: "glass_chime",
  currentBrowserNotificationPermission: vi.fn(() => "unsupported"),
  enableOperatorNotificationSound: vi.fn().mockResolvedValue(undefined),
  enableOperatorNotifications: vi.fn().mockResolvedValue("unsupported"),
  isOperatorNotificationSound: vi.fn((value: string | null) => (
    value === "glass_chime"
    || value === "pearl_chime"
    || value === "warm_keys"
    || value === "droplet_chime"
    || value === "dawn_chime"
    || value === "bell"
    || value === "double_bell"
    || value === "urgent_bell"
    || value === "soft_chime"
    || value === "crystal_ping"
    || value === "rising_chime"
    || value === "deep_bell"
  )),
  playOperatorInChatSound: vi.fn(),
  playOperatorNotificationSound: vi.fn(),
  showOperatorBrowserNotification: vi.fn(),
}));

vi.mock("./api", () => ({
  ApiRequestError: class ApiRequestError extends Error {
    constructor(message: string, readonly status: number) {
      super(message);
      this.name = "ApiRequestError";
    }
  },
  apiBaseUrl: "http://localhost:8080",
  managementApiRequest: vi.fn().mockResolvedValue({ can_choose: true, projects: [], departments: [], items: [] }),
  getDemoCredentials: vi.fn().mockResolvedValue({ email: "demo@tzomet.ai", password: "generated-demo-password", widget_url: "https://demo.tzomet.io" }),
  resolveAvatarUrl: vi.fn((value: string | null) => value),
  listInboxConversationPage: vi.fn(),
  getCurrentActor: vi.fn().mockResolvedValue({
    actor_id: "00000000-0000-0000-0000-000000000001",
    tenant_id: "00000000-0000-0000-0000-000000000002",
    project_id: "00000000-0000-0000-0000-000000000003",
    role: "operator",
    auth_method: "access_token",
    email: null,
    display_name: null,
    chat_display_name: null,
    avatar_url: null,
    permissions: ["conversations:read", "conversations:reply", "conversations:close", "contacts:read"],
  }),
  loginWithPassword: vi.fn().mockResolvedValue({
    actor_id: "00000000-0000-0000-0000-000000000001",
    tenant_id: "00000000-0000-0000-0000-000000000002",
    project_id: "00000000-0000-0000-0000-000000000003",
    role: "admin",
    auth_method: "session",
    email: "admin@tzomet.local",
    display_name: "Local Administrator",
    chat_display_name: "Local Administrator",
    avatar_url: null,
    permissions: ["conversations:read", "access_tokens:manage"],
  }),
  logoutSession: vi.fn(),
  selectProject: vi.fn(),
  refreshOperatorPresence: vi.fn().mockResolvedValue(undefined),
  clearOperatorPresence: vi.fn().mockResolvedValue(undefined),
  listInboxes: vi.fn().mockResolvedValue([
    {
      id: "00000000-0000-0000-0000-000000000004",
      project_id: "00000000-0000-0000-0000-000000000003",
      name: "Customer support",
      status: "active",
      created_at: "2026-08-01T00:00:00Z",
    },
  ]),
  getInboxRouting: vi.fn().mockResolvedValue({
    configuration: {
      inbox_id: "00000000-0000-0000-0000-000000000004",
      project_id: "00000000-0000-0000-0000-000000000003",
      configured: true,
      version: 3,
      enabled: true,
      assignment_strategy: "least_active",
      default_queue_id: "00000000-0000-4000-8000-000000000061",
      queues: [{
        id: "00000000-0000-4000-8000-000000000061",
        name: "Customer support",
        status: "active",
        member_ids: [],
      }],
      rules: [{
        id: "00000000-0000-4000-8000-000000000062",
        position: 1,
        enabled: true,
        channel_id: "00000000-0000-0000-0000-000000000010",
        language: "en",
        queue_id: "00000000-0000-4000-8000-000000000061",
      }],
      coverage: {
        timezone: "Europe/Istanbul",
        weekly_intervals: [{
          id: "00000000-0000-4000-8000-000000000063",
          weekday: 1,
          starts_at: "09:00",
          ends_at: "18:00",
        }],
        outside_hours_action: "ai",
        no_operator_action: "queue",
      },
      sla: {
        clock: "working_hours",
        unassigned_warning_seconds: 180,
        first_response_seconds: 300,
        resolution_seconds: 1_800,
        escalation_queue_id: null,
      },
      telegram: {
        new_visitor: false,
        new_message: false,
        operator_request: true,
        unassigned_warning: true,
        sla_breach: true,
        chat_ids: ["-1001234567890"],
        bot_token_configured: true,
      },
    },
    channel_options: [{
      id: "00000000-0000-0000-0000-000000000010",
      name: "Website widget",
      kind: "widget",
      status: "active",
    }],
    operator_options: [],
  }),
  updateInboxRouting: vi.fn(),
  PROJECTS_CHANGED_EVENT: "tzomet:projects-changed",
  listProjects: vi.fn().mockResolvedValue([
    {
      id: "00000000-0000-0000-0000-000000000003",
      name: "Customer support",
      slug: "customer-support",
      status: "active",
      role: "admin",
      current: true,
      inbox_count: 1,
      channel_count: 1,
      conversation_count: 2,
      team_count: 1,
      member_count: 1,
      created_at: "2026-08-01T00:00:00Z",
      updated_at: "2026-08-01T00:00:00Z",
    },
  ]),
  createProject: vi.fn(),
  updateProject: vi.fn(),
  listProjectMembers: vi.fn().mockResolvedValue([
    {
      membership_id: "00000000-0000-0000-0000-000000000011",
      user_id: "00000000-0000-0000-0000-000000000001",
      email: "admin@tzomet.local",
      display_name: "Local Administrator",
      chat_display_name: "Local Administrator",
      avatar_url: null,
      role: "admin",
      role_id: "admin",
      role_name: "Administrator",
      created_at: "2026-08-01T00:00:00Z",
      updated_at: "2026-08-01T00:00:00Z",
    },
  ]),
  grantProjectMember: vi.fn(),
  createProjectUser: vi.fn(),
  updateProjectMember: vi.fn(),
  revokeProjectMember: vi.fn(),
  updateOperatorChatProfile: vi.fn(),
  uploadOperatorAvatar: vi.fn(),
  deleteOperatorAvatar: vi.fn(),
  listAiProfiles: vi.fn().mockResolvedValue([]),
  createAiProfile: vi.fn(),
  updateAiProfile: vi.fn(),
  uploadAiProfileAvatar: vi.fn(),
  deleteAiProfileAvatar: vi.fn(),
  deleteAiProfile: vi.fn(),
  listAiProviders: vi.fn().mockResolvedValue([]),
  createAiProvider: vi.fn(),
  updateAiProvider: vi.fn(),
  deleteAiProvider: vi.fn(),
  listAiTasks: vi.fn().mockResolvedValue([]),
  listAiTaskRuns: vi.fn().mockResolvedValue([]),
  createAiTask: vi.fn(),
  updateAiTask: vi.fn(),
  deleteAiTask: vi.fn(),
  listKnowledgeBases: vi.fn().mockResolvedValue([]),
  createKnowledgeBase: vi.fn(),
  updateKnowledgeBase: vi.fn(),
  deleteKnowledgeBase: vi.fn(),
  listKnowledgeArticles: vi.fn().mockResolvedValue([]),
  createKnowledgeArticle: vi.fn(),
  updateKnowledgeArticle: vi.fn(),
  deleteKnowledgeArticle: vi.fn(),
  listReplyTemplates: vi.fn().mockResolvedValue([]),
  createReplyTemplate: vi.fn(),
  updateReplyTemplate: vi.fn(),
  deleteReplyTemplate: vi.fn(),
  listInboxConversations: vi.fn().mockResolvedValue([
    {
      id: "00000000-0000-0000-0000-000000000005",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-0000-0000-000000000006",
      contact: {
        display_name: "Maria Sokolova",
        email: "maria@example.com",
      },
      status: "open",
      subject: "Payment question",
      last_message_sequence: 1,
      created_at: "2026-08-01T00:00:00Z",
      updated_at: "2026-08-01T00:00:00Z",
      operator: {
        display_name: "Local Administrator",
        avatar_url: null,
        joined_at: "2026-08-01T00:00:00Z",
      },
      ai_agent: null,
      assigned_to_me: true,
      unread_customer_messages: 0,
      widget_attachments_enabled: false,
      channel: widgetChannel,
    },
  ]),
  markOperatorConversationRead: vi.fn().mockResolvedValue(undefined),
  listOperatorMessages: vi.fn().mockResolvedValue([
    {
      id: "00000000-0000-0000-0000-000000000007",
      conversation_id: "00000000-0000-0000-0000-000000000005",
      sequence: 1,
      direction: "inbound",
      kind: "text",
      author_kind: "contact",
      body: "Where is my payment?",
      status: "delivered",
      created_at: "2026-08-01T00:00:00Z",
      attachments: [],
    },
  ]),
  getVisitorIntelligence: vi.fn().mockResolvedValue({
    session_count: 2,
    first_seen_at: "2026-07-31T20:00:00Z",
    last_seen_at: "2026-08-01T00:00:00Z",
    observations: [
      {
        session_id: "00000000-0000-4000-8000-000000000009",
        widget_language: "ru",
        client_ip: "203.0.113.42",
        client_ip_source: "trusted_proxy",
        user_agent: "Example Browser/1.0",
        geo_ip: {
          country_code: "US",
          country: "United States",
          city: "New York",
          time_zone: "America/New_York",
        },
        user_agent_details: {
          browser: "Example Browser",
          browser_version: "1.0",
          operating_system: "Example OS",
          operating_system_version: "15",
          device_category: "pc",
        },
        accept_language: "en-US,en;q=0.9",
        client_hints: null,
        origin: "https://shop.example",
        captured_at: "2026-08-01T00:00:00Z",
        client_context: {
          schema_version: 1,
          captured_at: "2026-08-01T00:00:00Z",
          page: {
            url: "https://shop.example/checkout",
            title: "Checkout",
            referrer: "https://search.example",
          },
          locale: {
            language: "en-US",
            languages: ["en-US", "en"],
            timezone: "Europe/Chisinau",
          },
          display: {
            viewport_width: 1440,
            viewport_height: 900,
            screen_width: 2560,
            screen_height: 1440,
            device_pixel_ratio: 2,
            color_depth: 24,
          },
          browser: {
            platform: "macOS",
            brands: [{ brand: "Example", version: "1" }],
            mobile: false,
            cookie_enabled: true,
            global_privacy_control: true,
          },
          device: {
            memory_gb: 8,
            logical_processors: 8,
            max_touch_points: 0,
          },
          preferences: {
            color_scheme: "dark",
            reduced_motion: false,
          },
          connection: {
            effective_type: "4g",
            downlink_mbps: 10,
            rtt_ms: 50,
            save_data: false,
          },
          attribution: { utm_source: "search" },
        },
      },
    ],
  }),
  createOperatorRealtimeTicket: vi.fn().mockResolvedValue("ticket"),
  openRealtimeSocket: vi.fn().mockReturnValue({ close: vi.fn() }),
  sendOperatorMessage: vi.fn(),
  uploadOperatorAttachment: vi.fn(),
  downloadOperatorAttachment: vi.fn(),
  updateConversationAttachmentPolicy: vi.fn().mockResolvedValue(true),
  joinConversation: vi.fn(),
  takeOverConversation: vi.fn(),
  returnConversationToAi: vi.fn(),
  resolveConversation: vi.fn(),
  listContacts: vi.fn().mockResolvedValue({
    statistics: { contacts: 0, new_contacts: 0, returning_contacts: 0, conversations: 0, widget_sessions: 0, widget_contacts: 0, telegram_contacts: 0, countries: [] },
    items: [],
    total: 0,
    page: 1,
    per_page: 10,
    total_pages: 0,
  }),
  listOnlineVisitorWidgets: vi.fn().mockResolvedValue([{
    id: "00000000-0000-4000-8000-000000000022",
    inbox_id: "00000000-0000-0000-0000-000000000004",
    name: "Checkout widget",
  }]),
  listOnlineVisitors: vi.fn().mockResolvedValue({ items: [], online_window_seconds: 90 }),
  listAiChannelOptions: vi.fn().mockResolvedValue([]),
  listChannels: vi.fn().mockResolvedValue([]),
  createWidgetChannel: vi.fn(),
  deleteChannel: vi.fn(),
  updateWidgetChannel: vi.fn(),
  getEmailSettings: vi.fn(),
  updateEmailSettings: vi.fn(),
  getPublicSupportRating: vi.fn(),
  submitPublicSupportRating: vi.fn(),
  listRoles: vi.fn().mockResolvedValue([]),
  listAccessTokens: vi.fn().mockResolvedValue([]),
  listAccessTokenManagement: vi.fn().mockResolvedValue({ items: [], can_create_all_projects: false }),
  createAccessToken: vi.fn(),
  revokeAccessToken: vi.fn(),
  listApiIntegrations: vi.fn().mockResolvedValue({ items: [], ai_profiles: [] }),
  createApiIntegration: vi.fn(),
  updateApiIntegration: vi.fn(),
  deleteApiIntegration: vi.fn(),
}));

describe("App", () => {
  async function openConversation(name: RegExp = /Payment question/) {
    const row = await screen.findByRole("button", { name });
    fireEvent.click(row);
    return row;
  }

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  beforeEach(() => {
    document.documentElement.lang = "en";
    vi.clearAllMocks();
    vi.mocked(departmentApi.listDepartments).mockResolvedValue({ items: [], default_department_id: null, director_enabled: true, can_access_director: false, inbox_options: [] });
    vi.mocked(departmentApi.selectDepartment).mockReset();
    vi.mocked(departmentApi.getDirectorOverview).mockReset();
    vi.mocked(api.listInboxConversationPage).mockImplementation(async (...args) => ({
      items: await api.listInboxConversations(...args), has_more: false,
    }));
    window.history.replaceState(null, "", "/cabinet");
    sessionStorage.clear();
    localStorage.clear();
    Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });
    sessionStorage.setItem("tz-operator-token", "operator-token");
    document.documentElement.removeAttribute("data-theme");
    document.documentElement.removeAttribute("data-palette");
    document.documentElement.removeAttribute("data-product-edition");
    document.documentElement.removeAttribute("style");
    document.documentElement.lang = "en";
    document.title = "";
    let themeColor = document.querySelector<HTMLMetaElement>('meta[name="theme-color"]');
    if (!themeColor) {
      themeColor = document.createElement("meta");
      themeColor.name = "theme-color";
      document.head.append(themeColor);
    }
    themeColor.content = "#f4f5f7";
    vi.stubGlobal("matchMedia", vi.fn().mockReturnValue({
      matches: false,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
  });

  it("replaces an unknown admin path with the first permitted page", async () => {
    window.history.replaceState(null, "", "/cabinet/not-a-page");

    render(<App />);

    expect(await screen.findByRole("heading", { name: "Conversations" })).toBeInTheDocument();
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
  });

  it("keeps the conversation workspace closed until a conversation is selected", async () => {
    render(<App />);

    expect((await screen.findAllByText("Payment question")).length).toBeGreaterThan(0);
    expect(screen.getByText("Select a conversation.")).toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "Contact" })).not.toBeInTheDocument();
    expect(api.listOperatorMessages).not.toHaveBeenCalled();
    expect(api.getVisitorIntelligence).not.toHaveBeenCalled();

    const conversationRow = await screen.findByRole("button", { name: /Payment question/ });
    expect(conversationRow).not.toHaveAttribute("aria-current");
    fireEvent.click(conversationRow);
    expect(conversationRow).toHaveAttribute("aria-current", "true");
    expect(await screen.findByText("Where is my payment?")).toBeInTheDocument();
    expect(screen.getAllByText("Maria Sokolova").length).toBeGreaterThan(0);
    expect(screen.getAllByText("maria@example.com").length).toBeGreaterThan(0);
    expect((await screen.findAllByText("203.0.113.42")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("New York, United States (US)").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Example Browser 1.0").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Example OS 15").length).toBeGreaterThan(0);
    expect(screen.getAllByText("https://shop.example/checkout").length).toBeGreaterThan(0);
    expect(screen.getAllByText("User language").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Widget language").length).toBeGreaterThan(0);
    expect(screen.getAllByText("en-US, en").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Customer support").length).toBeGreaterThan(0);
    expect(screen.queryAllByRole("img", { name: "Tzomet" })).toHaveLength(0);

    fireEvent.click(screen.getByRole("button", { name: "Close conversation view" }));

    expect(screen.getByText("Select a conversation.")).toBeInTheDocument();
    expect(screen.queryByRole("complementary", { name: "Contact" })).not.toBeInTheDocument();
    expect(conversationRow).not.toHaveAttribute("aria-current");
    await waitFor(() => expect(conversationRow).toHaveFocus());
    expect(api.resolveConversation).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Refresh conversations" }));
    await waitFor(() => expect(api.listInboxConversations).toHaveBeenCalledTimes(2));
    expect(screen.getByText("Select a conversation.")).toBeInTheDocument();
  });

  it("restores messages missed while the realtime socket was disconnected", async () => {
    const conversationId = "00000000-0000-4000-8000-000000000080";
    const conversation: api.OperatorConversation = {
      id: conversationId,
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-4000-8000-000000000081",
      contact: { display_name: "Reconnect customer", email: null, is_blocked: false },
      status: "open",
      subject: "Reconnect regression",
      last_message_sequence: 11,
      created_at: "2026-08-31T12:00:00Z",
      updated_at: "2026-08-31T12:00:00Z",
      operator: {
        display_name: "Local Administrator",
        avatar_url: null,
        joined_at: "2026-08-31T12:00:00Z",
      },
      ai_agent: null,
      assigned_to_me: true,
      unread_customer_messages: 0,
      widget_attachments_enabled: false,
      channel: widgetChannel,
    };
    const messages = Array.from({ length: 15 }, (_, index): api.Message => ({
      id: `00000000-0000-4000-8000-${String(index + 1).padStart(12, "0")}`,
      conversation_id: conversationId,
      sequence: index + 1,
      direction: index < 11 ? "outbound" : "inbound",
      kind: "text",
      author_kind: index < 11 ? "operator" : "contact",
      body: `Reconnect message ${index + 1}`,
      status: index < 11 ? "sent" : "delivered",
      created_at: `2026-08-31T12:${String(index).padStart(2, "0")}:00Z`,
      attachments: [],
    }));
    vi.mocked(api.listInboxConversations)
      .mockResolvedValueOnce([conversation])
      .mockResolvedValueOnce([{ ...conversation, last_message_sequence: 15 }]);
    vi.mocked(api.listOperatorMessages)
      .mockResolvedValueOnce(messages.slice(0, 11))
      .mockResolvedValueOnce(messages);
    let onOpen: (() => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation(
      (_ticket, _onEvent, _onClose, handleOpen) => {
        onOpen = handleOpen;
        return {
          readyState: WebSocket.OPEN,
          close: vi.fn(),
        } as unknown as WebSocket;
      },
    );

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /Reconnect regression/ }));
    expect(await screen.findByText("Reconnect message 11")).toBeInTheDocument();
    expect(document.querySelectorAll(".message-scroll .message")).toHaveLength(11);
    await waitFor(() => expect(onOpen).toBeDefined());

    act(() => onOpen?.());

    expect(await screen.findByText("Reconnect message 15")).toBeInTheDocument();
    await waitFor(() => {
      expect(document.querySelectorAll(".message-scroll .message")).toHaveLength(15);
    });
  });

  it.each(["sent", "read"] as const)("refreshes an outbound message when realtime changes its status to %s", async (status) => {
    const label = status === "read" ? "Read" : "Sent";
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    const queuedMessage: api.Message = {
      id: "00000000-0000-4000-8000-000000000082",
      conversation_id: "00000000-0000-0000-0000-000000000005",
      sequence: 2,
      direction: "outbound",
      kind: "text",
      author_kind: "operator",
      body: "Status acknowledgement regression",
      status: "queued",
      created_at: "2026-08-31T14:31:00Z",
      attachments: [],
    };
    vi.mocked(api.listOperatorMessages)
      .mockResolvedValueOnce([queuedMessage])
      .mockResolvedValueOnce([{ ...queuedMessage, status }]);
    vi.mocked(api.openRealtimeSocket).mockImplementation(
      (_ticket, onEvent) => {
        onRealtime = onEvent;
        return {
          readyState: WebSocket.OPEN,
          close: vi.fn(),
        } as unknown as WebSocket;
      },
    );

    render(<App />);
    await openConversation();
    const body = await screen.findByText(queuedMessage.body);
    const bubble = body.closest("article");
    if (!bubble) throw new Error("Expected outbound message bubble");
    expect(within(bubble).getByText("Queued")).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000083",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      type: "message.status_updated",
      aggregate_id: queuedMessage.id,
      sequence: queuedMessage.sequence,
      data: {
        conversation_id: queuedMessage.conversation_id,
        direction: "outbound",
        status,
      },
    }));

    await waitFor(() => {
      expect(within(bubble).getByText(label)).toBeInTheDocument();
    });
    expect(within(bubble).getByText(label)).toHaveClass(`message-status--${status}`);
    expect(within(bubble).queryByText("Queued")).not.toBeInTheDocument();
    expect(api.listOperatorMessages).toHaveBeenCalledTimes(2);
    expect(api.listOperatorMessages).toHaveBeenLastCalledWith(
      expect.anything(),
      queuedMessage.conversation_id,
    );
  });

  it("does not let an older message request overwrite realtime catch-up", async () => {
    let resolveOlder!: (messages: api.Message[]) => void;
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    const currentMessage: api.Message = {
      id: "00000000-0000-4000-8000-000000000090",
      conversation_id: "00000000-0000-0000-0000-000000000005",
      sequence: 2,
      direction: "inbound",
      kind: "text",
      author_kind: "contact",
      body: "Current realtime history",
      status: "delivered",
      created_at: "2026-08-31T12:36:00Z",
      attachments: [],
    };
    const olderMessage: api.Message = {
      ...currentMessage,
      id: "00000000-0000-4000-8000-000000000091",
      sequence: 1,
      body: "Stale history response",
      created_at: "2026-08-31T12:00:00Z",
    };
    vi.mocked(api.listOperatorMessages)
      .mockImplementationOnce(() => new Promise((resolve) => { resolveOlder = resolve; }))
      .mockResolvedValueOnce([currentMessage]);
    vi.mocked(api.openRealtimeSocket).mockImplementation(
      (_ticket, onEvent) => {
        onRealtime = onEvent;
        return {
          readyState: WebSocket.OPEN,
          close: vi.fn(),
        } as unknown as WebSocket;
      },
    );

    render(<App />);
    await openConversation();
    await waitFor(() => expect(resolveOlder).toBeTypeOf("function"));
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000092",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      type: "message.created",
      aggregate_id: currentMessage.id,
      data: {
        conversation_id: currentMessage.conversation_id,
        direction: "inbound",
      },
    }));
    expect(await screen.findByText("Current realtime history")).toBeInTheDocument();

    await act(async () => resolveOlder([olderMessage]));
    expect(screen.getByText("Current realtime history")).toBeInTheDocument();
    expect(screen.queryByText("Stale history response")).not.toBeInTheDocument();
  });

  it("keeps an acknowledged reply when an older history request finishes later", async () => {
    let resolveHistory!: (messages: api.Message[]) => void;
    const sentMessage: api.Message = {
      id: "00000000-0000-4000-8000-000000000093",
      conversation_id: "00000000-0000-0000-0000-000000000005",
      sequence: 2,
      direction: "outbound",
      kind: "text",
      author_kind: "operator",
      body: "Acknowledged operator reply",
      status: "sent",
      created_at: "2026-08-31T13:05:00Z",
      attachments: [],
    };
    const staleMessage: api.Message = {
      ...sentMessage,
      id: "00000000-0000-4000-8000-000000000094",
      sequence: 1,
      direction: "inbound",
      author_kind: "contact",
      body: "Old history before reply",
      status: "delivered",
      created_at: "2026-08-31T13:04:00Z",
    };
    vi.mocked(api.listOperatorMessages)
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveHistory = resolve; }),
      )
      .mockResolvedValueOnce([sentMessage]);
    vi.mocked(api.sendOperatorMessage).mockResolvedValueOnce(sentMessage);

    render(<App />);
    await openConversation();
    await waitFor(() => expect(resolveHistory).toBeTypeOf("function"));
    const reply = screen.getByLabelText("Reply");
    fireEvent.change(reply, { target: { value: sentMessage.body } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(await screen.findByText(sentMessage.body)).toBeInTheDocument();
    await act(async () => resolveHistory([staleMessage]));
    expect(screen.getByText(sentMessage.body)).toBeInTheDocument();
    expect(screen.queryByText(staleMessage.body)).not.toBeInTheDocument();
  });

  it("keeps a realtime catch-up that starts while a reply is pending", async () => {
    let resolveSend!: (message: api.Message) => void;
    let resolveCatchUp!: (messages: api.Message[]) => void;
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    const conversationId = "00000000-0000-0000-0000-000000000005";
    const initialMessage: api.Message = {
      id: "00000000-0000-4000-8000-000000000095",
      conversation_id: conversationId,
      sequence: 1,
      direction: "inbound",
      kind: "text",
      author_kind: "contact",
      body: "History before the pending reply",
      status: "delivered",
      created_at: "2026-08-31T13:00:00Z",
      attachments: [],
    };
    const sentMessage: api.Message = {
      ...initialMessage,
      id: "00000000-0000-4000-8000-000000000096",
      sequence: 2,
      direction: "outbound",
      author_kind: "operator",
      body: "Reply that was still pending",
      status: "sent",
      created_at: "2026-08-31T13:01:00Z",
    };
    const visitorMessage: api.Message = {
      ...initialMessage,
      id: "00000000-0000-4000-8000-000000000097",
      sequence: 3,
      body: "Visitor message from realtime catch-up",
      created_at: "2026-08-31T13:02:00Z",
    };
    vi.mocked(api.listOperatorMessages).mockResolvedValueOnce([initialMessage]);
    vi.mocked(api.sendOperatorMessage).mockImplementationOnce(
      () => new Promise((resolve) => { resolveSend = resolve; }),
    );
    vi.mocked(api.openRealtimeSocket).mockImplementation(
      (_ticket, onEvent) => {
        onRealtime = onEvent;
        return {
          readyState: WebSocket.OPEN,
          close: vi.fn(),
        } as unknown as WebSocket;
      },
    );

    render(<App />);
    await openConversation();
    expect(await screen.findByText(initialMessage.body)).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());
    vi.mocked(api.listOperatorMessages)
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveCatchUp = resolve; }),
      )
      .mockResolvedValueOnce([initialMessage, sentMessage, visitorMessage]);
    fireEvent.change(screen.getByLabelText("Reply"), {
      target: { value: sentMessage.body },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000098",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      type: "message.created",
      aggregate_id: visitorMessage.id,
      data: { conversation_id: conversationId, direction: "inbound" },
    }));
    await waitFor(() => expect(resolveCatchUp).toBeTypeOf("function"));

    await act(async () => resolveSend(sentMessage));
    expect(await screen.findByText(sentMessage.body)).toBeInTheDocument();
    await act(async () => resolveCatchUp([initialMessage, sentMessage, visitorMessage]));
    expect(await screen.findByText(visitorMessage.body)).toBeInTheDocument();
  });

  it("retries an invalidated catch-up when sending the reply fails", async () => {
    let resolveSupersededCatchUp!: (messages: api.Message[]) => void;
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    const conversationId = "00000000-0000-0000-0000-000000000005";
    const initialMessage: api.Message = {
      id: "00000000-0000-4000-8000-000000000104",
      conversation_id: conversationId,
      sequence: 1,
      direction: "inbound",
      kind: "text",
      author_kind: "contact",
      body: "History before failed reply",
      status: "delivered",
      created_at: "2026-08-31T13:00:00Z",
      attachments: [],
    };
    const missedMessage: api.Message = {
      ...initialMessage,
      id: "00000000-0000-4000-8000-000000000105",
      sequence: 2,
      body: "Missed message restored after send failure",
      created_at: "2026-08-31T13:01:00Z",
    };
    vi.mocked(api.listOperatorMessages).mockResolvedValueOnce([initialMessage]);
    vi.mocked(api.sendOperatorMessage).mockRejectedValueOnce(new Error("send failed"));
    vi.mocked(api.openRealtimeSocket).mockImplementation(
      (_ticket, onEvent) => {
        onRealtime = onEvent;
        return {
          readyState: WebSocket.OPEN,
          close: vi.fn(),
        } as unknown as WebSocket;
      },
    );

    render(<App />);
    await openConversation();
    expect(await screen.findByText(initialMessage.body)).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());
    vi.mocked(api.listOperatorMessages)
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveSupersededCatchUp = resolve; }),
      )
      .mockResolvedValueOnce([initialMessage, missedMessage]);
    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000106",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      type: "message.created",
      aggregate_id: missedMessage.id,
      data: { conversation_id: conversationId, direction: "inbound" },
    }));
    await waitFor(() => expect(resolveSupersededCatchUp).toBeTypeOf("function"));

    fireEvent.change(screen.getByLabelText("Reply"), {
      target: { value: "Reply that fails" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(await screen.findByText(missedMessage.body)).toBeInTheDocument();
    await act(async () => resolveSupersededCatchUp([initialMessage, missedMessage]));
    expect(screen.getByText(missedMessage.body)).toBeInTheDocument();
  });

  it("does not mix a late reply response into a newly selected conversation", async () => {
    let resolveSend!: (message: api.Message) => void;
    let resolveSecondHistory!: (messages: api.Message[]) => void;
    const firstConversationId = "00000000-0000-4000-8000-000000000099";
    const secondConversationId = "00000000-0000-4000-8000-000000000100";
    const conversation = (
      id: string,
      subject: string,
    ): api.OperatorConversation => ({
      id,
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: crypto.randomUUID(),
      contact: { display_name: subject, email: null, is_blocked: false },
      status: "open",
      subject,
      last_message_sequence: 1,
      created_at: "2026-08-31T13:00:00Z",
      updated_at: "2026-08-31T13:00:00Z",
      operator: {
        display_name: "Local Administrator",
        avatar_url: null,
        joined_at: "2026-08-31T13:00:00Z",
      },
      ai_agent: null,
      assigned_to_me: true,
      unread_customer_messages: 0,
      widget_attachments_enabled: false,
      channel: widgetChannel,
    });
    const firstConversation = conversation(firstConversationId, "First pending chat");
    const secondConversation = conversation(secondConversationId, "Second selected chat");
    const message = (
      id: string,
      conversationId: string,
      body: string,
    ): api.Message => ({
      id,
      conversation_id: conversationId,
      sequence: 1,
      direction: "inbound",
      kind: "text",
      author_kind: "contact",
      body,
      status: "delivered",
      created_at: "2026-08-31T13:00:00Z",
      attachments: [],
    });
    const firstMessage = message(
      "00000000-0000-4000-8000-000000000101",
      firstConversationId,
      "First conversation history",
    );
    const secondMessage = message(
      "00000000-0000-4000-8000-000000000102",
      secondConversationId,
      "Second conversation history",
    );
    const sentMessage: api.Message = {
      ...firstMessage,
      id: "00000000-0000-4000-8000-000000000103",
      sequence: 2,
      direction: "outbound",
      author_kind: "operator",
      body: "Late reply for the first conversation",
      status: "sent",
      created_at: "2026-08-31T13:01:00Z",
    };
    vi.mocked(api.listInboxConversations)
      .mockResolvedValueOnce([firstConversation, secondConversation])
      .mockResolvedValueOnce([firstConversation, secondConversation]);
    vi.mocked(api.listOperatorMessages)
      .mockResolvedValueOnce([firstMessage])
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveSecondHistory = resolve; }),
      );
    vi.mocked(api.sendOperatorMessage).mockImplementationOnce(
      () => new Promise((resolve) => { resolveSend = resolve; }),
    );

    render(<App />);
    await openConversation(/First pending chat/);
    expect(await screen.findByText(firstMessage.body)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Reply"), {
      target: { value: sentMessage.body },
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    const secondRow = await screen.findByRole("button", { name: /Second selected chat/ });
    fireEvent.click(secondRow);
    await waitFor(() => expect(secondRow).toHaveAttribute("aria-current", "true"));
    await waitFor(() => expect(resolveSecondHistory).toBeTypeOf("function"));
    await act(async () => resolveSend(sentMessage));
    expect(screen.queryByText(sentMessage.body, { selector: ".message > div" }))
      .not.toBeInTheDocument();

    await act(async () => resolveSecondHistory([secondMessage]));
    expect(await screen.findByText(secondMessage.body)).toBeInTheDocument();
    expect(screen.queryByText(sentMessage.body, { selector: ".message > div" }))
      .not.toBeInTheDocument();
  });

  it("searches message words and conversation IDs without losing the Inbox filters", async () => {
    render(<App />);

    await openConversation();
    const search = screen.getByRole("searchbox", { name: "Search by conversation ID or messages" });
    const channelFilter = screen.getByRole("combobox", { name: "Conversation channel" });
    expect(within(channelFilter).getByRole("option", {
      name: "Website widget · Widget",
    })).toBeInTheDocument();

    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([]);
    fireEvent.change(search, { target: { value: "  delayed refund  " } });

    await waitFor(() => expect(api.listInboxConversations).toHaveBeenLastCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000004",
      expect.objectContaining({ search: "delayed refund" }),
    ));
    const conversationList = screen.getByRole("region", { name: "Conversation list" });
    expect(await within(conversationList).findByText(
      "No conversations match your search.",
    )).toBeInTheDocument();
    expect(within(channelFilter).getByRole("option", {
      name: "Website widget · Widget",
    })).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Conversation messages" }))
      .getByText("Select a conversation.")).toBeInTheDocument();

    fireEvent.change(search, { target: { value: "" } });

    await waitFor(() => expect(api.listInboxConversations).toHaveBeenLastCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000004",
      expect.objectContaining({ search: undefined }),
    ));
    expect(await within(conversationList).findByRole("button", {
      name: /Payment question/,
    })).toBeInTheDocument();

    fireEvent.change(search, {
      target: { value: "  00000000-0000-0000-0000-000000000005  " },
    });
    await waitFor(() => expect(api.listInboxConversations).toHaveBeenLastCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000004",
      expect.objectContaining({ search: "00000000-0000-0000-0000-000000000005" }),
    ));
    expect(await within(conversationList).findByRole("button", {
      name: /Payment question/,
    })).toBeInTheDocument();
  });

  it("aborts stale searches and retains the selected channel when search clears", async () => {
    render(<App />);

    const search = await screen.findByRole("searchbox", {
      name: "Search by conversation ID or messages",
    });
    let firstSignal: AbortSignal | undefined;
    let resolveFirst!: (items: api.OperatorConversation[]) => void;
    let resolveSecond!: (items: api.OperatorConversation[]) => void;
    vi.mocked(api.listInboxConversations)
      .mockImplementationOnce((_auth, _inboxId, options) => {
        firstSignal = options?.signal;
        return new Promise((resolve) => { resolveFirst = resolve; });
      })
      .mockImplementationOnce(() => (
        new Promise((resolve) => { resolveSecond = resolve; })
      ));
    const emailChannel = {
      id: "00000000-0000-4000-8000-000000000070",
      name: "Legacy email",
      kind: "imap_smtp",
    };
    const searchResult = (
      id: string,
      subject: string,
      channel = widgetChannel,
    ): api.OperatorConversation => ({
      id,
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-4000-8000-000000000071",
      contact: { display_name: "Search customer", email: null, is_blocked: false },
      status: "open",
      subject,
      last_message_sequence: 1,
      created_at: "2026-08-29T00:00:00Z",
      updated_at: "2026-08-29T00:00:00Z",
      operator: null,
      ai_agent: null,
      assigned_to_me: false,
      unread_customer_messages: 0,
      widget_attachments_enabled: false,
      channel,
    });

    fireEvent.change(search, { target: { value: "payment" } });
    await waitFor(() => expect(firstSignal).toBeInstanceOf(AbortSignal));
    fireEvent.change(search, { target: { value: "refund" } });
    await waitFor(() => expect(resolveSecond).toBeTypeOf("function"));
    expect(firstSignal?.aborted).toBe(true);

    await act(async () => {
      resolveSecond([searchResult(
        "00000000-0000-4000-8000-000000000072",
        "Current refund result",
        emailChannel,
      )]);
    });
    expect(await screen.findByRole("button", { name: /Current refund result/ }))
      .toBeInTheDocument();

    await act(async () => {
      resolveFirst([searchResult(
        "00000000-0000-4000-8000-000000000073",
        "Stale payment result",
      )]);
    });
    expect(screen.queryByRole("button", { name: /Stale payment result/ }))
      .not.toBeInTheDocument();

    const channelFilter = screen.getByRole("combobox", { name: "Conversation channel" });
    fireEvent.change(channelFilter, { target: { value: emailChannel.id } });
    fireEvent.change(search, { target: { value: "" } });

    await waitFor(() => expect(api.listInboxConversations).toHaveBeenLastCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000004",
      expect.objectContaining({ search: undefined }),
    ));
    await waitFor(() => expect(channelFilter).toHaveValue(emailChannel.id));
    expect(within(channelFilter).getByRole("option", {
      name: "Legacy email · IMAP / SMTP",
    })).toBeInTheDocument();
    fireEvent.change(channelFilter, { target: { value: "all" } });
    expect(await screen.findByRole("button", { name: /Payment question/ }))
      .toBeInTheDocument();
  });

  it("shows channel origin throughout a conversation and filters the Inbox by channel", async () => {
    const secondInboxId = "00000000-0000-4000-8000-000000000016";
    const telegramChannel = {
      id: "00000000-0000-4000-8000-000000000011",
      name: "Support Telegram",
      kind: "telegram_bot",
    };
    vi.mocked(api.listInboxes).mockResolvedValueOnce([
      {
        id: "00000000-0000-0000-0000-000000000004",
        project_id: "00000000-0000-0000-0000-000000000003",
        name: "Customer support",
        status: "active",
        created_at: "2026-08-01T00:00:00Z",
      },
      {
        id: secondInboxId,
        project_id: "00000000-0000-0000-0000-000000000003",
        name: "Second Inbox",
        status: "active",
        created_at: "2026-08-17T00:00:00Z",
      },
    ]);
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-4000-8000-000000000012",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-4000-8000-000000000013",
        contact: { display_name: "Telegram customer", email: null, is_blocked: false },
        status: "open",
        subject: "Telegram refund",
        last_message_sequence: 2,
        created_at: "2026-08-17T18:00:00Z",
        updated_at: "2026-08-17T18:02:00Z",
        operator: {
          display_name: "Local Administrator",
          avatar_url: null,
          joined_at: "2026-08-17T18:01:00Z",
        },
        ai_agent: {
          display_name: "Telegram assistant",
          avatar_url: null,
          joined_at: "2026-08-17T18:00:00Z",
          active: false,
        },
        assigned_to_me: true,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: telegramChannel,
      },
      {
        id: "00000000-0000-4000-8000-000000000014",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-4000-8000-000000000015",
        contact: { display_name: "Widget customer", email: null, is_blocked: false },
        status: "open",
        subject: "Widget payment",
        last_message_sequence: 1,
        created_at: "2026-08-17T18:00:00Z",
        updated_at: "2026-08-17T18:01:00Z",
        operator: {
          display_name: "Local Administrator",
          avatar_url: null,
          joined_at: "2026-08-17T18:01:00Z",
        },
        ai_agent: null,
        assigned_to_me: true,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
      {
        id: "00000000-0000-4000-8000-000000000017",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-4000-8000-000000000018",
        contact: { display_name: "Resolved customer", email: null, is_blocked: false },
        status: "resolved",
        subject: "Resolved widget",
        last_message_sequence: 3,
        created_at: "2026-08-16T18:00:00Z",
        updated_at: "2026-08-16T18:02:00Z",
        operator: null,
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]).mockResolvedValueOnce([
      {
        id: "00000000-0000-4000-8000-000000000019",
        inbox_id: secondInboxId,
        contact_id: "00000000-0000-4000-8000-000000000020",
        contact: { display_name: "Second customer", email: null, is_blocked: false },
        status: "open",
        subject: "Second Inbox conversation",
        last_message_sequence: 1,
        created_at: "2026-08-18T18:00:00Z",
        updated_at: "2026-08-18T18:01:00Z",
        operator: null,
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: telegramChannel,
      },
    ]);

    render(<App />);

    await openConversation(/Telegram refund/);
    expect(await screen.findAllByTitle("Support Telegram · Telegram bot")).toHaveLength(4);
    expect(screen.queryByRole("checkbox", { name: "Visitor files" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Attach a file" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Return to AI" })).toBeInTheDocument();

    const channelFilter = screen.getByRole("combobox", { name: "Conversation channel" });
    fireEvent.change(channelFilter, { target: { value: widgetChannel.id } });

    const conversationList = screen.getByRole("region", { name: "Conversation list" });
    const messagePanel = screen.getByRole("region", { name: "Conversation messages" });
    expect(within(conversationList).queryByRole("button", { name: /Telegram refund/ })).not.toBeInTheDocument();
    expect(within(messagePanel).getByText("Select a conversation.")).toBeInTheDocument();
    fireEvent.click(within(conversationList).getByRole("button", { name: /Widget payment/ }));

    expect(screen.getAllByTitle("Website widget · Widget")).toHaveLength(4);
    expect(screen.getByRole("checkbox", { name: "Visitor files" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Attach a file" })).toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: "Conversation status" }), {
      target: { value: "resolved" },
    });
    expect(within(messagePanel).getByText("Select a conversation.")).toBeInTheDocument();
    fireEvent.click(within(conversationList).getByRole("button", { name: /Resolved widget/ }));
    expect(within(conversationList).queryByRole("button", { name: /Widget payment/ })).not.toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: "Inbox" }), {
      target: { value: secondInboxId },
    });

    // The open conversation's link is left first, then the view remounts for the new inbox.
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Inbox" })).toHaveValue(secondInboxId));
    expect(within(screen.getByRole("region", { name: "Conversation messages" }))
      .getByText("Select a conversation.")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Conversation channel" })).toHaveValue("all");
  });

  it("omits empty conversations from the operator workspace", async () => {
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-4000-8000-000000000060",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-4000-8000-000000000061",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "new",
        subject: "Empty conversation",
        last_message_sequence: 0,
        created_at: "2026-08-17T18:00:00Z",
        updated_at: "2026-08-17T18:00:00Z",
        operator: null,
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
      {
        id: "00000000-0000-4000-8000-000000000062",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-4000-8000-000000000063",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Conversation with messages",
        last_message_sequence: 12,
        created_at: "2026-08-17T18:01:00Z",
        updated_at: "2026-08-17T18:02:00Z",
        operator: null,
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);

    render(<App />);

    expect(await screen.findByRole("button", { name: /Conversation with messages/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Empty conversation/ })).not.toBeInTheDocument();
  });

  it("shows a visitor profile as soon as the contact is updated", async () => {
    const anonymousConversation: api.OperatorConversation = {
      id: "00000000-0000-0000-0000-000000000005",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-0000-0000-000000000006",
      contact: { display_name: null, email: null, is_blocked: false },
      status: "open",
      subject: "Payment question",
      last_message_sequence: 1,
      created_at: "2026-08-01T00:00:00Z",
      updated_at: "2026-08-01T00:00:00Z",
      operator: null,
      ai_agent: null,
      assigned_to_me: false,
      unread_customer_messages: 0,
      widget_attachments_enabled: false,
      channel: widgetChannel,
    };
    vi.mocked(api.listInboxConversations)
      .mockResolvedValueOnce([anonymousConversation])
      .mockResolvedValueOnce([{
        ...anonymousConversation,
        contact: {
          is_blocked: false,
          display_name: "Elena Volkova",
          email: "elena@example.com",
        },
      }]);
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);

    expect((await screen.findAllByText("Payment question")).length).toBeGreaterThan(0);
    await openConversation();
    expect(screen.queryByText("Elena Volkova")).not.toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000070",
      inbox_id: anonymousConversation.inbox_id,
      contact_id: anonymousConversation.contact_id,
      type: "contact.updated",
      aggregate_id: anonymousConversation.contact_id,
      data: { contact_id: anonymousConversation.contact_id },
    }));

    await waitFor(() => expect(api.listInboxConversations).toHaveBeenCalledTimes(2));
    expect((await screen.findAllByText("Elena Volkova")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("elena@example.com").length).toBeGreaterThan(0);
  });

  it("shows unread customer messages only until that conversation is opened", async () => {
    const unreadConversationId = "00000000-0000-4000-8000-000000000050";
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-0000-0000-000000000005",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-0000-0000-000000000006",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Payment question",
        last_message_sequence: 1,
        created_at: "2026-08-01T00:00:00Z",
        updated_at: "2026-08-01T00:00:00Z",
        operator: null,
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
      {
        id: unreadConversationId,
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-4000-8000-000000000051",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Refund question",
        last_message_sequence: 4,
        created_at: "2026-08-15T03:59:00Z",
        updated_at: "2026-08-15T04:00:00Z",
        operator: null,
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 3,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);

    render(<App />);

    const unreadBadge = await screen.findByLabelText("Unread customer messages: 3");
    expect(unreadBadge).toHaveTextContent("3");

    fireEvent.click(screen.getByRole("button", { name: /Refund question/ }));

    await waitFor(() => expect(api.markOperatorConversationRead).toHaveBeenCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      unreadConversationId,
    ));
    expect(screen.queryByLabelText("Unread customer messages: 3")).not.toBeInTheDocument();
  });

  it("lets the assigned session operator change visitor file access in chat", async () => {
    sessionStorage.clear();
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "operator",
      auth_method: "session",
      email: "operator@example.com",
      display_name: "Local Administrator",
      chat_display_name: "Local Administrator",
      avatar_url: null,
      permissions: ["conversations:read", "conversations:reply", "contacts:read"],
    });
    vi.mocked(api.updateConversationAttachmentPolicy).mockResolvedValueOnce(true);

    render(<App />);

    await openConversation();
    const toggle = await screen.findByRole("checkbox", { name: "Visitor files" });
    expect(toggle).toBeEnabled();
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);

    await waitFor(() => expect(api.updateConversationAttachmentPolicy).toHaveBeenCalledWith(
      { kind: "session", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000005",
      true,
    ));
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it("shows when another operator is connected to the conversation", async () => {
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-0000-0000-000000000005",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-0000-0000-000000000006",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Payment question",
        last_message_sequence: 1,
        created_at: "2026-08-01T00:00:00Z",
        updated_at: "2026-08-01T00:00:00Z",
        operator: {
          display_name: "Anna Petrova",
          avatar_url: null,
          joined_at: "2026-08-15T05:00:00Z",
        },
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);

    render(<App />);

    await openConversation();
    expect(await screen.findByText("Operator connected")).toBeInTheDocument();
    expect(screen.getAllByText("Anna Petrova").length).toBeGreaterThan(0);
    expect(screen.getByText("Anna Petrova is handling this conversation.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Join" })).not.toBeInTheDocument();
  });

  it("shows an active AI agent and keeps human takeover available", async () => {
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-0000-0000-000000000005",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-0000-0000-000000000006",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Payment question",
        last_message_sequence: 1,
        created_at: "2026-08-01T00:00:00Z",
        updated_at: "2026-08-15T05:00:00Z",
        operator: null,
        ai_agent: {
          display_name: "Order assistant",
          avatar_url: null,
          joined_at: "2026-08-15T05:00:00Z",
          active: true,
        },
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);
    vi.mocked(api.joinConversation).mockResolvedValueOnce({
      operator: {
        display_name: "Local Administrator",
        avatar_url: null,
        joined_at: "2026-08-15T05:01:00Z",
      },
      joined_at: "2026-08-15T05:01:00Z",
    });

    render(<App />);

    await openConversation();
    const reply = await screen.findByLabelText("Reply");
    expect(screen.getByText("AI agent connected")).toBeInTheDocument();
    expect(screen.getAllByText("Order assistant").length).toBeGreaterThan(0);
    expect(screen.getByText(
      "AI agent Order assistant is handling this conversation. Join to take over.",
    )).toBeInTheDocument();
    expect(reply).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Join" }));

    await waitFor(() => expect(api.joinConversation).toHaveBeenCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000005",
    ));
    expect(screen.queryByText("AI agent connected")).not.toBeInTheDocument();
    expect(screen.getByText("You joined")).toBeInTheDocument();
    expect(reply).toBeEnabled();
  });

  it("returns an operator-owned conversation to its previous AI agent", async () => {
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-0000-0000-000000000005",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-0000-0000-000000000006",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Payment question",
        last_message_sequence: 3,
        created_at: "2026-08-01T00:00:00Z",
        updated_at: "2026-08-15T05:01:00Z",
        operator: {
          display_name: "Local Administrator",
          avatar_url: null,
          joined_at: "2026-08-15T05:01:00Z",
        },
        ai_agent: {
          display_name: "Order assistant",
          avatar_url: null,
          joined_at: "2026-08-15T05:00:00Z",
          active: false,
        },
        assigned_to_me: true,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);
    vi.mocked(api.returnConversationToAi).mockResolvedValueOnce({
      ai_agent: {
        display_name: "Order assistant",
        avatar_url: null,
        joined_at: "2026-08-15T05:02:00Z",
        active: true,
      },
      returned_at: "2026-08-15T05:02:00Z",
    });

    render(<App />);

    await openConversation();
    const reply = await screen.findByLabelText("Reply");
    expect(screen.getByText("You joined")).toBeInTheDocument();
    expect(reply).toBeEnabled();

    fireEvent.click(screen.getByRole("button", { name: "Return to AI" }));

    await waitFor(() => expect(api.returnConversationToAi).toHaveBeenCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000005",
    ));
    expect(screen.getByText("AI agent connected")).toBeInTheDocument();
    expect(screen.queryByText("You joined")).not.toBeInTheDocument();
    expect(reply).toBeDisabled();
  });

  it("lets a session administrator reply as the active AI agent without taking over", async () => {
    sessionStorage.clear();
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin",
      auth_method: "session",
      email: "admin@tzomet.local",
      display_name: "Local Administrator",
      chat_display_name: "Local Administrator",
      avatar_url: null,
      permissions: ["conversations:read", "conversations:reply"],
    });
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-0000-0000-000000000005",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-0000-0000-000000000006",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Payment question",
        last_message_sequence: 1,
        created_at: "2026-08-01T00:00:00Z",
        updated_at: "2026-08-15T05:00:00Z",
        operator: null,
        ai_agent: {
          display_name: "Order assistant",
          avatar_url: null,
          joined_at: "2026-08-15T05:00:00Z",
          active: true,
        },
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);
    vi.mocked(api.sendOperatorMessage).mockResolvedValueOnce({
      id: "00000000-0000-4000-8000-000000000031",
      conversation_id: "00000000-0000-0000-0000-000000000005",
      sequence: 2,
      direction: "outbound",
      kind: "text",
      author_kind: "ai",
      body: "I checked this for you.",
      status: "sent",
      created_at: "2026-08-15T05:01:00Z",
      attachments: [],
    });

    render(<App />);

    await openConversation();
    const reply = await screen.findByLabelText("Reply as AI agent Order assistant");
    expect(reply).toBeEnabled();
    expect(screen.getByText(
      "Your message will be sent as AI agent Order assistant. The agent will remain connected.",
    )).toBeInTheDocument();

    fireEvent.change(reply, { target: { value: "I checked this for you." } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(api.sendOperatorMessage).toHaveBeenCalledWith(
      { kind: "session", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000005",
      "I checked this for you.",
      "ai",
      "markdown",
    ));
    expect(api.joinConversation).not.toHaveBeenCalled();
  });

  describe("when another operator handles the conversation", () => {
    const defaultActor = vi.mocked(api.getCurrentActor).getMockImplementation()!;

    afterEach(() => {
      vi.mocked(api.getCurrentActor).mockReset().mockImplementation(defaultActor);
    });

    const operatorOwnedConversation = {
      id: "00000000-0000-0000-0000-000000000005",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-0000-0000-000000000006",
      contact: { display_name: null, email: null, is_blocked: false },
      status: "open",
      subject: "Payment question",
      last_message_sequence: 1,
      created_at: "2026-08-01T00:00:00Z",
      updated_at: "2026-08-15T05:00:00Z",
      operator: {
        display_name: "Anna Petrova",
        avatar_url: null,
        joined_at: "2026-08-15T05:00:00Z",
      },
      ai_agent: null,
      assigned_to_me: false,
      unread_customer_messages: 0,
      widget_attachments_enabled: false,
      channel: widgetChannel,
    } satisfies api.OperatorConversation;

    beforeEach(() => {
      sessionStorage.clear();
      vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
        actor_id: "00000000-0000-0000-0000-000000000001",
        tenant_id: "00000000-0000-0000-0000-000000000002",
        project_id: "00000000-0000-0000-0000-000000000003",
        role: "admin",
        auth_method: "session",
        email: "admin@tzomet.local",
        display_name: "Local Administrator",
        chat_display_name: "Local Administrator",
        avatar_url: null,
        permissions: ["conversations:read", "conversations:reply"],
      });
      vi.mocked(api.listInboxConversations).mockResolvedValueOnce([operatorOwnedConversation]);
    });

    it("lets a session administrator reply as that operator without taking over", async () => {
      vi.mocked(api.sendOperatorMessage).mockResolvedValueOnce({
        id: "00000000-0000-4000-8000-000000000032",
        conversation_id: "00000000-0000-0000-0000-000000000005",
        sequence: 2,
        direction: "outbound",
        kind: "text",
        author_kind: "operator",
        body: "Anna asked me to confirm the refund.",
        status: "sent",
        created_at: "2026-08-15T05:01:00Z",
        attachments: [],
      });

      render(<App />);

      await openConversation();
      const reply = await screen.findByLabelText("Reply as operator Anna Petrova");
      expect(reply).toBeEnabled();
      expect(screen.getByText(
        "Your message will be sent as operator Anna Petrova. Anna Petrova will keep handling the conversation.",
      )).toBeInTheDocument();
      expect(screen.queryByText("Anna Petrova is handling this conversation.")).not.toBeInTheDocument();

      fireEvent.change(reply, { target: { value: "Anna asked me to confirm the refund." } });
      fireEvent.click(screen.getByRole("button", { name: "Send" }));

      await waitFor(() => expect(api.sendOperatorMessage).toHaveBeenCalledWith(
        { kind: "session", projectId: defaultProjectId },
        "00000000-0000-0000-0000-000000000005",
        "Anna asked me to confirm the refund.",
        "assigned_operator",
        "markdown",
      ));
      expect(api.takeOverConversation).not.toHaveBeenCalled();
      expect(api.joinConversation).not.toHaveBeenCalled();
    });

    it("lets a session administrator join in place of that operator after confirming", async () => {
      const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
      vi.mocked(api.takeOverConversation).mockResolvedValueOnce({
        operator: {
          display_name: "Local Administrator",
          avatar_url: null,
          joined_at: "2026-08-15T05:02:00Z",
        },
        joined_at: "2026-08-15T05:02:00Z",
      });

      render(<App />);

      await openConversation();
      await screen.findByLabelText("Reply as operator Anna Petrova");
      const join = screen.getByRole("button", { name: "Take over" });
      expect(join).toHaveAttribute(
        "title",
        "The conversation will move to you and Anna Petrova will stop handling it",
      );

      fireEvent.click(join);
      expect(confirm).toHaveBeenLastCalledWith(
        "Join instead of Anna Petrova? The conversation will move to you and Anna Petrova will stop handling it.",
      );
      expect(api.takeOverConversation).not.toHaveBeenCalled();

      fireEvent.click(join);

      await waitFor(() => expect(api.takeOverConversation).toHaveBeenCalledWith(
        { kind: "session", projectId: defaultProjectId },
        "00000000-0000-0000-0000-000000000005",
      ));
      expect(api.joinConversation).not.toHaveBeenCalled();
      expect(await screen.findByText("You joined")).toBeInTheDocument();
      expect(screen.getByLabelText("Reply")).toBeEnabled();
      expect(screen.queryByRole("button", { name: "Take over" })).not.toBeInTheDocument();
      confirm.mockRestore();
    });

    it("lets an administrator transfer another operator's chat directly to AI", async () => {
      let finishReturn!: (value: Awaited<ReturnType<typeof api.returnConversationToAi>>) => void;
      vi.mocked(api.returnConversationToAi).mockReturnValueOnce(new Promise((resolve) => {
        finishReturn = resolve;
      }));
      render(<App />);
      await openConversation();
      await screen.findByLabelText("Reply as operator Anna Petrova");

      fireEvent.click(screen.getByRole("button", { name: "Return to AI" }));
      expect(screen.getByRole("button", { name: "Take over" })).toBeDisabled();
      expect(screen.getByLabelText("Reply as operator Anna Petrova")).toBeDisabled();
      await waitFor(() => expect(api.returnConversationToAi).toHaveBeenCalledWith(
        { kind: "session", projectId: defaultProjectId },
        operatorOwnedConversation.id,
      ));
      finishReturn({
        ai_agent: {
          display_name: "Order assistant", avatar_url: null,
          joined_at: "2026-08-15T05:02:00Z", active: true,
        },
        returned_at: "2026-08-15T05:02:00Z",
      });
      expect(await screen.findByText("AI agent connected")).toBeInTheDocument();
      expect(screen.queryByText("Operator connected")).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Return to AI" })).not.toBeInTheDocument();
      expect(api.takeOverConversation).not.toHaveBeenCalled();
      expect(api.joinConversation).not.toHaveBeenCalled();
    });

    it("keeps the operator visible when no eligible AI is available", async () => {
      vi.mocked(api.returnConversationToAi).mockRejectedValueOnce(new Error("No eligible agent"));
      vi.mocked(api.listInboxConversations).mockResolvedValueOnce([operatorOwnedConversation]);
      render(<App />);
      await openConversation();
      fireEvent.click(await screen.findByRole("button", { name: "Return to AI" }));
      expect(await screen.findByText(
        "Could not return the conversation to AI. Check that the AI agent is still available.",
      )).toBeInTheDocument();
      expect(screen.getByText("Operator connected")).toBeInTheDocument();
      expect(screen.getByLabelText("Reply as operator Anna Petrova")).toBeEnabled();
    });

    it("does not let another ordinary operator take over or transfer the chat", async () => {
      const actorMock = vi.mocked(api.getCurrentActor);
      actorMock.mockReset().mockImplementation(defaultActor).mockResolvedValueOnce({
        actor_id: "00000000-0000-0000-0000-000000000001",
        tenant_id: "00000000-0000-0000-0000-000000000002",
        project_id: defaultProjectId,
        role: "operator", auth_method: "session", email: "operator@example.com",
        display_name: "Other operator", chat_display_name: "Other operator", avatar_url: null,
        permissions: ["conversations:read", "conversations:reply"],
      });
      render(<App />);
      await openConversation();
      expect(await screen.findByText("Anna Petrova is handling this conversation.")).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Take over" })).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: "Return to AI" })).not.toBeInTheDocument();
    });

    it("offers joining over an operator and replying for them in Lite", async () => {
      document.documentElement.lang = "ru";

      render(<App />);

      await openConversation();
      expect(await screen.findByLabelText("Ответ от имени оператора Anna Petrova")).toBeEnabled();
      expect(screen.getByText(
        "Сообщение будет отправлено от имени оператора Anna Petrova. Anna Petrova продолжит вести диалог.",
      )).toBeInTheDocument();
      expect(screen.queryByText("Диалог ведёт Anna Petrova.")).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Взять себе" })).toHaveAttribute(
        "title",
        "Диалог перейдёт к вам, Anna Petrova перестанет его вести",
      );
      expect(screen.getByRole("button", { name: "Передать ИИ" })).toBeEnabled();
    });
  });

  it("joins an unassigned conversation before enabling replies", async () => {
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-0000-0000-000000000005",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-0000-0000-000000000006",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "new",
        subject: "Payment question",
        last_message_sequence: 1,
        created_at: "2026-08-01T00:00:00Z",
        updated_at: "2026-08-01T00:00:00Z",
        operator: null,
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);
    vi.mocked(api.joinConversation).mockResolvedValueOnce({
      operator: {
        display_name: "Anna Petrova",
        avatar_url: "https://cdn.example/anna.jpg",
        joined_at: "2026-08-14T00:00:00Z",
      },
      joined_at: "2026-08-14T00:00:00Z",
    });

    render(<App />);

    await openConversation();
    const reply = await screen.findByLabelText("Reply");
    expect(reply).toBeDisabled();
    expect(screen.getByText("Join this conversation before replying to the customer.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Join" }));

    await waitFor(() => expect(api.joinConversation).toHaveBeenCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000005",
    ));
    expect(reply).toBeEnabled();
    expect(screen.getAllByText("Anna Petrova").length).toBeGreaterThan(0);
    expect(screen.getByText("You joined")).toBeInTheDocument();
  });

  it.each([true, false])("resolves an unassigned conversation with requestRating=%s", async (requestRating) => {
    vi.mocked(api.listInboxConversations).mockResolvedValueOnce([
      {
        id: "00000000-0000-0000-0000-000000000005",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-0000-0000-000000000006",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "new",
        subject: "Payment question",
        last_message_sequence: 1,
        created_at: "2026-08-01T00:00:00Z",
        updated_at: "2026-08-01T00:00:00Z",
        operator: null,
        ai_agent: null,
        assigned_to_me: false,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);
    vi.mocked(api.resolveConversation).mockResolvedValueOnce({
      id: "00000000-0000-0000-0000-000000000008",
      conversation_id: "00000000-0000-0000-0000-000000000005",
      resolved_at: "2026-08-15T04:00:00Z",
    });

    render(<App />);

    await openConversation();
    const resolve = await screen.findByRole("button", { name: requestRating ? "Resolve" : "Resolve without requesting a rating" });
    expect(resolve).toBeEnabled();
    fireEvent.click(resolve);

    await waitFor(() => expect(api.resolveConversation).toHaveBeenCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000005",
      requestRating,
    ));
    expect(api.joinConversation).not.toHaveBeenCalled();
  });

  it("shows visitor typing only while active and animates operator message sending", async () => {
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });
    let resolveSend!: (message: api.Message) => void;
    vi.mocked(api.sendOperatorMessage).mockReturnValueOnce(new Promise((resolve) => {
      resolveSend = resolve;
    }));

    render(<App />);

    await openConversation();
    expect(await screen.findByText("Where is my payment?")).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());
    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000030",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-0000-0000-000000000006",
      type: "draft.updated",
      aggregate_id: "00000000-0000-0000-0000-000000000005",
      data: {
        conversation_id: "00000000-0000-0000-0000-000000000005",
        body: "I am checking the transaction",
      },
    }));

    await waitFor(() => {
      expect(document.querySelector(".visitor-live-draft-text")?.textContent).toBe(
        "I am checking the transaction",
      );
    });
    expect(screen.getByText("Visitor is writing")).toBeInTheDocument();
    expect(document.querySelectorAll(".visitor-typing-dots b")).toHaveLength(3);
    expect(document.querySelectorAll(".visitor-live-draft-character")).toHaveLength(0);
    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000032",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-0000-0000-000000000006",
      type: "draft.updated",
      aggregate_id: "00000000-0000-0000-0000-000000000005",
      data: {
        conversation_id: "00000000-0000-0000-0000-000000000005",
        body: "",
      },
    }));
    await waitFor(() => {
      expect(document.querySelector(".visitor-live-draft")).not.toBeInTheDocument();
    });

    const reply = screen.getByLabelText("Reply");
    fireEvent.change(reply, { target: { value: "We found it." } });
    expect(reply.closest("form")).toHaveClass("message-composer--typing");
    expect(screen.getByRole("button", { name: "Send" })).toHaveClass("message-send-button--ready");

    const messageScroll = document.querySelector(".message-scroll") as HTMLDivElement;
    const scrollTo = vi.fn();
    Object.defineProperty(messageScroll, "scrollHeight", { configurable: true, value: 720 });
    Object.defineProperty(messageScroll, "scrollTo", { configurable: true, value: scrollTo });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(await screen.findByRole("button", { name: "Sending…" })).toHaveClass(
      "message-send-button--sending",
    );
    const sentMessage: api.Message = {
      id: "00000000-0000-4000-8000-000000000031",
      conversation_id: "00000000-0000-0000-0000-000000000005",
      sequence: 2,
      direction: "outbound",
      kind: "text",
      author_kind: "operator",
      body: "We found it.",
      status: "sent",
      created_at: "2026-08-09T07:00:00Z",
      attachments: [],
    };
    vi.mocked(api.listOperatorMessages).mockResolvedValueOnce([sentMessage]);
    act(() => resolveSend(sentMessage));

    const sentArticle = (await screen.findByText("We found it.", { selector: ".message > div" })).closest("article");
    expect(sentArticle).toHaveClass("message--entering");
    await waitFor(() => expect(scrollTo).toHaveBeenCalledWith({ top: 720, behavior: "smooth" }));
  });

  it("heartbeats while the operator workspace is open and clears presence on disconnect", async () => {
    render(<App />);

    const disconnect = await screen.findByRole("button", { name: "Disconnect" });
    await waitFor(() => expect(api.refreshOperatorPresence).toHaveBeenCalledWith({
      kind: "access_token",
      token: "operator-token",
      projectId: defaultProjectId,
    }));

    fireEvent.click(disconnect);

    await waitFor(() => expect(api.clearOperatorPresence).toHaveBeenCalledWith({
      kind: "access_token",
      token: "operator-token",
      projectId: defaultProjectId,
    }));
  });

  it("keeps the conversation usable when visitor intelligence is unavailable", async () => {
    vi.mocked(api.getVisitorIntelligence).mockResolvedValueOnce(null);
    render(<App />);

    await openConversation();
    expect(await screen.findByText("Where is my payment?")).toBeInTheDocument();
    expect((await screen.findAllByText(
      "Visitor details are unavailable for this conversation.",
    )).length).toBeGreaterThan(0);
  });

  it("does not expose the removed system page and redirects its legacy URL", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin",
      auth_method: "access_token",
      email: null,
      display_name: null,
      chat_display_name: null,
      avatar_url: null,
      permissions: ["conversations:read", "system:read"],
    });
    window.history.replaceState(null, "", "/cabinet/system");
    render(<App />);

    expect(await screen.findByRole("heading", { name: "Conversations" })).toBeInTheDocument();
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
    expect(screen.queryByRole("link", { name: "System" })).not.toBeInTheDocument();
  });

  it("does not expose API integrations to access-token sessions", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin",
      auth_method: "access_token",
      email: null,
      display_name: null,
      chat_display_name: null,
      avatar_url: null,
      permissions: ["conversations:read", "integrations:manage"],
    });
    window.history.replaceState(null, "", "/cabinet/integrations");
    render(<App />);

    expect(await screen.findByRole("heading", { name: "Conversations" })).toBeInTheDocument();
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
    expect(screen.queryByRole("link", { name: "Integrations" })).not.toBeInTheDocument();
    expect(api.listApiIntegrations).not.toHaveBeenCalled();
  });

  it("opens API integrations for an employee assigned to a department", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "employee", tenant_id: "tenant", project_id: "project",
      role: "manager", auth_method: "session", email: "employee@example.test",
      display_name: "Employee", chat_display_name: "Employee", avatar_url: null,
      department_id: "support", department_name: "Support", department_restricted: true,
      is_director: false, can_access_director: false, inbox_scope: ["inbox"],
      permissions: ["integrations:manage"],
    });
    vi.mocked(departmentApi.listDepartments).mockResolvedValue({
      items: [{ id: "support", name: "Support", icon: "headset", sidebar_items: ["integrations"], default_page: "integrations", position: 0, inbox_ids: ["inbox"], member_count: 1 }],
      default_department_id: "support", director_enabled: true, can_access_director: false,
    });
    window.history.replaceState(null, "", "/cabinet/integrations");
    render(<App />);
    expect(await screen.findByRole("heading", { name: "API integrations" })).toBeInTheDocument();
    expect(api.listApiIntegrations).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "New API connection" }));
    expect(screen.queryByText("Projects and departments")).not.toBeInTheDocument();
  });

  it("does not expose project-wide API integrations to inbox-scoped sessions", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin",
      auth_method: "session",
      email: "scoped-admin@tzomet.local",
      display_name: "Scoped Administrator",
      chat_display_name: "Scoped Administrator",
      avatar_url: null,
      inbox_scope: ["00000000-0000-0000-0000-000000000004"],
      permissions: ["conversations:read", "integrations:manage"],
    });
    window.history.replaceState(null, "", "/cabinet/integrations");
    render(<App />);

    expect(await screen.findByRole("heading", { name: "Conversations" })).toBeInTheDocument();
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
    expect(screen.queryByRole("link", { name: "Integrations" })).not.toBeInTheDocument();
    expect(api.listApiIntegrations).not.toHaveBeenCalled();
  });

  it("opens the first permitted module when conversations are not allowed", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "operator",
      auth_method: "access_token",
      email: null,
      display_name: null,
      chat_display_name: null,
      avatar_url: null,
      permissions: ["ai:manage", "tasks:own", "tasks:manage"],
    });

    window.history.replaceState(null, "", "/cabinet/system");
    render(<App />);

    expect(await screen.findByRole("heading", { name: "AI" })).toBeInTheDocument();
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/ai")));
    expect(screen.queryByRole("link", { name: "Conversations" })).not.toBeInTheDocument();
  });

  it("shows and copies the website widget installation code", async () => {
    const publicWidgetId = "66666666-6666-6666-6666-666666666666";
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "manager",
      auth_method: "access_token",
      email: null,
      display_name: null,
      chat_display_name: null,
      avatar_url: null,
      permissions: ["conversations:read", "channels:read", "channels:manage"],
    });
    vi.mocked(api.listChannels).mockResolvedValueOnce([{
      id: "00000000-0000-0000-0000-000000000010",
      project_id: "00000000-0000-0000-0000-000000000003",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      public_id: publicWidgetId,
      blacklist_reply: { default_language: "en", translations: { en: "Blocked due to spam." } },
      kind: "widget",
      name: "Website widget",
      status: "active",
      allowed_origins: ["https://shop.example.com"],
      greeting: "How can we help?",
      default_language: "en" as const,
      translations: {
        en: { support_name: "Support", greeting: "How can we help?", offline_message: "Operators are offline. Leave a message and your contact details.", launcher_label: "Chat with us", rating_prompt: "How was this conversation?", rating_thanks: "Thank you for your feedback.", proactive_invitation_message: "Hi! Can I help you?" },
        ru: { support_name: "Служба поддержки", greeting: "Чем мы можем помочь?", offline_message: "Операторы офлайн. Оставьте сообщение и контакты.", launcher_label: "Напишите нам", rating_prompt: "Как прошёл разговор?", rating_thanks: "Спасибо за вашу оценку.", proactive_invitation_message: "Здравствуйте! Могу помочь?" },
      },
      launcher: {
        launcher_type: "icon",
        position: "bottom_right",
        label: "Chat with us",
        show_greeting: true,
        show_operator_profile: false,
        offset_x: 20,
        offset_y: 20,
        attention_animation: "pulse",
        animation_interval_seconds: 5,
        proactive_invitation_enabled: false,
        proactive_invitation_delay_seconds: 15,
      },
      theme: {
        accent_color: "#F05A28",
        accent_text_color: "#FFFFFF",
        surface_color: "#FFFFFF",
        text_color: "#1B1B1D",
        border_radius: 20,
      },
      updated_at: "2026-08-01T00:00:00Z",
    }]);

    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "Channels" }));
    const widgetsRow = (await screen.findByText("Website widgets")).closest("tr");
    expect(widgetsRow).not.toBeNull();
    fireEvent.click(within(widgetsRow as HTMLTableRowElement).getByRole("button", { name: "Open" }));
    fireEvent.click(await screen.findByRole("button", { name: "Website widget" }));

    expect(screen.getByRole("heading", { name: "Widget settings" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Languages" }));
    const englishSettings = screen.getByRole("group", { name: "en" });
    fireEvent.click(within(englishSettings).getByRole("button", { name: /^en/ }));
    expect(screen.getAllByText("Text after conversation closure")[0].closest("label")?.querySelector("textarea")).toHaveValue(
      "How was this conversation?",
    );
    fireEvent.click(screen.getByRole("tab", { name: "Installation" }));
    const installHeading = await screen.findByText("Install on your website");
    const expectedAttributes = [
      `src="${new URL(import.meta.env.VITE_WIDGET_LOADER_URL || "/loader/widget-loader.js", window.location.origin)}"`,
    ];
    if (import.meta.env.VITE_WIDGET_FRAME_URL) {
      expectedAttributes.push(`data-src="${new URL(import.meta.env.VITE_WIDGET_FRAME_URL, window.location.origin)}"`);
    }
    if (import.meta.env.VITE_WIDGET_API_BASE_URL) {
      expectedAttributes.push(`data-api-base="${new URL(import.meta.env.VITE_WIDGET_API_BASE_URL, window.location.origin)}"`);
    }
    expectedAttributes.push(`data-widget-id="${publicWidgetId}"`);
    expectedAttributes.push('data-theme="light"');
    const expectedCode = `<script\n${expectedAttributes.map((attribute) => `  ${attribute}`).join("\n")}\n></script>`;
    expect(installHeading.closest("section")?.querySelector("code")?.textContent).toBe(expectedCode);
    expect(screen.queryByText("Public widget ID")).not.toBeInTheDocument();
    expect(screen.getByText(/Language priority: data-language/)).toBeInTheDocument();
    expect(screen.getByText(/host site selects the palette explicitly/)).toBeInTheDocument();
    const adaptiveThemeSection = screen.getByText("Automatic light and dark theme switching").closest("section");
    expect(adaptiveThemeSection).not.toBeNull();
    expect(adaptiveThemeSection?.querySelector("code")?.textContent).toContain('id="tz-widget-loader"');
    expect(adaptiveThemeSection?.querySelector("code")?.textContent).toContain(`data-widget-id="${publicWidgetId}"`);
    expect(adaptiveThemeSection?.querySelector("code")?.textContent).toContain("new MutationObserver(syncWidgetTheme)");
    expect(adaptiveThemeSection?.querySelector("code")?.textContent).toContain("window.TzWidget?.setTheme(theme)");
    expect(adaptiveThemeSection?.querySelector("code")?.textContent).not.toContain("iframe.src =");

    fireEvent.click(screen.getByRole("button", { name: "Copy installation code" }));
    expect(writeText).toHaveBeenCalledWith(expectedCode);
    expect(await screen.findByRole("button", { name: "Code copied" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy automatic theme code" }));
    expect(writeText).toHaveBeenLastCalledWith(expect.stringContaining("localStorage.getItem('darkMode')"));
  });

  it("deletes a website widget after confirmation", async () => {
    const widgetId = "00000000-0000-0000-0000-000000000010";
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "manager",
      auth_method: "access_token",
      email: null,
      display_name: null,
      chat_display_name: null,
      avatar_url: null,
      permissions: ["conversations:read", "channels:read", "channels:manage"],
    });
    vi.mocked(api.listChannels).mockResolvedValueOnce([{
      id: widgetId,
      project_id: "00000000-0000-0000-0000-000000000003",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      public_id: "66666666-6666-6666-6666-666666666666",
      blacklist_reply: { default_language: "en", translations: { en: "Blocked due to spam." } },
      kind: "widget",
      name: "Checkout widget",
      status: "active",
      allowed_origins: ["https://shop.example.com"],
      greeting: "How can we help?",
      default_language: "en",
      translations: {
        en: { support_name: "Support", greeting: "How can we help?", offline_message: "Operators are offline. Leave a message and your contact details.", launcher_label: "Chat with us", rating_prompt: "How was this conversation?", rating_thanks: "Thank you for your feedback.", proactive_invitation_message: "Hi! Can I help you?" },
      },
      launcher: {
        launcher_type: "icon",
        position: "bottom_right",
        label: "Chat with us",
        show_greeting: true,
        show_operator_profile: false,
        offset_x: 20,
        offset_y: 20,
        attention_animation: "pulse",
        animation_interval_seconds: 5,
        proactive_invitation_enabled: false,
        proactive_invitation_delay_seconds: 15,
      },
      theme: {
        accent_color: "#F05A28",
        accent_text_color: "#FFFFFF",
        surface_color: "#FFFFFF",
        text_color: "#1B1B1D",
        border_radius: 20,
        footer_text: null,
      },
      notify_on_new_visitor: false,
      updated_at: "2026-08-01T00:00:00Z",
    }]);
    vi.mocked(api.deleteChannel).mockResolvedValueOnce(undefined);
    const confirmDelete = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "Channels" }));
    const widgetsRow = (await screen.findByText("Website widgets")).closest("tr");
    fireEvent.click(within(widgetsRow as HTMLTableRowElement).getByRole("button", { name: "Open" }));
    fireEvent.click(await screen.findByRole("button", { name: "Delete widget Checkout widget" }));

    expect(confirmDelete).toHaveBeenCalledWith(
      "Delete widget “Checkout widget”? It will stop working on websites immediately. Conversation history will be preserved.",
    );
    await waitFor(() => expect(api.deleteChannel).toHaveBeenCalledWith(
      expect.any(Object),
      widgetId,
    ));
    expect(await screen.findByText("Widget deleted.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Checkout widget" })).not.toBeInTheDocument();

    confirmDelete.mockRestore();
  });

  it("opens reply templates directly with a standalone management permission and creates a reply", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "operator",
      auth_method: "access_token",
      email: null,
      display_name: "Template manager",
      chat_display_name: null,
      avatar_url: null,
      permissions: ["reply_templates:manage"],
    });
    const template: api.ReplyTemplate = {
      id: "00000000-0000-4000-8000-000000000081",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      title: "Payment check",
      body: "**Checking your payment.**",
      body_format: "markdown",
      created_at: "2026-09-03T00:00:00Z",
      updated_at: "2026-09-03T00:00:00Z",
    };
    vi.mocked(api.listReplyTemplates).mockResolvedValueOnce([]);
    vi.mocked(api.createReplyTemplate).mockResolvedValueOnce(template);
    window.history.replaceState(null, "", "/cabinet/reply-templates");
    render(<App />);

    expect(await screen.findByRole("heading", { level: 1, name: "Reply templates" })).toBeInTheDocument();
    expect(await screen.findByText("No reply templates yet.")).toBeInTheDocument();
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/reply-templates"));
    expect(screen.getByRole("link", { name: "Reply templates" })).toHaveAttribute("aria-current", "page");
    expect(screen.queryByRole("link", { name: "Conversations" })).not.toBeInTheDocument();
    expect(api.listReplyTemplates).toHaveBeenCalledWith(expect.any(Object), template.inbox_id, expect.any(AbortSignal));

    fireEvent.click(screen.getByRole("button", { name: "Create template" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: template.title } });
    fireEvent.change(await screen.findByRole("textbox", { name: "Reply text" }), { target: { value: template.body } });
    fireEvent.click(screen.getByRole("button", { name: "Save template" }));
    expect(await screen.findByText("Template saved.")).toBeInTheDocument();
    expect(api.createReplyTemplate).toHaveBeenCalledExactlyOnceWith(expect.any(Object), template.inbox_id, { title: template.title, body: template.body });
  });

  it("lets replying operators read templates without granting management controls", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "operator",
      auth_method: "access_token",
      email: null,
      display_name: "Replying operator",
      chat_display_name: null,
      avatar_url: null,
      permissions: ["conversations:read", "conversations:reply"],
    });
    vi.mocked(api.listReplyTemplates).mockResolvedValueOnce([{
      id: "00000000-0000-4000-8000-000000000081",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      title: "Payment check",
      body: "**Checking your payment.**",
      body_format: "markdown",
      created_at: "2026-09-03T00:00:00Z",
      updated_at: "2026-09-03T00:00:00Z",
    }]);
    window.history.replaceState(null, "", "/cabinet/reply-templates");
    render(<App />);

    expect(await screen.findByText("Checking your payment.", { selector: "strong" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Reply templates" })).toHaveAttribute("href", scopedCabinetPath("/cabinet/reply-templates"));
    expect(screen.queryByRole("button", { name: "Create template" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save template" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete template" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Reply text" })).not.toBeInTheDocument();
  });

  it("protects unsaved reply template changes when leaving through the sidebar", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "operator",
      auth_method: "access_token",
      email: null,
      display_name: "Template manager",
      chat_display_name: null,
      avatar_url: null,
      permissions: ["conversations:read", "reply_templates:manage"],
    });
    vi.mocked(api.listReplyTemplates).mockResolvedValueOnce([{
      id: "00000000-0000-4000-8000-000000000081",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      title: "Payment check",
      body: "Checking your payment.",
      body_format: "markdown",
      created_at: "2026-09-03T00:00:00Z",
      updated_at: "2026-09-03T00:00:00Z",
    }]);
    window.history.replaceState(null, "", "/cabinet/reply-templates");
    render(<App />);
    const editor = await screen.findByRole("textbox", { name: "Reply text" });
    fireEvent.change(editor, { target: { value: "Unsaved payment reply." } });
    const discard = vi.spyOn(window, "confirm").mockReturnValue(false);
    try {
      fireEvent.click(screen.getByRole("link", { name: "Conversations" }));
      expect(discard).toHaveBeenCalledWith("Discard your unsaved changes?");
      expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/reply-templates"));
      expect(editor).toHaveValue("Unsaved payment reply.");
      expect(screen.getByRole("heading", { level: 1, name: "Reply templates" })).toBeInTheDocument();

      discard.mockReturnValue(true);
      fireEvent.click(screen.getByRole("link", { name: "Conversations" }));
      expect(await screen.findByRole("heading", { level: 1, name: "Conversations" })).toBeInTheDocument();
      expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet"));
      expect(api.updateReplyTemplate).not.toHaveBeenCalled();
    } finally {
      discard.mockRestore();
    }
  });

  it("shows grouped admin navigation and opens management workspaces", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin",
      auth_method: "session",
      email: "admin@tzomet.local",
      display_name: "Local Administrator",
      chat_display_name: "Local Administrator",
      avatar_url: null,
      inbox_scope: null,
      permissions: [
        "projects:read",
        "projects:manage",
        "conversations:read",
        "contacts:read",
        "visitor_network:read",
        "channels:read",
        "channels:manage",
        "access_tokens:manage",
        "quality:read",
        "reviews:read",
        "routing:manage",
        "teams:manage",
        "ai:manage",
        "tasks:own",
        "tasks:manage",
        "tasks:configure",
        "knowledge:manage",
        "integrations:manage",
        "roles:manage",
        "system:read",
      ],
    });

    window.history.replaceState(null, "", "/cabinet/tasks");
    render(<App />);

    expect(await screen.findByRole("heading", { level: 1, name: "Tasks" })).toBeInTheDocument();
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/tasks"));
    expect(await screen.findByRole("heading", { name: "Work" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Quality" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Management" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Administration" })).toBeInTheDocument();

    expect(screen.queryByRole("button", { name: "Teams & operators" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reviews" })).not.toBeInTheDocument();

    const mainNavigation = screen.getByRole("navigation", { name: "Main navigation" });
    expect(within(mainNavigation).getAllByRole("link").map((link) => [
      link.textContent,
      link.getAttribute("href"),
    ])).toEqual([
      ["Conversations", scopedCabinetPath("/cabinet")],
      ["Contacts", scopedCabinetPath("/cabinet/contacts")],
      ["Tasks", scopedCabinetPath("/cabinet/tasks")],
      ["Online visitors", scopedCabinetPath("/cabinet/online-visitors")],
      ["Support quality", scopedCabinetPath("/cabinet/support-quality")],
      ["AI", scopedCabinetPath("/cabinet/ai")],
      ["Inbox & routing", scopedCabinetPath("/cabinet/inbox-routing")],
      ["Channels", scopedCabinetPath("/cabinet/channels")],
      ["Knowledge base", scopedCabinetPath("/cabinet/knowledge-base")],
      ["Integrations", scopedCabinetPath("/cabinet/integrations")],
      ["Users", scopedCabinetPath("/cabinet/users")],
      ["Roles & permissions", scopedCabinetPath("/cabinet/roles")],
      ["Access tokens", scopedCabinetPath("/cabinet/access-tokens")],
    ]);
    expect(screen.getByRole("link", { name: "Tasks" })).toHaveAttribute("aria-current", "page");

    const management = screen.getByRole("heading", { name: "Management" }).closest("section");
    expect(management).not.toBeNull();
    expect(within(management as HTMLElement).getAllByRole("link").map((link) => link.textContent)).toEqual([
      "AI",
      "Inbox & routing",
      "Channels",
      "Knowledge base",
      "Integrations",
    ]);

    fireEvent.click(screen.getByRole("link", { name: "Inbox & routing" }));
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/inbox-routing"));
    expect(await screen.findByRole("heading", { level: 1, name: "Inbox & routing" })).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: /^Website widget:/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^SLA:/ })).toHaveTextContent("Resolve 30m");
    expect(api.getInboxRouting).toHaveBeenCalledWith(expect.any(Object), "00000000-0000-0000-0000-000000000004");

    const discardRouting = vi.spyOn(window, "confirm").mockReturnValue(false);
    fireEvent.change(screen.getByLabelText("And language is"), { target: { value: "ru" } });
    fireEvent.click(screen.getByRole("link", { name: "Knowledge base" }));
    expect(discardRouting).toHaveBeenCalledWith(
      "Discard unsaved routing changes and leave this page?",
    );
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/inbox-routing"));
    discardRouting.mockReturnValue(true);
    fireEvent.click(screen.getByRole("link", { name: "Knowledge base" }));
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/knowledge-base"));
    expect(await screen.findByRole("heading", { name: "Knowledge base" })).toBeInTheDocument();
    expect(await screen.findByText("No knowledge bases have been created.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("link", { name: "Tasks" }));
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/tasks"));
    expect(await screen.findByRole("heading", { name: "Tasks" })).toBeInTheDocument();
    expect(await screen.findByText("No tasks yet. Create one to assign work to your agents.")).toBeInTheDocument();
    expect(taskOrchestrationApi.listTasks).toHaveBeenCalled();

    fireEvent.click(screen.getByRole("link", { name: "AI" }));
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/ai"));
    expect(await screen.findByRole("heading", { name: "AI" })).toBeInTheDocument();
    expect(await screen.findByText("No agents have been configured.")).toBeInTheDocument();
    expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Agents",
      "Model connections",
      "Skills",
    ]);

    fireEvent.click(screen.getByRole("link", { name: "Integrations" }));
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/integrations"));
    expect(await screen.findByRole("heading", { name: "API integrations" })).toBeInTheDocument();
    expect(await screen.findByText("No API connections have been configured.")).toBeInTheDocument();
    expect(api.listApiIntegrations).toHaveBeenCalled();

  });

  it("guards browser Back while Inbox routing has unsaved changes", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin",
      auth_method: "session",
      email: "admin@tzomet.local",
      display_name: "Local Administrator",
      chat_display_name: "Local Administrator",
      avatar_url: null,
      inbox_scope: null,
      permissions: ["conversations:read", "routing:manage"],
    });
    window.history.replaceState(null, "", "/cabinet");
    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "Inbox & routing" }));

    fireEvent.change(await screen.findByLabelText("And language is"), {
      target: { value: "ru" },
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Save routing" })).toBeEnabled());
    const discardRouting = vi.spyOn(window, "confirm").mockReturnValue(false);

    act(() => window.history.back());
    await waitFor(() => expect(discardRouting).toHaveBeenCalledWith(
      "Discard unsaved routing changes and leave this page?",
    ));
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/inbox-routing")));
    expect(screen.getByRole("heading", { level: 1, name: "Inbox & routing" })).toBeInTheDocument();
    expect(screen.getByLabelText("And language is")).toHaveValue("ru");

    discardRouting.mockReturnValue(true);
    act(() => window.history.back());
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
    expect(await screen.findByRole("heading", { level: 1, name: "Conversations" })).toBeInTheDocument();
  });

  it.each([false, true])("keeps navigation read-only (Lite: %s)", async (lite) => {
    document.documentElement.lang = lite ? "ru" : "en";
    render(<App />);
    const navigation = await screen.findByRole("navigation", { name: lite ? "Основная навигация" : "Main navigation" });
    expect(within(navigation).queryByRole("button", { name: /Edit menu|Редактировать меню/ })).not.toBeInTheDocument();
    const links = within(navigation).getAllByRole("link");
    expect(links.every((link) => !link.hasAttribute("draggable"))).toBe(true);
    const order = links.map((link) => link.textContent);
    fireEvent.keyDown(links[1], { key: "ArrowUp", altKey: true, shiftKey: true });
    expect(within(navigation).getAllByRole("link").map((link) => link.textContent)).toEqual(order);
  });

  it("moves one shared navigation highlight down and back up", async () => {
    const rect = (top: number, height: number): DOMRect => ({
      bottom: top + height,
      height,
      left: 0,
      right: 220,
      top,
      width: 220,
      x: 0,
      y: top,
      toJSON: () => ({}),
    });
    const boundsSpy = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect")
      .mockImplementation(function (this: HTMLElement) {
        if (this.classList.contains("nav")) return rect(100, 500);
        const sidebarCollapsed = document.getElementById("admin-sidebar")
          ?.classList.contains("sidebar--collapsed") ?? false;
        if (this.getAttribute("href") === scopedCabinetPath("/cabinet")) return rect(sidebarCollapsed ? 100 : 124, 40);
        if (this.getAttribute("href") === scopedCabinetPath("/cabinet/contacts")) return rect(sidebarCollapsed ? 143 : 167, 40);
        return rect(0, 0);
      });

    try {
      render(<App />);

      const contactsLink = await screen.findByRole("link", { name: "Contacts" });
      const navigation = screen.getByRole("navigation", { name: "Main navigation" });
      const indicator = navigation.querySelector<HTMLElement>(".nav-active-indicator");
      expect(indicator).not.toBeNull();
      await waitFor(() => {
        expect(indicator).toHaveClass("nav-active-indicator--ready");
        expect(indicator?.style.transform).toBe("translate3d(0, 24px, 0)");
        expect(indicator?.style.height).toBe("40px");
      });

      fireEvent.click(contactsLink);

      await waitFor(() => {
        expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/contacts"));
        expect(indicator?.style.transform).toBe("translate3d(0, 67px, 0)");
      });
      expect(navigation.querySelector(".nav-active-indicator")).toBe(indicator);

      fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));
      await waitFor(() => {
        expect(indicator?.style.transform).toBe("translate3d(0, 43px, 0)");
      });
      expect(navigation.querySelector(".nav-active-indicator")).toBe(indicator);

      fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
      await waitFor(() => {
        expect(indicator?.style.transform).toBe("translate3d(0, 67px, 0)");
      });

      fireEvent.click(screen.getByRole("link", { name: "Conversations" }));

      await waitFor(() => {
        expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet"));
        expect(indicator?.style.transform).toBe("translate3d(0, 24px, 0)");
      });
    } finally {
      boundsSpy.mockRestore();
    }
  });

  it("moves focus into the mobile drawer and restores it when closing", async () => {
    render(<App />);

    const openMenuButton = await screen.findByRole("button", { name: "Open menu" });
    fireEvent.click(openMenuButton);

    expect(document.getElementById("admin-sidebar")).toHaveClass("sidebar--open");
    expect(
      screen.getAllByRole("button", { name: "Close menu" })
        .find((button) => button.classList.contains("mobile-close")),
    ).toHaveFocus();

    fireEvent.keyDown(document, { key: "Escape" });

    expect(document.getElementById("admin-sidebar")).not.toHaveClass("sidebar--open");
    expect(openMenuButton).toHaveFocus();
  });

  it.each(["restored", "manual"] as const)("opens conversations with a %s project access token without a department", async (signIn) => {
    const signedInActor: api.ActorContext = {
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      project_name: "Hinadex",
      role: "manager",
      auth_method: "access_token",
      email: null,
      display_name: "Hinadex",
      chat_display_name: "Hinadex",
      avatar_url: null,
      inbox_scope: null,
      department_id: null,
      department_name: null,
      department_default_page: null,
      department_restricted: false,
      is_director: false,
      can_access_director: false,
      permissions: ["conversations:read", "conversations:reply", "conversations:close", "contacts:read"],
    };
    if (signIn === "manual") {
      sessionStorage.clear();
      vi.mocked(api.getCurrentActor).mockRejectedValueOnce(new api.ApiRequestError("Unauthorized", 401));
    }
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce(signedInActor);
    vi.mocked(departmentApi.listDepartments).mockRejectedValue(new api.ApiRequestError("Forbidden", 403));

    render(<App />);

    if (signIn === "manual") {
      fireEvent.click(await screen.findByRole("button", { name: "Use an access token" }));
      fireEvent.change(screen.getByLabelText("Access token"), { target: { value: "operator-token" } });
      fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    }

    expect(await screen.findByRole("heading", { level: 1, name: "Conversations" })).toBeInTheDocument();
    const navigation = screen.getByRole("navigation", { name: "Main navigation" });
    expect(within(navigation).getByRole("link", { name: "Conversations" })).toHaveAttribute("aria-current", "page");
    expect(within(navigation).getByRole("link", { name: "Contacts" })).toBeInTheDocument();
    await waitFor(() => expect(api.listInboxConversationPage).toHaveBeenCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000004",
      expect.any(Object),
    ));
    expect(departmentApi.listDepartments).not.toHaveBeenCalled();
    expect(screen.queryByRole("combobox", { name: "Switch department" })).not.toBeInTheDocument();
    expect(screen.queryByText("Could not open the workspace. Try again.")).not.toBeInTheDocument();
  });

  it("keeps a direct admin link through sign-in", async () => {
    const signedInActor = {
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "operator" as const,
      auth_method: "access_token" as const,
      email: null,
      display_name: null,
      chat_display_name: null,
      avatar_url: null,
      inbox_scope: null,
      permissions: ["ai:manage", "tasks:own", "tasks:manage"],
    };
    window.history.replaceState(null, "", "/cabinet/tasks");
    sessionStorage.clear();
    vi.mocked(api.getCurrentActor)
      .mockRejectedValueOnce(new Error("No session"))
      .mockResolvedValueOnce(signedInActor);

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Use an access token" }));
    fireEvent.change(screen.getByLabelText("Access token"), {
      target: { value: "direct-link-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));

    expect(await screen.findByRole("heading", { level: 1, name: "Tasks" })).toBeInTheDocument();
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/tasks"));
    expect(screen.getByRole("link", { name: "Tasks" })).toHaveAttribute("aria-current", "page");
  });

  it("does not expose project-wide tasks to an inbox-scoped actor", async () => {
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "operator",
      auth_method: "access_token",
      email: null,
      display_name: null,
      chat_display_name: null,
      avatar_url: null,
      inbox_scope: ["00000000-0000-0000-0000-000000000004"],
      permissions: ["ai:manage", "tasks:own", "tasks:manage"],
    });

    render(<App />);

    expect(await screen.findByRole("link", { name: "AI" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Tasks" })).not.toBeInTheDocument();
    expect(taskOrchestrationApi.listTasks).not.toHaveBeenCalled();
  });

  it("shows saved visitor email and finds contacts by it", async () => {
    const anna = {
      is_blocked: false,
      id: "00000000-0000-4000-8000-000000000061",
      project_id: "00000000-0000-0000-0000-000000000003",
      display_name: "Anna Petrova",
      email: "anna@example.com",
      client_ip: "203.0.113.55",
      geo_ip: {
        country_code: "DE",
        country: "Germany",
        city: "Berlin",
        time_zone: "Europe/Berlin",
      },
      user_agent_details: {
        browser: "Example Browser",
        browser_version: "1.0",
        operating_system: "Example OS",
        operating_system_version: "15",
        device_category: "pc",
      },
      created_at: "2026-08-14T00:00:00Z",
      last_activity_at: "2026-08-14T13:25:00Z",
      channel_kinds: ["widget"],
      browser_timezone: "Europe/Berlin",
      updated_at: "2026-08-14T00:00:00Z",
    };
    const anonymous = {
      is_blocked: false,
      id: "00000000-0000-4000-8000-000000000062",
      project_id: "00000000-0000-0000-0000-000000000003",
      display_name: null,
      email: null,
      client_ip: null,
      geo_ip: null,
      user_agent_details: null,
      created_at: "2026-08-13T00:00:00Z",
      last_activity_at: "2026-08-14T13:25:00Z",
      channel_kinds: ["widget"],
      browser_timezone: "Europe/Berlin",
      updated_at: "2026-08-13T00:00:00Z",
    };
    vi.mocked(api.listContacts)
      .mockResolvedValueOnce({
        items: [anna, anonymous],
        total: 2,
        page: 1,
        per_page: 10,
        total_pages: 1,
        statistics: { contacts: 2, new_contacts: 1, returning_contacts: 1, conversations: 2, widget_sessions: 2, widget_contacts: 2, telegram_contacts: 0, countries: [] },
      })
      .mockResolvedValueOnce({
        items: [anna],
        total: 1,
        page: 1,
        per_page: 10,
        total_pages: 1,
        statistics: { contacts: 2, new_contacts: 1, returning_contacts: 1, conversations: 2, widget_sessions: 2, widget_contacts: 2, telegram_contacts: 0, countries: [] },
      });

    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "Contacts" }));

    expect(await screen.findByText("anna@example.com")).toBeInTheDocument();
    expect(screen.getByText("Anonymous visitor")).toBeInTheDocument();
    expect(screen.getByText("203.0.113.55")).toBeInTheDocument();
    expect(screen.getByTitle("Germany")).toBeInTheDocument();
    expect(screen.getByText("Example Browser 1.0")).toBeInTheDocument();
    expect(screen.getByText("Example OS 15")).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("Search contacts"), {
      target: { value: "anna@example.com" },
    });

    await waitFor(() => expect(api.listContacts).toHaveBeenLastCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      expect.objectContaining({ page: 1, per_page: 10, search: "anna@example.com", period: "day" }),
    ));
    expect(screen.getByText("Anna Petrova")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("Anonymous visitor")).not.toBeInTheDocument());
  });

  it("paginates contacts through the server-backed result set", async () => {
    const contact = (id: string, displayName: string) => ({
      is_blocked: false,
      id,
      project_id: "00000000-0000-0000-0000-000000000003",
      display_name: displayName,
      email: null,
      client_ip: null,
      geo_ip: null,
      user_agent_details: null,
      created_at: "2026-08-14T00:00:00Z",
      last_activity_at: "2026-08-14T13:25:00Z",
      channel_kinds: ["widget"],
      browser_timezone: "Europe/Berlin",
      updated_at: "2026-08-14T00:00:00Z",
    });
    vi.mocked(api.listContacts)
      .mockResolvedValueOnce({
        items: [contact("00000000-0000-4000-8000-000000000071", "First page contact")],
        total: 11,
        page: 1,
        per_page: 10,
        total_pages: 2,
        statistics: { contacts: 11, new_contacts: 11, returning_contacts: 0, conversations: 11, widget_sessions: 11, widget_contacts: 11, telegram_contacts: 0, countries: [] },
      })
      .mockResolvedValueOnce({
        items: [contact("00000000-0000-4000-8000-000000000072", "Second page contact")],
        total: 11,
        page: 2,
        per_page: 10,
        total_pages: 2,
        statistics: { contacts: 11, new_contacts: 11, returning_contacts: 0, conversations: 11, widget_sessions: 11, widget_contacts: 11, telegram_contacts: 0, countries: [] },
      });

    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "Contacts" }));

    expect(await screen.findByText("First page contact")).toBeInTheDocument();
    expect(screen.getByText("1–10 of 11")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));

    expect(await screen.findByText("Second page contact")).toBeInTheDocument();
    expect(screen.getByText("11–11 of 11")).toBeInTheDocument();
    expect(api.listContacts).toHaveBeenLastCalledWith(
      { kind: "access_token", token: "operator-token", projectId: defaultProjectId },
      expect.objectContaining({ page: 2, per_page: 10, search: undefined, period: "day" }),
    );
  });

  it("lets the signed-in user update their public name from the sidebar", async () => {
    sessionStorage.clear();
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin",
      auth_method: "session",
      email: "admin@tzomet.local",
      display_name: "Local Administrator",
      chat_display_name: "Local Administrator",
      avatar_url: null,
      permissions: ["projects:read", "conversations:read"],
    });
    vi.mocked(api.updateOperatorChatProfile).mockResolvedValueOnce({
      user_id: "00000000-0000-0000-0000-000000000001",
      display_name: "Anna Petrova",
      avatar_url: null,
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Edit your name and avatar" }));
    fireEvent.change(screen.getByLabelText("Name shown to customers"), {
      target: { value: "Anna Petrova" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save profile" }));

    await waitFor(() => expect(api.updateOperatorChatProfile).toHaveBeenCalledWith(
      { kind: "session", projectId: defaultProjectId },
      "00000000-0000-0000-0000-000000000001",
      {
        display_name: "Anna Petrova",
        avatar_url: null,
      },
    ));
    expect(await screen.findByRole("button", { name: "Edit your name and avatar" }))
      .toHaveTextContent("Anna Petrova");
  });

  it("keeps a workspace refresh error outside the current page and recovers after retry", async () => {
    sessionStorage.clear();
    window.history.replaceState(null, "", "/cabinet/users");
    const actor: api.ActorContext = {
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin", auth_method: "session", permissions: ["conversations:read", "roles:manage"],
      chat_display_name: "Administrator", avatar_url: null,
      department_id: null, department_restricted: false, can_access_director: true,
    };
    vi.mocked(api.getCurrentActor)
      .mockResolvedValueOnce(actor)
      .mockRejectedValueOnce(new Error("Workspace refresh failed"))
      .mockResolvedValueOnce(actor);
    vi.mocked(api.listRoles).mockResolvedValueOnce([{
      id: "operator", name: "Operator", base_role: "operator", permissions: ["conversations:read"],
      access_token_permissions: [], is_system: true, member_count: 1, active_token_count: 0,
    }]);
    vi.mocked(api.createProjectUser).mockResolvedValueOnce({
      membership_id: "membership-user", user_id: "new-user", email: "new-user@example.com",
      display_name: "New User", chat_display_name: "New User", avatar_url: null,
      role: "operator", role_id: "operator", role_name: "Operator",
      department_id: null, director_access: false,
      created_at: "2026-09-11T00:00:00Z", updated_at: "2026-09-11T00:00:00Z",
    });
    render(<App />);

    await screen.findByRole("heading", { level: 1, name: "Users" });
    await waitFor(() => expect(screen.getByRole("combobox", { name: "Role in project" })).toHaveValue("operator"));
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "New User" } });
    fireEvent.change(screen.getByRole("textbox", { name: "User email" }), { target: { value: "new-user@example.com" } });
    fireEvent.change(screen.getByLabelText("Password", { exact: true }), { target: { value: "x".repeat(12) } });
    fireEvent.click(screen.getByRole("button", { name: "Create user" }));

    const notice = await screen.findByRole("alert");
    expect(notice).toHaveTextContent("Could not open the workspace. Try again.");
    expect(api.createProjectUser).toHaveBeenCalledOnce();
    const content = screen.getByRole("main");
    expect(within(content).getByRole("heading", { level: 1, name: "Users" })).toBeVisible();
    expect(within(content).getByText("New User", { selector: "strong" })).toBeVisible();
    expect(content).not.toContainElement(notice);
    expect(content.previousElementSibling).toContainElement(notice);
    expect(content.previousElementSibling).toHaveAttribute("data-page-width", content.getAttribute("data-page-width"));

    fireEvent.click(screen.getByRole("link", { name: "Conversations" }));
    expect(await within(content).findByRole("heading", { level: 1, name: "Conversations" })).toBeVisible();
    expect(notice).toBeVisible();
    expect(content).not.toContainElement(notice);
    expect(content.previousElementSibling).toContainElement(notice);

    const departmentLoads = vi.mocked(departmentApi.listDepartments).mock.calls.length;
    fireEvent.click(within(notice).getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(departmentApi.listDepartments).toHaveBeenCalledTimes(departmentLoads + 1));
    expect(api.getCurrentActor).toHaveBeenCalledTimes(3);
    expect(notice).not.toBeInTheDocument();
    expect(within(content).getByRole("heading", { level: 1, name: "Conversations" })).toBeVisible();
  });

  it.each([false, true])("opens project users for a password administrator with department selection: %s", async (inDepartment) => {
    sessionStorage.clear();
    window.history.replaceState(null, "", "/cabinet/users");
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin",
      auth_method: "session",
      permissions: ["conversations:read", "roles:manage"],
      chat_display_name: "Administrator",
      avatar_url: null,
      department_id: inDepartment ? "support" : null,
      department_restricted: false,
      can_access_director: true,
    });
    vi.mocked(departmentApi.listDepartments).mockResolvedValue({
      items: [{ id: "support", name: "Support", icon: "headset", sidebar_items: ["conversations"], default_page: "conversations", position: 0, inbox_ids: [], member_count: 1 }],
      default_department_id: "support", director_enabled: true, can_access_director: true,
    });

    render(<App />);

    expect(await screen.findByRole("heading", { level: 1, name: "Users" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Users" })).toHaveAttribute("aria-current", "page");
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/users"));
    await waitFor(() => expect(api.listProjectMembers).toHaveBeenCalledWith(
      { kind: "session", projectId: defaultProjectId }, "00000000-0000-0000-0000-000000000003",
    ));
  });

  it.each([
    { role: "operator" as const, auth_method: "session" as const },
    { role: "manager" as const, auth_method: "session" as const },
    { role: "admin" as const, auth_method: "access_token" as const },
    { role: "admin" as const, auth_method: "session" as const, is_demo: true },
    { role: "admin" as const, auth_method: "session" as const, department_restricted: true },
  ])("denies the Users page to an unauthorized actor: %j", async (identity) => {
    window.history.replaceState(null, "", "/cabinet/users");
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      permissions: ["conversations:read"],
      chat_display_name: "User",
      avatar_url: null,
      ...identity,
    });

    render(<App />);

    await screen.findByRole("link", { name: "Conversations" });
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
    expect(screen.queryByRole("link", { name: "Users" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Users" })).not.toBeInTheDocument();
    expect(api.listProjectMembers).not.toHaveBeenCalled();
    expect(api.createProjectUser).not.toHaveBeenCalled();
  });

  it("shows online widget visitors without a separate notification control", async () => {
    sessionStorage.clear();
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "manager",
      auth_method: "access_token",
      email: "manager@tzomet.local",
      display_name: "Support Manager",
      chat_display_name: "Support Manager",
      avatar_url: null,
      permissions: ["conversations:read", "visitor_network:read"],
    });
    vi.mocked(api.listOnlineVisitors).mockResolvedValueOnce({
      online_window_seconds: 90,
      items: [{
        session_id: "00000000-0000-4000-8000-000000000020",
        contact_id: "00000000-0000-4000-8000-000000000021",
        channel_id: "00000000-0000-4000-8000-000000000022",
        widget_name: "Checkout widget",
        conversation_id: "00000000-0000-4000-8000-000000000023",
        client_ip: "203.0.113.55",
        user_agent: "Example Browser/1.0",
        geo_ip: {
          country_code: "DE",
          country: "Germany",
          city: "Berlin",
          time_zone: "Europe/Berlin",
        },
        user_agent_details: {
          browser: "Example Browser",
          browser_version: "1.0",
          operating_system: "Example OS",
          operating_system_version: "15",
          device_category: "pc",
        },
        origin: "https://shop.example",
        page_url: "https://shop.example/checkout",
        page_title: "Checkout",
        referrer: "https://search.example",
        language: "en",
        first_seen_at: "2026-08-09T00:00:00Z",
        last_seen_at: "2026-08-09T00:01:00Z",
      }],
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "Online visitors" }));

    expect(await screen.findByRole("heading", { level: 1, name: "Online visitors (1)" })).toBeInTheDocument();
    expect(screen.getAllByText("Checkout widget")).not.toHaveLength(0);
    expect(screen.getByText("Visits are recorded only on pages where the web widget is installed and loaded successfully. This list shows visitors of the selected widget who were active within the last 90 seconds.")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Widget" })).toHaveValue("00000000-0000-4000-8000-000000000022");
    expect(api.listOnlineVisitorWidgets).toHaveBeenCalledWith({ kind: "session", projectId: defaultProjectId });
    expect(api.listChannels).not.toHaveBeenCalled();
    expect(api.listOnlineVisitors).toHaveBeenCalledWith(
      { kind: "session", projectId: defaultProjectId },
      "00000000-0000-4000-8000-000000000022",
    );
    expect(screen.getByText("https://shop.example/checkout")).toBeInTheDocument();
    expect(screen.getByText("203.0.113.55")).toBeInTheDocument();
    expect(screen.getByText("Berlin, Germany (DE)")).toBeInTheDocument();
    expect(screen.getByText("Example Browser 1.0")).toBeInTheDocument();
    expect(screen.getByText("Example OS 15 · Desktop")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Enable notifications" })).not.toBeInTheDocument();
    expect(operatorNotifications.enableOperatorNotifications).not.toHaveBeenCalled();
  });

  it("explains unsupported browser notifications on the conversations page", async () => {
    render(<App />);

    expect(await screen.findByRole("button", { name: "Enable notifications" })).toBeDisabled();
    expect(screen.getByText("This browser does not support system notifications.")).toBeInTheDocument();
    expect(operatorNotifications.enableOperatorNotifications).not.toHaveBeenCalled();
  });

  it("lets operators request browser notifications directly from conversations", async () => {
    vi.mocked(operatorNotifications.currentBrowserNotificationPermission).mockReturnValueOnce("default");
    vi.mocked(operatorNotifications.enableOperatorNotifications).mockResolvedValueOnce("granted");
    render(<App />);

    const enable = await screen.findByRole("button", { name: "Enable notifications" });
    expect(screen.queryByRole("link", { name: "Online visitors" })).not.toBeInTheDocument();
    expect(operatorNotifications.enableOperatorNotifications).not.toHaveBeenCalled();
    fireEvent.click(enable);

    expect(await screen.findByRole("button", { name: "Disable notifications" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(operatorNotifications.enableOperatorNotifications).toHaveBeenCalledOnce();
    expect(localStorage.getItem("tz-operator-notifications")).toBe("enabled");

    fireEvent.click(screen.getByRole("button", { name: "Disable notifications" }));
    expect(screen.getByRole("button", { name: "Enable notifications" })).toHaveAttribute("aria-pressed", "false");
    expect(localStorage.getItem("tz-operator-notifications")).toBe("disabled");
    expect(operatorNotifications.enableOperatorNotifications).toHaveBeenCalledOnce();
  });

  it("keeps browser notifications disabled when permission is denied", async () => {
    vi.mocked(operatorNotifications.currentBrowserNotificationPermission).mockReturnValueOnce("default");
    vi.mocked(operatorNotifications.enableOperatorNotifications).mockResolvedValueOnce("denied");
    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Enable notifications" }));

    expect(await screen.findByText(
      "Browser notifications are blocked. Sound and in-app notifications remain available.",
    )).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Enable notifications" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(localStorage.getItem("tz-operator-notifications")).toBe("disabled");
  });

  it("prevents duplicate browser permission requests while one is pending", async () => {
    vi.mocked(operatorNotifications.currentBrowserNotificationPermission).mockReturnValueOnce("default");
    let resolvePermission!: (permission: BrowserNotificationPermission) => void;
    const permission = new Promise<BrowserNotificationPermission>((resolve) => {
      resolvePermission = resolve;
    });
    vi.mocked(operatorNotifications.enableOperatorNotifications).mockReturnValueOnce(permission);
    render(<App />);

    const toggle = await screen.findByRole("button", { name: "Enable notifications" });
    fireEvent.click(toggle);

    expect(toggle).toBeDisabled();
    fireEvent.click(toggle);
    expect(operatorNotifications.enableOperatorNotifications).toHaveBeenCalledOnce();

    await act(async () => {
      resolvePermission("granted");
      await permission;
    });
    expect(await screen.findByRole("button", { name: "Disable notifications" })).toBeEnabled();
  });

  it("refreshes browser permission after site settings change", async () => {
    vi.mocked(operatorNotifications.currentBrowserNotificationPermission)
      .mockReturnValueOnce("denied")
      .mockReturnValueOnce("granted");
    render(<App />);

    expect(await screen.findByRole("button", { name: "Enable notifications" })).toBeDisabled();

    act(() => window.dispatchEvent(new Event("focus")));

    await waitFor(() => expect(
      screen.getByRole("button", { name: "Enable notifications" }),
    ).toBeEnabled());
  });

  it("shows a realtime notification for a widget configured to notify", async () => {
    localStorage.setItem("tz-operator-notifications", "enabled");
    vi.mocked(operatorNotifications.currentBrowserNotificationPermission).mockReturnValueOnce("granted");
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });
    render(<App />);
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000030",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-4000-8000-000000000031",
      type: "visitor.entered",
      aggregate_id: "00000000-0000-4000-8000-000000000032",
      data: {
        notify: true,
        widget_name: "Checkout widget",
        page_title: "Checkout",
      },
    }));

    expect(await screen.findByText("New website visitor")).toBeInTheDocument();
    expect(screen.getByText("Checkout widget: Checkout")).toBeInTheDocument();
    expect(operatorNotifications.playOperatorNotificationSound).toHaveBeenCalledWith("pearl_chime");
    expect(operatorNotifications.showOperatorBrowserNotification).toHaveBeenCalledWith(
      "New website visitor",
      "Checkout widget: Checkout",
    );
  });

  it("chimes for a new message while the operator is viewing chats", async () => {
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Conversations" })).toBeInTheDocument();
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000035",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-4000-8000-000000000036",
      type: "message.created",
      aggregate_id: "00000000-0000-4000-8000-000000000037",
      data: {
        conversation_id: "00000000-0000-0000-0000-000000000005",
        direction: "inbound",
      },
    }));

    expect(operatorNotifications.playOperatorNotificationSound).toHaveBeenCalledOnce();
    expect(operatorNotifications.showOperatorBrowserNotification).not.toHaveBeenCalled();
  });

  it("shows a browser notification for an inbound customer message when allowed", async () => {
    localStorage.setItem("tz-operator-notifications", "enabled");
    vi.mocked(operatorNotifications.currentBrowserNotificationPermission).mockReturnValueOnce("granted");
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });
    render(<App />);
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000038",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-4000-8000-000000000039",
      type: "message.created",
      aggregate_id: "00000000-0000-4000-8000-000000000040",
      data: {
        conversation_id: "00000000-0000-0000-0000-000000000005",
        direction: "inbound",
      },
    }));

    expect(operatorNotifications.showOperatorBrowserNotification).toHaveBeenCalledWith(
      "New customer message",
      "A customer sent a new message.",
    );
  });

  it.each(["text", "attachment"])("refreshes blocked-contact %s events without notifying the operator", async (kind) => {
    localStorage.setItem("tz-operator-notifications", "enabled");
    vi.mocked(operatorNotifications.currentBrowserNotificationPermission).mockReturnValueOnce("granted");
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });
    render(<App />);
    await openConversation();
    await waitFor(() => expect(onRealtime).toBeDefined());
    const conversationRequests = vi.mocked(api.listInboxConversationPage).mock.calls.length;
    const event: api.RealtimeEvent = {
      event_id: "00000000-0000-4000-8000-000000000038",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      type: "message.created",
      aggregate_id: "00000000-0000-4000-8000-000000000040",
      data: { conversation_id: "00000000-0000-0000-0000-000000000005", direction: "inbound", kind, contact_is_blocked: true },
    };
    act(() => onRealtime?.(event));
    await waitFor(() => expect(vi.mocked(api.listInboxConversationPage).mock.calls.length).toBeGreaterThan(conversationRequests));
    expect(screen.queryByText("New customer message")).not.toBeInTheDocument();
    expect(operatorNotifications.showOperatorBrowserNotification).not.toHaveBeenCalled();
    expect(operatorNotifications.playOperatorNotificationSound).not.toHaveBeenCalled();

    act(() => onRealtime?.({ ...event, event_id: "00000000-0000-4000-8000-000000000041", data: { ...event.data, contact_is_blocked: false } }));
    expect(operatorNotifications.showOperatorBrowserNotification).toHaveBeenCalledWith("New customer message", "A customer sent a new message.");
    expect(operatorNotifications.playOperatorNotificationSound).toHaveBeenCalledOnce();
  });

  it("opens the exact conversation from a new message notification", async () => {
    const targetConversationId = "00000000-0000-4000-8000-000000000040";
    vi.mocked(api.listInboxConversations).mockResolvedValue([
      {
        id: "00000000-0000-0000-0000-000000000005",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-0000-0000-000000000006",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Payment question",
        last_message_sequence: 1,
        created_at: "2026-08-01T00:00:00Z",
        updated_at: "2026-08-01T00:00:00Z",
        operator: {
          display_name: "Local Administrator",
          avatar_url: null,
          joined_at: "2026-08-01T00:00:00Z",
        },
        ai_agent: null,
        assigned_to_me: true,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
      {
        id: targetConversationId,
        inbox_id: "00000000-0000-0000-0000-000000000004",
        contact_id: "00000000-0000-4000-8000-000000000041",
        contact: { display_name: null, email: null, is_blocked: false },
        status: "open",
        subject: "Refund question",
        last_message_sequence: 1,
        created_at: "2026-08-09T00:00:00Z",
        updated_at: "2026-08-09T00:00:00Z",
        operator: {
          display_name: "Local Administrator",
          avatar_url: null,
          joined_at: "2026-08-09T00:00:00Z",
        },
        ai_agent: null,
        assigned_to_me: true,
        unread_customer_messages: 0,
        widget_attachments_enabled: false,
        channel: widgetChannel,
      },
    ]);
    let onRealtime: ((event: api.RealtimeEvent) => void) | undefined;
    vi.mocked(api.openRealtimeSocket).mockImplementation((_ticket, onEvent) => {
      onRealtime = onEvent;
      return { close: vi.fn() } as unknown as WebSocket;
    });

    render(<App />);
    expect((await screen.findAllByText("Payment question")).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("link", { name: "Contacts" }));
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/contacts"));
    await waitFor(() => expect(onRealtime).toBeDefined());

    act(() => onRealtime?.({
      event_id: "00000000-0000-4000-8000-000000000042",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      contact_id: "00000000-0000-4000-8000-000000000041",
      type: "message.created",
      aggregate_id: "00000000-0000-4000-8000-000000000043",
      data: {
        conversation_id: targetConversationId,
        direction: "inbound",
      },
    }));

    fireEvent.click(await screen.findByRole("button", { name: "Open chat" }));

    expect(window.location.pathname).toBe(scopedCabinetPath(
      `/cabinet/conversations/00000000-0000-0000-0000-000000000004/${targetConversationId}`,
    ));
    expect(await screen.findByRole("heading", { name: "Conversations" })).toBeInTheDocument();
    await waitFor(() => expect(api.listOperatorMessages).toHaveBeenCalledWith(
      expect.anything(),
      targetConversationId,
    ));

    fireEvent.click(screen.getByRole("button", { name: "Close conversation view" }));
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet"));
    expect(within(screen.getByRole("region", { name: "Conversation messages" }))
      .getByText("Select a conversation.")).toBeInTheDocument();
    vi.mocked(api.listOperatorMessages).mockClear();

    fireEvent.click(screen.getByRole("link", { name: "Contacts" }));
    fireEvent.click(screen.getByRole("link", { name: "Conversations" }));

    expect(await within(screen.getByRole("region", { name: "Conversation messages" }))
      .findByText("Select a conversation.")).toBeInTheDocument();
    expect(api.listOperatorMessages).not.toHaveBeenCalled();
  });

  async function openWidgetCreation() {
    const createdWidget = {
      id: "00000000-0000-0000-0000-000000000011",
      project_id: "00000000-0000-0000-0000-000000000003",
      inbox_id: "00000000-0000-0000-0000-000000000004",
      public_id: "77777777-7777-7777-7777-777777777777",
      blacklist_reply: { default_language: "en", translations: { en: "Blocked due to spam." } },
      kind: "widget",
      name: "Checkout chat",
      status: "active",
      allowed_origins: ["https://shop.example.com"],
      greeting: "Need help?",
      default_language: "язык клиента",
      translations: {
        en: { support_name: "Support", greeting: "Need help?", offline_message: "We're offline. Leave your order number and contact details.", launcher_label: "Ask support", rating_prompt: "How was this conversation?", rating_thanks: "Thank you for your feedback.", proactive_invitation_message: "Need help choosing?" },
        ru: { support_name: "Служба поддержки", greeting: "Нужна помощь?", offline_message: "Операторы офлайн. Оставьте сообщение и контакты.", launcher_label: "Поддержка", rating_prompt: "Как прошёл разговор?", rating_thanks: "Спасибо за вашу оценку.", proactive_invitation_message: "Здравствуйте! Могу помочь?" },
        "язык клиента": { support_name: "Anna Petrova", greeting: "您好，需要帮助吗？", offline_message: "客服当前离线。请留言并留下联系方式。", launcher_label: "联系我们", rating_prompt: "How was this conversation?", rating_thanks: "Thank you for your feedback.", proactive_invitation_message: "需要帮助吗？" },
      },
      launcher: {
        launcher_type: "icon_text" as const,
        position: "bottom_left" as const,
        label: "联系我们",
        show_greeting: true,
        show_operator_profile: true,
        offset_x: 28,
        offset_y: 32,
        attention_animation: "sway" as const,
        animation_interval_seconds: 12,
        proactive_invitation_enabled: true,
        proactive_invitation_delay_seconds: 18,
      },
      theme: {
        accent_color: "#F05A28",
        accent_text_color: "#FFFFFF",
        surface_color: "#FFFFFF",
        text_color: "#1B1B1D",
        border_radius: 20,
      },
      updated_at: "2026-08-01T00:00:00Z",
    };
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "manager",
      auth_method: "access_token",
      email: null,
      display_name: null,
      chat_display_name: null,
      avatar_url: null,
      permissions: ["conversations:read", "channels:read", "channels:manage"],
    });
    vi.mocked(api.listChannels).mockResolvedValueOnce([]);
    vi.mocked(api.createWidgetChannel).mockResolvedValueOnce(createdWidget);

    render(<App />);
    fireEvent.click(await screen.findByRole("link", { name: "Channels" }));
    const widgetsRow = (await screen.findByText("Website widgets")).closest("tr");
    fireEvent.click(within(widgetsRow as HTMLTableRowElement).getByRole("button", { name: "Open" }));
    fireEvent.click((await screen.findAllByRole("button", { name: "Create widget" }))[0]);
  }

  it("uses neutral installation code", async () => {
    await openWidgetCreation();
    expect(screen.queryByRole("img", { name: "Tzomet" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Installation" }));
    const code = screen.getByText("Automatic light and dark theme switching").closest("section")?.querySelector("code")?.textContent;
    expect(code).toContain('id="tz-widget-loader"');
    expect(code).toContain("window.TzWidget?.setTheme(theme)");
    expect(code).not.toMatch(/tzomet|hinadex/i);
  });

  it("creates a website widget with launcher behavior", async () => {
    await openWidgetCreation();

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Checkout chat" } });
    fireEvent.change(screen.getByLabelText(/^Allowed origins/), { target: { value: "https://shop.example.com" } });
    fireEvent.click(screen.getByRole("checkbox", { name: /Notify operators about each new visitor/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Allow visitors to send files/ }));

    fireEvent.click(screen.getByRole("tab", { name: "Installation" }));
    expect(screen.getByText("Automatic light and dark theme switching")).toBeInTheDocument();
    expect(screen.getAllByText(/YOUR_WIDGET_ID/).length).toBeGreaterThan(0);

    fireEvent.click(screen.getByRole("tab", { name: "Languages" }));
    const englishSettings = screen.getByRole("group", { name: "en" });
    const englishToggle = within(englishSettings).getByRole("button", { name: /^en/ });
    expect(englishToggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(englishToggle);
    expect(englishToggle).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(screen.getByRole("tab", { name: "Launcher" }));
    const greetingToggle = screen.getByRole("checkbox", { name: /Show a greeting when the chat opens/ });
    const animationSelect = screen.getByLabelText("Attention animation");
    expect(animationSelect).toHaveValue("pulse");
    expect(within(animationSelect).getByRole("option", { name: /pulse/i })).toBeInTheDocument();
    fireEvent.click(greetingToggle);
    expect(animationSelect).toHaveValue("lift");
    expect(within(animationSelect).queryByRole("option", { name: /pulse/i })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Languages" }));
    expect(within(englishSettings).queryByLabelText("Greeting")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Launcher" }));
    fireEvent.click(greetingToggle);
    expect(animationSelect).toHaveValue("pulse");
    expect(within(animationSelect).getByRole("option", { name: /pulse/i })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /Show the joined operator/ }));
    fireEvent.change(screen.getByLabelText("Button type"), { target: { value: "icon_text" } });
    fireEvent.change(screen.getByLabelText("Position"), { target: { value: "bottom_left" } });
    fireEvent.change(animationSelect, { target: { value: "sway" } });
    fireEvent.change(screen.getByLabelText("Repeat every, seconds"), { target: { value: "12" } });
    fireEvent.click(screen.getByRole("checkbox", { name: /Show an automatic invitation/ }));
    fireEvent.change(screen.getByRole("spinbutton", { name: /^Show after, seconds/ }), { target: { value: "18" } });
    fireEvent.change(screen.getByRole("textbox", { name: /^Automatic invitation text · en/ }), { target: { value: "Need help choosing?" } });
    expect(screen.getAllByText("Need help choosing?")).toHaveLength(2);
    fireEvent.change(screen.getByLabelText("Side offset, px"), { target: { value: "28" } });
    fireEvent.change(screen.getByLabelText("Bottom offset, px"), { target: { value: "32" } });

    fireEvent.click(screen.getByRole("tab", { name: "Languages" }));
    fireEvent.change(within(englishSettings).getByLabelText("Greeting"), { target: { value: "Need help?" } });
    fireEvent.change(
      within(englishSettings).getByLabelText(/^Message when operators are offline/),
      { target: { value: "We're offline. Leave your order number and contact details." } },
    );
    fireEvent.change(within(englishSettings).getByLabelText(/^Support or operator name/), { target: { value: "Anna Petrova" } });
    fireEvent.change(within(englishSettings).getByLabelText("Button text"), { target: { value: "Ask support" } });
    fireEvent.click(screen.getByRole("button", { name: "Add language" }));
    const chineseSettings = screen.getByRole("group", { name: "New language" });
    fireEvent.change(within(chineseSettings).getByLabelText(/^Language/), { target: { value: "Язык клиента" } });
    fireEvent.change(within(chineseSettings).getByLabelText(/^Support or operator name/), { target: { value: "客户支持" } });
    fireEvent.change(within(chineseSettings).getByLabelText("Greeting"), { target: { value: "您好，需要帮助吗？" } });
    fireEvent.change(
      within(chineseSettings).getByLabelText(/^Message when operators are offline/),
      { target: { value: "客服当前离线。请留言并留下联系方式。" } },
    );
    fireEvent.change(within(chineseSettings).getByLabelText("Button text"), { target: { value: "联系我们" } });
    fireEvent.change(screen.getByLabelText(/^Default language/), { target: { value: "язык клиента" } });

    fireEvent.click(screen.getByRole("tab", { name: "Launcher" }));
    fireEvent.change(screen.getByRole("textbox", { name: /^Automatic invitation text · язык клиента/ }), { target: { value: "需要帮助吗？" } });

    fireEvent.click(screen.getByRole("button", { name: "Create widget" }));

    await waitFor(() => expect(api.createWidgetChannel).toHaveBeenCalledWith(
      expect.any(Object),
      expect.objectContaining({
        kind: "widget",
        name: "Checkout chat",
        inbox_id: "00000000-0000-0000-0000-000000000004",
        allowed_origins: ["https://shop.example.com"],
        default_language: "язык клиента",
        translations: {
          en: expect.objectContaining({
            support_name: "Anna Petrova",
            greeting: "Need help?",
            offline_message: "We're offline. Leave your order number and contact details.",
            launcher_label: "Ask support",
            rating_prompt: "Thanks for chatting with us. Please rate the support you received.",
            rating_thanks: "Thank you! Your feedback helps us improve.",
            proactive_invitation_message: "Need help choosing?",
            online_now: "Online now",
            message_placeholder: "Write a message…",
            contact_title: "Introduce yourself",
          }),
          ru: expect.objectContaining({
            support_name: "Support",
            greeting: "Чем мы можем помочь? Напишите нам.",
            offline_message: "Операторы офлайн. Оставьте сообщение и контакты.",
            launcher_label: "Напишите нам",
            rating_prompt: "Спасибо за обращение! Оцените, пожалуйста, качество поддержки.",
            rating_thanks: "Спасибо! Ваш отзыв поможет нам стать лучше.",
            proactive_invitation_message: "Здравствуйте! Могу помочь?",
          }),
          "язык клиента": expect.objectContaining({
            support_name: "客户支持",
            greeting: "您好，需要帮助吗？",
            offline_message: "客服当前离线。请留言并留下联系方式。",
            launcher_label: "联系我们",
            rating_prompt: "Thanks for chatting with us. Please rate the support you received.",
            rating_thanks: "Thank you! Your feedback helps us improve.",
            proactive_invitation_message: "需要帮助吗？",
          }),
        },
        launcher: {
          launcher_type: "icon_text",
          position: "bottom_left",
          label: "联系我们",
          show_greeting: true,
          show_operator_profile: true,
          offset_x: 28,
          offset_y: 32,
          attention_animation: "sway",
          animation_interval_seconds: 12,
          proactive_invitation_enabled: true,
          proactive_invitation_delay_seconds: 18,
        },
        notify_on_new_visitor: true,
        attachments_enabled: true,
      }),
    ));
    expect(await screen.findByRole("heading", { name: "Checkout chat" })).toBeInTheDocument();
    expect(screen.getByText("Widget created.")).toBeInTheDocument();
  });

  it("creates a website widget with independent light and dark appearance", async () => {
    await openWidgetCreation();
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Checkout chat" } });
    fireEvent.change(screen.getByLabelText(/^Allowed origins/), { target: { value: "https://shop.example.com" } });

    fireEvent.click(screen.getByRole("tab", { name: "Appearance" }));
    const appearance = within(screen.getByRole("tabpanel", { name: "Appearance" }));
    const fontSelect = appearance.getByRole("combobox", { name: "Font" });
    expect(fontSelect).toHaveValue("system");
    expect(appearance.getByRole("checkbox", { name: /Use website font/ })).not.toBeChecked();
    fireEvent.change(fontSelect, { target: { value: "georgia" } });
    fireEvent.click(appearance.getByRole("checkbox", { name: /Use website font/ }));
    expect(fontSelect).toBeEnabled();
    const replyTypingToggle = appearance.getByRole("checkbox", { name: /Show replies with a typing effect/ });
    expect(replyTypingToggle).not.toBeChecked();
    fireEvent.click(replyTypingToggle);
    fireEvent.change(appearance.getByLabelText("Corner radius, px"), { target: { value: "26" } });
    const independentColorFields = [
      ["status_text_color", "Header status text"],
      ["launcher_background_color", "Launcher background"],
      ["launcher_icon_color", "Launcher icon and text"],
      ["input_background_color", "Message input background"],
      ["input_text_color", "Message input text"],
      ["input_placeholder_color", "Message input placeholder"],
      ["footer_text_color", "Footer text color"],
      ["sound_icon_color", "Sound button icon"],
      ["close_icon_color", "Close button icon"],
      ["send_background_color", "Send button background"],
      ["send_icon_color", "Send button icon color"],
    ] as const;
    const lightControlColors = Object.fromEntries(independentColorFields.map(([key, label], index) => {
      const value = `#AA00${String(index).padStart(2, "0")}`;
      fireEvent.change(appearance.getByLabelText(`${label} hex color`), { target: { value } });
      return [key, value];
    }));
    fireEvent.change(appearance.getByLabelText("Launcher icon"), { target: { value: "headphones" } });
    fireEvent.change(appearance.getByLabelText("Send icon"), { target: { value: "arrow_up" } });
    expect(document.querySelector(".widget-appearance-launcher svg")).toHaveAttribute("data-widget-icon", "headphones");
    fireEvent.click(appearance.getByRole("button", { name: "Edit dark widget theme" }));
    const darkControlColors = Object.fromEntries(independentColorFields.map(([key, label], index) => {
      const value = `#00BB${String(index).padStart(2, "0")}`;
      expect(appearance.getByLabelText(`${label} hex color`)).not.toHaveValue(lightControlColors[key]);
      fireEvent.change(appearance.getByLabelText(`${label} hex color`), { target: { value } });
      return [key, value];
    }));
    fireEvent.click(appearance.getByRole("button", { name: "Edit light widget theme" }));
    for (const [key, label] of independentColorFields) {
      expect(appearance.getByLabelText(`${label} hex color`)).toHaveValue(lightControlColors[key]);
    }
    fireEvent.click(appearance.getByRole("button", { name: "Edit dark widget theme" }));
    fireEvent.change(appearance.getByLabelText("Surface hex color"), { target: { value: "#202124" } });
    fireEvent.change(appearance.getByLabelText("Chat background hex color"), { target: { value: "#18191B" } });
    fireEvent.change(appearance.getByLabelText("Border width"), { target: { value: "2" } });
    fireEvent.change(appearance.getByLabelText("Border color hex color"), { target: { value: "#55585E" } });
    const borderToggle = appearance.getByRole("checkbox", { name: /Show chat window border/ });
    fireEvent.click(borderToggle);
    expect(appearance.queryByLabelText("Border width")).not.toBeInTheDocument();
    fireEvent.click(borderToggle);
    fireEvent.click(screen.getByRole("button", { name: "Create widget" }));

    await waitFor(() => expect(api.createWidgetChannel).toHaveBeenCalledWith(
      expect.any(Object),
      expect.objectContaining({
        kind: "widget",
        name: "Checkout chat",
        theme: expect.objectContaining({
          accent_color: "#F05A28",
          accent_text_color: "#FFFFFF",
          surface_color: "#FFFFFF",
          text_color: "#1B1B1D",
          component_colors: expect.objectContaining({
            ...lightControlColors,
            background_color: "#FAFAFA",
            control_background_color: "#FFFFFF",
            divider_color: "#E4E4E4",
          }),
          dark: expect.objectContaining({
            accent_color: "#F05A28",
            accent_text_color: "#FFFFFF",
            surface_color: "#202124",
            text_color: "#F2F3F4",
            component_colors: expect.objectContaining({
              ...darkControlColors,
              background_color: "#18191B",
              control_background_color: "#1C1E21",
            }),
          }),
          border: {
            enabled: true,
            width: 2,
            light_color: "#E3E4E6",
            dark_color: "#55585E",
          },
          border_radius: 26,
          font_family: "georgia",
          use_site_font: true,
          reply_typing_effect: true,
          launcher_icon: "headphones",
          send_icon: "arrow_up",
          footer_text: null,
        }),
      }),
    ));
    expect(await screen.findByRole("heading", { name: "Checkout chat" })).toBeInTheDocument();
    expect(screen.getByText("Widget created.")).toBeInTheDocument();
  });


  it.each(["restored", "manual"] as const)("opens the workspace directly for a %s Lite password session and keeps department navigation hidden", async (signIn) => {
    document.documentElement.lang = "ru";
    sessionStorage.clear();
    const liteActor: api.ActorContext = {
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      project_name: "Internal workspace",
      role: "admin", auth_method: "session", inbox_scope: null,
      chat_display_name: null, avatar_url: null,
      department_id: "support", department_name: "Support", department_restricted: false, can_access_director: false,
      permissions: ["projects:read", "projects:manage", "conversations:read", "teams:manage"],
    };
    vi.mocked(departmentApi.listDepartments).mockResolvedValue({
      items: [{ id: "support", name: "Support", icon: "headset", sidebar_items: ["contacts", "ai"], default_page: "contacts", position: 0, inbox_ids: [], member_count: 1 }],
      default_department_id: "support", director_enabled: true, can_access_director: false,
    });
    if (signIn === "manual") {
      vi.mocked(api.getCurrentActor).mockRejectedValueOnce(new api.ApiRequestError("Unauthorized", 401));
      vi.mocked(api.loginWithPassword).mockResolvedValueOnce(liteActor);
    } else {
      vi.mocked(api.getCurrentActor).mockResolvedValueOnce(liteActor);
    }
    render(<App />);
    if (signIn === "manual") {
      fireEvent.change(await screen.findByLabelText("Электронная почта"), { target: { value: "admin@example.com" } });
      fireEvent.change(screen.getByLabelText("Пароль"), { target: { value: "test-password" } });
      fireEvent.click(screen.getByRole("button", { name: "Войти" }));
    }

    const navigation = await screen.findByRole("navigation", { name: "Основная навигация" });
    expect(await within(navigation).findByRole("link", { name: "Диалоги" })).toBeInTheDocument();
    expect(within(navigation).queryByRole("link", { name: "Команда" })).not.toBeInTheDocument();
    expect(within(navigation).queryByRole("link", { name: "Отделы" })).not.toBeInTheDocument();
    expect(within(navigation).queryByRole("link", { name: "Проекты" })).not.toBeInTheDocument();
    expect(within(navigation).queryByRole("link", { name: "Контакты" })).not.toBeInTheDocument();
    expect(within(navigation).queryByRole("link", { name: "ИИ-агенты и модели" })).not.toBeInTheDocument();
    expect(within(navigation).getByRole("heading", { name: "Работа" })).toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Переключить отдел" })).not.toBeInTheDocument();
    expect(screen.queryByText("Рабочее пространство")).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Проекты" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Выберите проект" })).not.toBeInTheDocument();
    expect(api.listProjects).toHaveBeenCalled();
    expect(api.selectProject).not.toHaveBeenCalled();
    expect(departmentApi.selectDepartment).not.toHaveBeenCalled();
    expect(api.listInboxes).toHaveBeenCalledWith({ kind: "session", projectId: liteActor.project_id });
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
  });

  it.each(["restored", "manual"] as const)("opens the workspace directly for a %s Lite token login", async (signIn) => {
    document.documentElement.lang = "ru";
    const liteActor: api.ActorContext = {
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      project_name: "Internal workspace",
      role: "manager", auth_method: "access_token", access_token_project_scope: "all",
      chat_display_name: null, avatar_url: null,
      permissions: ["conversations:read"], inbox_scope: null,
    };
    if (signIn === "manual") {
      sessionStorage.clear();
      vi.mocked(api.getCurrentActor).mockRejectedValueOnce(new api.ApiRequestError("Unauthorized", 401));
    }
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce(liteActor);
    render(<App />);
    if (signIn === "manual") {
      fireEvent.click(await screen.findByRole("button", { name: "Войти по токену доступа" }));
      fireEvent.change(screen.getByLabelText("Токен доступа"), { target: { value: "operator-token" } });
      fireEvent.click(screen.getByRole("button", { name: "Подключиться" }));
    }
    expect(await screen.findByRole("heading", { name: "Диалоги" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Проекты" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Выберите проект" })).not.toBeInTheDocument();
    expect(api.listProjects).toHaveBeenCalled();
    expect(api.selectProject).not.toHaveBeenCalled();
    expect(api.listInboxes).toHaveBeenCalledWith({ kind: "access_token", token: "operator-token", projectId: liteActor.project_id });
  });

  it("replaces the Lite projects URL with the first permitted workspace page", async () => {
    document.documentElement.lang = "ru";
    sessionStorage.clear();
    window.history.replaceState(null, "", scopedCabinetPath("/cabinet/projects"));
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: "00000000-0000-0000-0000-000000000003",
      role: "admin", auth_method: "session", inbox_scope: null,
      chat_display_name: null, avatar_url: null,
      permissions: ["projects:read", "projects:manage", "conversations:read"],
    });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Диалоги" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Проекты" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Создание проекта" })).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Проекты" })).toBeInTheDocument();
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet")));
  });

  it("keeps Lite closed when the server fails to resolve its fixed workspace", async () => {
    document.documentElement.lang = "ru";
    sessionStorage.clear();
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "00000000-0000-0000-0000-000000000001",
      tenant_id: "00000000-0000-0000-0000-000000000002",
      project_id: null, role: "admin", auth_method: "session",
      chat_display_name: null, avatar_url: null,
      permissions: ["projects:read", "projects:manage", "conversations:read"],
    });
    render(<App />);
    expect(await screen.findByRole("button", { name: "Войти" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Выберите проект" })).not.toBeInTheDocument();
    expect(screen.queryByRole("navigation", { name: "Основная навигация" })).not.toBeInTheDocument();
    expect(api.listInboxes).not.toHaveBeenCalled();
    expect(api.listProjects).not.toHaveBeenCalled();
  });

  it.each([
    { showDefaultChannels: false, director: false, visible: false },
    { showDefaultChannels: true, director: false, visible: true },
    { showDefaultChannels: false, director: true, visible: true },
  ])("applies the channel catalog setting to the active workspace: %j", async ({ showDefaultChannels, director, visible }) => {
    sessionStorage.clear();
    window.history.replaceState(null, "", "/cabinet/channels");
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "admin", tenant_id: "tenant", project_id: "project", project_name: "My business",
      role: "admin", auth_method: "session", chat_display_name: "Admin", avatar_url: null,
      permissions: ["channels:read", "channels:manage"], inbox_scope: null,
      department_id: director ? null : "support", department_name: director ? null : "Support", department_default_page: "channels",
      department_restricted: false, is_director: director, can_access_director: true,
    });
    vi.mocked(departmentApi.listDepartments).mockResolvedValue({
      items: [{ id: "support", name: "Support", icon: "headset", sidebar_items: ["channels"], default_page: "channels", show_default_channels: showDefaultChannels, position: 0, inbox_ids: ["inbox"], member_count: 1 }],
      default_department_id: "support", director_enabled: true, can_access_director: true,
    });
    render(<App />);
    await screen.findByRole("button", { name: "Add channel" });
    expect(Boolean(screen.queryByText("Website widgets"))).toBe(visible);
    expect(Boolean(screen.queryByText("Telegram"))).toBe(visible);
    expect(Boolean(screen.queryByText("Email"))).toBe(visible);
  });

  it.each([false, true])("opens public note links without cabinet authentication in edition lite=%s", async (lite) => {
    document.documentElement.lang = lite ? "ru" : "en";
    window.history.replaceState(null, "", "/notes/share/protected-notes");
    vi.stubGlobal("fetch", vi.fn().mockImplementation(async () => new Response(JSON.stringify({ error: { code: "unauthorized", message: "authentication required" } }), { status: 401 })));
    render(<App />);
    expect(await screen.findByRole("heading", { name: lite ? "Ссылка защищена паролем" : "This link is password protected" })).toBeInTheDocument();
    expect(api.getCurrentActor).not.toHaveBeenCalled();
    expect(api.managementApiRequest).not.toHaveBeenCalled();
    if (lite) {
      expect(document.body).not.toHaveTextContent("Tzomet");
      expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    }
  });

  it("protects a public note draft when browser history changes the selected shared page", async () => {
    const firstId = "71000000-0000-4000-8000-000000000010";
    const secondId = "71000000-0000-4000-8000-000000000011";
    const first = { id: firstId, parent_id: null, title: "First shared page", body: "First body", icon: "", version: 1, created_at: "2026-09-21T00:00:00Z", updated_at: "2026-09-21T00:00:00Z" };
    const second = { ...first, id: secondId, title: "Second shared page" };
    const link = "/notes/share/shared-pages";
    window.history.replaceState(null, "", `${link}?note=${secondId}`);
    vi.stubGlobal("fetch", vi.fn().mockImplementation(async (_url, options) => {
      const input = JSON.parse(options.body as string);
      if (input.action === "embeds") return new Response(JSON.stringify({ items: [] }));
      return new Response(JSON.stringify({ can_edit: true, root_note_id: null, items: [first, second], note: input.note_id === secondId ? second : first }));
    }));
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "First shared page" }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("First shared page"));
    fireEvent.change(screen.getByRole("textbox", { name: "Page title" }), { target: { value: "My public draft" } });
    act(() => window.history.back());
    await waitFor(() => expect(confirm).toHaveBeenCalledWith("Discard your unsaved changes?"));
    await waitFor(() => expect(window.location.search).toBe(`?note=${firstId}`));
    expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("My public draft");
    confirm.mockReturnValue(true);
    act(() => window.history.back());
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Page title" })).toHaveValue("Second shared page"));
    confirm.mockRestore();
  });

  it("opens project notes and protects an unsaved page when leaving the section", async () => {
    sessionStorage.clear();
    window.history.replaceState(null, "", "/cabinet/notes");
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "editor", tenant_id: "tenant", project_id: "project", project_name: "Company",
      chat_display_name: "Editor", avatar_url: null, role: "operator", auth_method: "session",
      permissions: ["notes:read", "notes:write", "contacts:read"], inbox_scope: null,
      department_id: "hr", department_name: "HR", department_default_page: "notes",
      department_restricted: true, is_director: false, can_access_director: false,
    });
    vi.mocked(departmentApi.listDepartments).mockResolvedValue({
      items: [{ id: "hr", name: "HR", icon: "users", sidebar_items: ["notes", "contacts"], default_page: "notes", position: 0, inbox_ids: [], member_count: 1 }],
      default_department_id: "hr", director_enabled: true, can_access_director: false,
    });
    vi.mocked(api.managementApiRequest).mockResolvedValue({ items: [] });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<App />);
    fireEvent.click((await screen.findAllByRole("button", { name: "New page" }))[0]);
    fireEvent.change(await screen.findByRole("textbox", { name: "Page title" }), { target: { value: "Meeting decisions" } });
    fireEvent.click(screen.getByRole("link", { name: "Contacts" }));
    expect(confirm).toHaveBeenCalledWith("Discard unsaved note changes?");
    expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/notes/new", "project"));
    confirm.mockReturnValue(true);
    fireEvent.click(screen.getByRole("link", { name: "Contacts" }));
    await waitFor(() => expect(window.location.pathname).toBe(scopedCabinetPath("/cabinet/contacts", "project")));
    expect(api.managementApiRequest).toHaveBeenCalledWith({ kind: "session", projectId: "project" }, "/api/v1/notes", expect.anything());
    confirm.mockRestore();
  });

  it.each([false, true])("shows read-only project notes in edition lite=%s", async (lite) => {
    document.documentElement.lang = lite ? "ru" : "en";
    sessionStorage.clear();
    window.history.replaceState(null, "", "/cabinet/notes");
    vi.mocked(api.getCurrentActor).mockResolvedValueOnce({
      actor_id: "reader", tenant_id: "tenant", project_id: "project", project_name: "Company",
      chat_display_name: "Reader", avatar_url: null, role: "operator", auth_method: "session",
      permissions: ["notes:read"], inbox_scope: null, department_restricted: false,
      is_director: false, can_access_director: false,
    });
    vi.mocked(api.managementApiRequest).mockResolvedValue({ items: [] });
    render(<App />);
    expect(await screen.findByRole("link", { name: lite ? "Заметки" : "Notes" })).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: lite ? "Заметки" : "Notes" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: lite ? "Новая страница" : "New page" })).not.toBeInTheDocument();
  });

});
