import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import * as channelApi from "../custom-ai-channel-api";
import * as departmentApi from "../department-api";
import { createI18n, I18nContext } from "../i18n";
import { ChannelsView } from "./ChannelsView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";

vi.mock("../custom-ai-channel-api", () => ({ listCustomAiChannels: vi.fn(), createCustomAiChannel: vi.fn(), updateCustomAiChannel: vi.fn(), deleteCustomAiChannel: vi.fn() }));
vi.mock("../department-api", () => ({ listDepartments: vi.fn() }));
vi.mock("../api", async (original) => ({ ...await original<typeof import("../api")>(), managementApiRequest: vi.fn(), listAiProfiles: vi.fn(), listChannels: vi.fn() }));

const auth: api.OperatorAuth = { kind: "session" };
const inbox: api.Inbox = { id: "inbox", project_id: "project", name: "Messages", status: "active", created_at: "2026-09-11T00:00:00Z" };
const profile: api.AiProfile = {
  id: "agent", provider_connection_id: null, name: "Support agent", avatar_url: null, status: "draft", mode: "copilot", model: null,
  instructions: "Read customer requests.", tool_instructions: "", blacklist_reply_text: "", blacklist_reply_match_language: true,
  http_allowed_hosts: [], capabilities: { http_get: true, http_post: false, shell: false }, language: "en", max_output_tokens: 800,
  auto_join_new_conversations: false, can_resolve_conversations: false, telegram_notifications: { new_visitor: false, new_message: false, operator_request: false },
  custom_fields: [], secrets: [], knowledge_base_ids: [], channel_ids: [], public_identities: [], created_at: "2026-09-11T00:00:00Z", updated_at: "2026-09-11T00:00:00Z",
};
const channel: channelApi.CustomAiChannel = {
  id: "channel", icon: "bot", name: "LinkedIn requests", inbox_id: inbox.id, connection_type: "ai_agent", destination: "conversations", mode: "agent", source_url: "https://www.linkedin.com/messaging/", source_kind: "website", instructions: "Read new messages and summarize them.", ai_profile_id: profile.id,
  status: "draft", created_at: "2026-09-11T00:00:00Z", updated_at: "2026-09-11T00:00:00Z",
};

function show(canManage = true, showDefaultChannels?: boolean) {
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><ChannelsView auth={auth} inboxes={[inbox]} canManage={canManage} showDefaultChannels={showDefaultChannels} /></I18nContext.Provider>);
}

function showAt(segments: string[]) {
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><MemoryPageRoute initialSegments={segments}><ChannelsView auth={auth} inboxes={[inbox]} canManage /><CurrentPageRoute /></MemoryPageRoute></I18nContext.Provider>);
}

const pageRoute = () => screen.getByTestId("page-route").textContent;

async function createForm() {
  fireEvent.click(await screen.findByRole("button", { name: "Add channel" }));
  await screen.findByRole("option", { name: "Support agent" });
  fireEvent.change(screen.getByLabelText("Channel name"), { target: { value: channel.name } });
  fireEvent.change(screen.getByLabelText("Website or service (optional)"), { target: { value: channel.source_url } });
}

beforeEach(() => {
  vi.clearAllMocks();
    vi.mocked(api.managementApiRequest).mockResolvedValue({ projects: [], departments: [] });
  vi.mocked(channelApi.listCustomAiChannels).mockResolvedValue([]);
  vi.mocked(api.listAiProfiles).mockResolvedValue([profile]);
  vi.mocked(api.listChannels).mockResolvedValue([]);
  vi.mocked(departmentApi.listDepartments).mockResolvedValue({ items: [{ id: "department", name: "Support", icon: "headset", sidebar_items: [], default_page: "conversations", position: 0, inbox_ids: [inbox.id], member_count: 1 }], default_department_id: "department", director_enabled: false, can_access_director: false });
  vi.mocked(channelApi.createCustomAiChannel).mockImplementation(async (_auth, input) => ({ ...channel, ...input }));
  vi.mocked(channelApi.updateCustomAiChannel).mockImplementation(async (_auth, _id, input) => ({ ...channel, ...input }));
  vi.mocked(channelApi.deleteCustomAiChannel).mockResolvedValue();
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe("Custom AI channel settings", () => {
  it("hides standard channel types while keeping custom channels editable", async () => {
    vi.mocked(channelApi.listCustomAiChannels).mockResolvedValue([channel]);
    show(true, false);
    const row = (await screen.findByText(channel.name)).closest("tr")!;
    expect(screen.queryByText("Website widgets")).not.toBeInTheDocument();
    expect(screen.queryByText("Telegram")).not.toBeInTheDocument();
    expect(screen.queryByText("Email")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add channel" })).toBeEnabled();
    fireEvent.click(within(row).getByRole("button", { name: "Open settings" }));
    expect(await screen.findByLabelText("Channel name")).toHaveValue(channel.name);
  });

  it("shows an empty state and allows creation when standard channels are hidden", async () => {
    show(true, false);
    expect(await screen.findByText("No custom channels yet.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add channel" }));
    expect(await screen.findByLabelText("Channel name")).toHaveValue("");
  });

  it("adds a named channel directly from the catalog and saves its chosen icon, agent, source and task as a draft", async () => {
    show();
    await screen.findByRole("button", { name: "Add channel" });
    expect(screen.getByText("Website widgets")).toBeInTheDocument();
    expect(screen.getByText("Telegram")).toBeInTheDocument();
    expect(screen.getByText("Email")).toBeInTheDocument();
    expect(screen.queryByText("External API")).not.toBeInTheDocument();
    await createForm();
    expect(screen.getByRole("option", { name: "Messages" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Source type")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Icon: LinkedIn" }));
    expect(screen.getByRole("button", { name: "Icon: LinkedIn" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("button", { name: /activate|start|test/i })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Agent"), { target: { value: profile.id } });
    fireEvent.change(screen.getByLabelText("Task for the agent (optional)"), { target: { value: channel.instructions } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(channelApi.createCustomAiChannel).toHaveBeenCalledWith(auth, {
      name: channel.name, icon: "linkedin", source_url: channel.source_url, inbox_id: inbox.id, connection_type: "ai_agent", destination: "conversations", mode: "agent", source_kind: "website", instructions: channel.instructions, ai_profile_id: profile.id,
    }));
    await screen.findByRole("button", { name: "Add channel" });
    const row = screen.getByText(channel.name).closest("tr")!;
    expect(within(row).getByText("Draft")).toBeInTheDocument();
    expect(screen.queryByText("Custom channel")).not.toBeInTheDocument();
    await waitFor(() => expect(api.listChannels).toHaveBeenCalledTimes(2));
  });

  it("requires a valid HTTPS source and meaningful instructions, and clears an old agent selection in instructions mode", async () => {
    show();
    await createForm();
    fireEvent.change(screen.getByLabelText("Agent"), { target: { value: profile.id } });
    fireEvent.click(screen.getByText("Other configuration options"));
    fireEvent.click(screen.getByLabelText("Instructions without an agent"));
    fireEvent.change(screen.getByLabelText("Website or service (optional)"), { target: { value: "https://login:secret@example.com/messages" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Enter a valid HTTPS URL without credentials.");
    fireEvent.change(screen.getByLabelText("Website or service (optional)"), { target: { value: channel.source_url } });
    fireEvent.change(screen.getByLabelText("Action instructions"), { target: { value: "Read" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(screen.getByRole("alert")).toHaveTextContent("at least 10 characters");
    expect(channelApi.createCustomAiChannel).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Action instructions"), { target: { value: channel.instructions } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(channelApi.createCustomAiChannel).toHaveBeenCalledWith(auth, expect.objectContaining({ mode: "instructions", ai_profile_id: null, instructions: channel.instructions })));
  });

  it("requires an available agent before saving agent mode", async () => {
    show();
    await createForm();
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Choose an available agent.");
    expect(channelApi.createCustomAiChannel).not.toHaveBeenCalled();
  });

  it("switches to external API without an agent or source URL and persists the task destination", async () => {
    show();
    await createForm();
    fireEvent.change(screen.getByLabelText("Agent"), { target: { value: profile.id } });
    fireEvent.click(screen.getByRole("radio", { name: "External API" }));
    expect(screen.queryByRole("combobox", { name: "Agent" })).not.toBeInTheDocument();
    expect(screen.queryByText("Other configuration options")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Website / API documentation (optional)"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("What to do in the workspace"), { target: { value: "tasks" } });
    expect(screen.getByLabelText("Inbox")).toHaveValue(inbox.id);
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(channelApi.createCustomAiChannel).toHaveBeenCalledWith(auth, expect.objectContaining({ connection_type: "external_api", destination: "tasks", mode: "instructions", ai_profile_id: null, source_kind: "api", source_url: "", instructions: "" })));
    await screen.findByRole("button", { name: "Add channel" });
    expect(within(screen.getByText(channel.name).closest("tr")!).getByText("External API · Create tasks")).toBeInTheDocument();
  });

  it("allows an internal AI workflow without a website and keeps the selected destination on reopen", async () => {
    show();
    await createForm();
    fireEvent.change(screen.getByLabelText("Agent"), { target: { value: profile.id } });
    fireEvent.change(screen.getByLabelText("Website or service (optional)"), { target: { value: "" } });
    fireEvent.change(screen.getByLabelText("What to do in the workspace"), { target: { value: "tasks" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(channelApi.createCustomAiChannel).toHaveBeenCalledWith(auth, expect.objectContaining({ connection_type: "ai_agent", destination: "tasks", ai_profile_id: profile.id, source_url: "" })));
    fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));
    expect(screen.getByLabelText("What to do in the workspace")).toHaveValue("tasks");
    expect(screen.getByLabelText("Website or service (optional)")).toHaveValue("");
  });

  it("requires meaningful instructions for a custom action even for an external API", async () => {
    show();
    await createForm();
    fireEvent.click(screen.getByRole("radio", { name: "External API" }));
    fireEvent.change(screen.getByLabelText("What to do in the workspace"), { target: { value: "custom" } });
    fireEvent.change(screen.getByLabelText("Action instructions"), { target: { value: "Do it" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("at least 10 characters");
    expect(channelApi.createCustomAiChannel).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Action instructions"), { target: { value: "Create a task for each incoming support request." } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(channelApi.createCustomAiChannel).toHaveBeenCalledWith(auth, expect.objectContaining({ connection_type: "external_api", destination: "custom", instructions: "Create a task for each incoming support request." })));
  });

  it("retains edits on server rejection and preserves a saved API source when updating", async () => {
    vi.mocked(channelApi.listCustomAiChannels).mockResolvedValue([{ ...channel, source_kind: "api" }]);
    vi.mocked(channelApi.updateCustomAiChannel).mockRejectedValueOnce(new api.ApiRequestError("This agent is no longer available.", 400));
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));
    await screen.findByRole("option", { name: "Support agent" });
    fireEvent.change(screen.getByLabelText("Channel name"), { target: { value: "Revised channel" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("This agent is no longer available.");
    expect(screen.getByLabelText("Channel name")).toHaveValue("Revised channel");
    expect(api.listChannels).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await waitFor(() => expect(channelApi.updateCustomAiChannel).toHaveBeenLastCalledWith(auth, channel.id, expect.objectContaining({ name: "Revised channel", source_kind: "api", mode: "agent" })));
    await screen.findByRole("button", { name: "Add channel" });
  });

  it("deletes persisted settings only after confirmation", async () => {
    vi.mocked(channelApi.listCustomAiChannels).mockResolvedValue([channel]);
    const confirm = vi.spyOn(window, "confirm").mockReturnValueOnce(false).mockReturnValueOnce(true);
    show();
    fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(channelApi.deleteCustomAiChannel).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining(channel.name));
    await screen.findByRole("button", { name: "Add channel" });
    expect(screen.queryByText(channel.name)).not.toBeInTheDocument();
    expect(channelApi.deleteCustomAiChannel).toHaveBeenCalledWith(auth, channel.id);
    await waitFor(() => expect(api.listChannels).toHaveBeenCalledTimes(2));
  });

  it("lets read-only users inspect settings without mutation actions", async () => {
    vi.mocked(channelApi.listCustomAiChannels).mockResolvedValue([channel]);
    show(false);
    fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));
    expect(screen.getByLabelText("Channel name")).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Save settings" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Delete" })).not.toBeInTheDocument();
  });

  it("keeps the creation form and an opened channel in the page URL", async () => {
    vi.mocked(channelApi.listCustomAiChannels).mockResolvedValue([channel]);
    showAt([]);
    fireEvent.click(await screen.findByRole("button", { name: "Add channel" }));
    expect(await screen.findByLabelText("Channel name")).toHaveValue("");
    expect(pageRoute()).toBe("custom-ai/new");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    fireEvent.click(await screen.findByRole("button", { name: "Open settings" }));
    expect(await screen.findByLabelText("Channel name")).toHaveValue(channel.name);
    expect(pageRoute()).toBe(`custom-ai/${channel.id}`);
    await screen.findByRole("option", { name: "Support agent" });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    await screen.findByRole("button", { name: "Add channel" });
    expect(channelApi.updateCustomAiChannel).toHaveBeenCalledWith(auth, channel.id, expect.objectContaining({ name: channel.name }));
    expect(pageRoute()).toBe("");
  });

  it("opens the channel named by the URL once the custom channel list has loaded", async () => {
    let finishLoading!: (channels: channelApi.CustomAiChannel[]) => void;
    vi.mocked(channelApi.listCustomAiChannels).mockImplementationOnce(() => new Promise((resolve) => { finishLoading = resolve; }));
    showAt(["custom-ai", channel.id]);
    await waitFor(() => expect(api.listChannels).toHaveBeenCalled());
    expect(await screen.findByText("Loading channels…")).toBeInTheDocument();
    expect(screen.queryByLabelText("Channel name")).not.toBeInTheDocument();
    await act(async () => finishLoading([channel]));
    expect(await screen.findByLabelText("Channel name")).toHaveValue(channel.name);
    expect(pageRoute()).toBe(`custom-ai/${channel.id}`);
  });

  it("keeps the channel URL while the custom channel list cannot load and opens the channel after a retry", async () => {
    vi.mocked(channelApi.listCustomAiChannels).mockRejectedValueOnce(new Error("offline")).mockResolvedValue([channel]);
    showAt(["custom-ai", channel.id]);
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load custom channels.");
    expect(pageRoute()).toBe(`custom-ai/${channel.id}`);
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByLabelText("Channel name")).toHaveValue(channel.name);
    expect(pageRoute()).toBe(`custom-ai/${channel.id}`);
  });
});
