import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { I18nContext, createI18n } from "../i18n";
import { AISettingsView } from "./AISettingsView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";
import { usePageRoute } from "./page-route";

vi.mock("../api", () => ({ managementApiRequest: vi.fn(),
  aiTestRequest: vi.fn(),
  createAiProfile: vi.fn(),
  createAiProfileBackup: vi.fn(),
  listAiProfileBackups: vi.fn(),
  restoreAiProfileBackup: vi.fn(),
  createAiProvider: vi.fn(),
  createAiTask: vi.fn(),
  deleteAiProfile: vi.fn(),
  deleteAiProfileAvatar: vi.fn(),
  deleteAiProfilePublicIdentityAvatar: vi.fn(),
  deleteAiProvider: vi.fn(),
  deleteAiTask: vi.fn(),
  listAiChannelOptions: vi.fn(),
  listAiProfiles: vi.fn(),
  listAiProviders: vi.fn(),
  listAiTaskRuns: vi.fn(),
  listAiTasks: vi.fn(),
  listKnowledgeBases: vi.fn(),
  resolveAvatarUrl: vi.fn((value: string | null) => value),
  updateAiProfile: vi.fn(),
  updateAiProvider: vi.fn(),
  updateAiTask: vi.fn(),
  uploadAiProfileAvatar: vi.fn(),
  uploadAiProfilePublicIdentityAvatar: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const inbox: api.Inbox = {
  id: "00000000-0000-4000-8000-000000000010",
  project_id: "00000000-0000-4000-8000-000000000011",
  name: "Customer support",
  status: "active",
  created_at: "2026-08-14T00:00:00Z",
};
const channel: api.AiChannelOption = {
  id: "00000000-0000-4000-8000-000000000012",
  inbox_id: inbox.id,
  kind: "widget",
  name: "Main website",
  status: "active",
};
const provider: api.AiProvider = {
  id: "00000000-0000-4000-8000-000000000019",
  name: "OpenAI",
  provider_kind: "openai",
  base_url: "https://api.openai.com/v1",
  default_model: "gpt-5-mini",
  status: "active",
  api_key_configured: true,
  created_at: "2026-08-14T00:00:00Z",
  updated_at: "2026-08-14T00:00:00Z",
};
const anthropicProvider: api.AiProvider = {
  ...provider,
  id: "00000000-0000-4000-8000-000000000018",
  name: "Anthropic",
  provider_kind: "anthropic",
};
const profile: api.AiProfile = {
  id: "00000000-0000-4000-8000-000000000020",
  provider_connection_id: provider.id,
  name: "Support agent",
  avatar_url: null,
  status: "active",
  mode: "copilot",
  model: null,
  instructions: "Answer support questions.",
  tool_instructions: "GET https://example.com/status returns a status.",
  blacklist_reply_text: "Contact support by email.",
  blacklist_reply_match_language: true,
  http_allowed_hosts: ["example.com"],
  capabilities: {
    http_get: true,
    http_post: false,
    shell: true,
  },
  language: "en",
  max_output_tokens: 800,
  auto_join_new_conversations: false,
  can_resolve_conversations: false,
  telegram_notifications: {
    new_visitor: false,
    new_message: false,
    operator_request: true,
  },
  custom_fields: [{ key: "DB_HOST", value: "db.internal" }],
  secrets: [{ key: "DB_PASSWORD", configured: true }],
  knowledge_base_ids: [],
  channel_ids: [channel.id],
  public_identities: [{ language: "en", display_name: "Kate", avatar_url: null }],
  created_at: "2026-08-14T00:00:00Z",
  updated_at: "2026-08-14T00:00:00Z",
};
const secondProfile: api.AiProfile = {
  ...profile,
  id: "00000000-0000-4000-8000-000000000021",
  name: "Escalation agent",
  channel_ids: [],
  public_identities: [],
};
const disabledProfile: api.AiProfile = {
  ...profile,
  id: "00000000-0000-4000-8000-000000000022",
  name: "Legacy agent",
  status: "disabled",
  channel_ids: [],
  public_identities: [],
};
const anthropicProfile: api.AiProfile = {
  ...profile,
  id: "00000000-0000-4000-8000-000000000023",
  name: "Anthropic agent",
  provider_connection_id: anthropicProvider.id,
  channel_ids: [],
  public_identities: [],
};

/** Changes the URL from outside the page, as a link or browser Back/Forward does. */
function RouteLink({ segments }: { segments: string[] }) {
  const route = usePageRoute();
  return <button type="button" onClick={() => void route.navigate(segments)}>{`Open /${segments.join("/")}`}</button>;
}

function showRouted(segments: string[], links: string[][] = []) {
  return render(<I18nContext.Provider value={createI18n("en", vi.fn())}><MemoryPageRoute initialSegments={segments}>
    <AISettingsView auth={auth} inboxes={[inbox]} /><CurrentPageRoute />{links.map((link) => <RouteLink key={link.join("/")} segments={link} />)}
  </MemoryPageRoute></I18nContext.Provider>);
}

const pageRoute = () => screen.getByTestId("page-route").textContent;

describe("AISettingsView", () => {
  let savedProfile: api.AiProfile;

  beforeEach(() => {
    vi.mocked(api.managementApiRequest).mockResolvedValue({ projects: [], departments: [], items: [] });
    savedProfile = profile;
    vi.mocked(api.aiTestRequest).mockResolvedValue({ items: [] });
    vi.mocked(api.createAiProfile).mockResolvedValue(profile);
    vi.mocked(api.listAiProfiles).mockImplementation(async () => [savedProfile, secondProfile, disabledProfile, anthropicProfile]);
    vi.mocked(api.listAiProviders).mockResolvedValue([provider, anthropicProvider]);
    vi.mocked(api.listAiChannelOptions).mockResolvedValue([channel]);
    vi.mocked(api.listKnowledgeBases).mockResolvedValue([]);
    vi.mocked(api.updateAiProfile).mockImplementation(async (_auth, _profileId, input) => {
      savedProfile = {
        ...savedProfile,
        ...input,
        public_identities: input.public_identities.map((identity) => ({
          ...identity,
          avatar_url: savedProfile.public_identities.find(
            (current) => current.language === identity.language,
          )?.avatar_url ?? null,
        })),
        secrets: input.secrets.map((secret) => ({ key: secret.key, configured: true })),
      };
      return savedProfile;
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("saves the agent concurrency limit and restores it when reopening settings", async () => {
    showRouted(["agents", profile.id]);
    const input = await screen.findByRole("spinbutton", { name: "Parallel runs" });
    expect(input).toHaveValue(2);
    fireEvent.change(input, { target: { value: "3" } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(auth, profile.id,
      expect.objectContaining({ max_concurrent_runs: 3 })));
    fireEvent.click(screen.getByRole("button", { name: /Escalation agent/ }));
    expect(screen.getByRole("spinbutton", { name: "Parallel runs" })).toHaveValue(2);
    fireEvent.click(screen.getByRole("button", { name: /Support agent/ }));
    expect(screen.getByRole("spinbutton", { name: "Parallel runs" })).toHaveValue(3);
    fireEvent.change(screen.getByRole("spinbutton", { name: "Parallel runs" }), { target: { value: "0" } });
    expect(screen.getByRole("spinbutton", { name: "Parallel runs" })).toBeInvalid();
    fireEvent.change(screen.getByRole("spinbutton", { name: "Parallel runs" }), { target: { value: "17" } });
    expect(screen.getByRole("spinbutton", { name: "Parallel runs" })).toBeInvalid();
  });

  it("opens the requested accessible agent from the company org chart link", async () => {
    showRouted(["agents", secondProfile.id]);
    expect(await screen.findByRole("heading", { name: "Escalation agent" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Support agent" })).not.toBeInTheDocument();
    expect(screen.getByRole("tablist", { name: "Agent sections" })).toBeInTheDocument();
    expect(pageRoute()).toBe(`agents/${secondProfile.id}`);
  });

  it("does not select an arbitrary agent when the requested agent is unavailable", async () => {
    showRouted(["agents", "unavailable-agent"]);
    await screen.findByRole("button", { name: /Support agent/ });
    await waitFor(() => expect(pageRoute()).toBe(""));
    expect(screen.queryByRole("heading", { name: "Support agent" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Escalation agent" })).not.toBeInTheDocument();
    expect(screen.queryByRole("tablist", { name: "Agent sections" })).not.toBeInTheDocument();
    expect(api.updateAiProfile).not.toHaveBeenCalled();
  });

  it("opens the agent section named by the page URL and keeps the URL current", async () => {
    showRouted(["agents", secondProfile.id, "instructions"]);
    expect(await screen.findByRole("textbox", { name: "Agent instructions" })).toHaveValue(secondProfile.instructions);
    expect(screen.getByRole("tab", { name: "Instructions", selected: true })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    expect(pageRoute()).toBe(`agents/${secondProfile.id}/knowledge`);
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(pageRoute()).toBe(`agents/${secondProfile.id}`);
    fireEvent.click(screen.getByRole("button", { name: /Support agent/ }));
    expect(screen.getByRole("heading", { name: "Support agent" })).toBeInTheDocument();
    expect(pageRoute()).toBe(`agents/${profile.id}`);
    fireEvent.click(screen.getByRole("tab", { name: "Tests" }));
    expect(pageRoute()).toBe(`agents/${profile.id}/tests`);
    fireEvent.click(screen.getByRole("button", { name: "Create new agent" }));
    expect(screen.getByRole("heading", { name: "Create agent" })).toBeInTheDocument();
    expect(pageRoute()).toBe("agents/new");
    fireEvent.click(screen.getByRole("button", { name: /Escalation agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Agent instructions" }), { target: { value: "Unsaved instructions" } });

    fireEvent.click(screen.getByRole("tab", { name: "Model connections" }));
    expect(pageRoute()).toBe("providers");
    fireEvent.click(within(screen.getByRole("complementary", { name: "Model connections" })).getByRole("button", { name: /Anthropic/ }));
    expect(pageRoute()).toBe(`providers/${anthropicProvider.id}`);
    fireEvent.click(screen.getByRole("tab", { name: "Agents" }));
    expect(pageRoute()).toBe(`agents/${secondProfile.id}/instructions`);
    expect(screen.getByRole("textbox", { name: "Agent instructions" })).toHaveValue("Unsaved instructions");
    fireEvent.click(screen.getByRole("tab", { name: "Model connections" }));
    expect(pageRoute()).toBe(`providers/${anthropicProvider.id}`);
    expect(screen.getByLabelText("Connection name")).toHaveValue("Anthropic");
    fireEvent.click(screen.getByRole("button", { name: "New connection" }));
    expect(pageRoute()).toBe("providers/new");
    expect(screen.getByRole("heading", { name: "Create model connection" })).toBeInTheDocument();
  });

  it("follows links and history to other agents, new drafts and the agent list", async () => {
    showRouted([], [["agents", profile.id], ["agents", secondProfile.id, "instructions"], ["agents", "new"], []]);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Internal agent name" }), { target: { value: "Unsaved name" } });
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    fireEvent.click(screen.getByRole("button", { name: `Open /agents/${secondProfile.id}/instructions` }));
    expect(screen.getByRole("heading", { name: "Escalation agent" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Instructions", selected: true })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: `Open /agents/${profile.id}` }));
    expect(screen.getByRole("textbox", { name: "Internal agent name" })).toHaveValue("Support agent");
    fireEvent.click(screen.getByRole("button", { name: "Open /agents/new" }));
    expect(screen.getByRole("heading", { name: "Create agent" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Internal agent name" })).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: "Open /" }));
    expect(screen.getByText("Select an agent or create a new one.")).toBeInTheDocument();
    expect(screen.queryByRole("tablist", { name: "Agent sections" })).not.toBeInTheDocument();
    expect(api.updateAiProfile).not.toHaveBeenCalled();
  });

  it("replaces a new agent URL with the saved agent", async () => {
    showRouted(["agents", "new", "instructions"]);
    fireEvent.change(await screen.findByRole("textbox", { name: "Agent instructions" }), { target: { value: "Help customers." } });
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(pageRoute()).toBe("agents/new");
    fireEvent.change(screen.getByRole("textbox", { name: "Internal agent name" }), { target: { value: "Routed agent" } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(pageRoute()).toBe(`agents/${profile.id}`));
    expect(api.createAiProfile).toHaveBeenCalledWith(auth, expect.objectContaining({ name: "Routed agent", instructions: "Help customers." }));
    expect(screen.getByRole("status")).toHaveTextContent("Agent settings saved.");
    expect(screen.getByRole("heading", { name: "Support agent" })).toBeInTheDocument();
  });

  it("opens a model connection named by the URL and replaces a new connection URL once saved", async () => {
    vi.mocked(api.createAiProvider).mockResolvedValue(provider);
    showRouted(["providers", anthropicProvider.id]);
    expect(await screen.findByLabelText("Connection name")).toHaveValue("Anthropic");
    expect(screen.getByRole("tab", { name: "Model connections", selected: true })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add connection" }));
    expect(pageRoute()).toBe("providers/new");
    fireEvent.change(screen.getByLabelText("Connection name"), { target: { value: "OpenAI" } });
    fireEvent.change(screen.getByLabelText("API base URL"), { target: { value: provider.base_url } });
    fireEvent.change(screen.getByLabelText("Default model"), { target: { value: provider.default_model } });
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }));
    await waitFor(() => expect(pageRoute()).toBe(`providers/${provider.id}`));
    expect(screen.getByRole("status")).toHaveTextContent("Model connection saved.");
  });

  it("opens the skill library and the skill named by the URL", async () => {
    vi.mocked(api.managementApiRequest).mockImplementation(async (_auth, path) => path === "/api/v1/ai/skills"
      ? { items: [{ id: "invoice-skill", name: "Review invoices", description: "", instructions: "Check totals.", ai_profile_ids: [] }] }
      : { projects: [], departments: [], items: [] });
    showRouted(["skills", "invoice-skill"]);
    expect(await screen.findByRole("textbox", { name: "Skill name" })).toHaveValue("Review invoices");
    expect(screen.getByRole("tab", { name: "Skills", selected: true })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Agents" }));
    expect(pageRoute()).toBe("");
    fireEvent.click(screen.getByRole("tab", { name: "Skills" }));
    expect(pageRoute()).toBe("skills/invoice-skill");
    fireEvent.click(screen.getByRole("button", { name: "Create skill" }));
    expect(pageRoute()).toBe("skills/new");
  });

  it.each([
    [["agents"], ""],
    [["unknown"], ""],
    [["agents", profile.id, "settings"], `agents/${profile.id}`],
    [["agents", profile.id, "unknown"], `agents/${profile.id}`],
    [["agents", profile.id, "tests", "extra"], `agents/${profile.id}/tests`],
    [["agents", "new", "tests"], "agents/new"],
    [["providers", "missing"], "providers"],
    [["providers", provider.id, "extra"], `providers/${provider.id}`],
    [["skills", "missing"], "skills"],
  ])("replaces the stale AI URL %j with the page it shows", async (segments, canonical) => {
    showRouted(segments);
    await waitFor(() => expect(pageRoute()).toBe(canonical));
  });

  it("loads skills only when opened and retains a skill draft across settings tabs", async () => {
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    await screen.findByRole("button", { name: /Support agent/ });
    expect(api.managementApiRequest).not.toHaveBeenCalledWith(auth, "/api/v1/ai/skills", expect.anything());
    vi.mocked(api.managementApiRequest).mockResolvedValueOnce({ items: [] });
    fireEvent.click(screen.getByRole("tab", { name: "Skills" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh skills" })).toBeEnabled());
    expect(screen.getByRole("tabpanel", { name: "Skills" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New model connection" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create skill" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Skill name" }), { target: { value: "My skill draft" } });
    fireEvent.click(screen.getByRole("tab", { name: "Agents" }));
    expect(screen.queryByRole("textbox", { name: "Skill name" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Skills" }));
    expect(screen.getByRole("textbox", { name: "Skill name" })).toHaveValue("My skill draft");
    expect(vi.mocked(api.managementApiRequest).mock.calls.filter((call) => call[1] === "/api/v1/ai/skills")).toHaveLength(1);
  });

  it("uses project model connections for skill creation and locks tabs until generation is cancelled", async () => {
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    await screen.findByRole("button", { name: /Support agent/ });
    vi.mocked(api.managementApiRequest).mockResolvedValueOnce({ items: [] });
    fireEvent.click(screen.getByRole("tab", { name: "Skills" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh skills" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Create with AI" }));
    expect(screen.getByRole("combobox", { name: "Model connection" })).toHaveValue(provider.id);
    fireEvent.change(screen.getByRole("textbox", { name: "What should the skill do?" }), { target: { value: "Review a handbook" } });
    const book = new File(["handbook"], "handbook.pdf");
    fireEvent.change(screen.getByLabelText("Source files", { selector: "input" }), { target: { files: [book] } });
    fireEvent.click(screen.getByRole("tab", { name: "Agents" }));
    fireEvent.click(screen.getByRole("tab", { name: "Skills" }));
    expect(screen.getByText("handbook.pdf")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "What should the skill do?" })).toHaveValue("Review a handbook");
    vi.mocked(api.managementApiRequest).mockImplementationOnce(() => new Promise(() => {}));
    fireEvent.click(screen.getByRole("button", { name: "Generate instructions" }));
    expect(screen.getByRole("tab", { name: "Agents" })).toBeDisabled();
    expect(screen.getByRole("tab", { name: "Model connections" })).toBeDisabled();
    const generation = vi.mocked(api.managementApiRequest).mock.calls.find((call) => call[1] === "/api/v1/ai/skills/generate");
    expect((generation?.[2]?.body as FormData).get("provider_connection_id")).toBe(provider.id);
    fireEvent.click(screen.getByRole("button", { name: "Cancel generation" }));
    expect(generation?.[2]?.signal?.aborted).toBe(true);
    expect(screen.getByRole("tab", { name: "Agents" })).toBeEnabled();
    expect(screen.getByRole("tab", { name: "Model connections" })).toBeEnabled();
    expect(screen.getByText("handbook.pdf")).toBeInTheDocument();
  });

  it("offers agent API settings only with full project access", async () => {
    const rendered = render(<I18nContext.Provider value={createI18n("en", vi.fn())}>
      <AISettingsView auth={auth} inboxes={[inbox]} canManageApi={false} />
    </I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    expect(screen.queryByRole("button", { name: "API settings" })).not.toBeInTheDocument();
    rendered.rerender(<I18nContext.Provider value={createI18n("en", vi.fn())}>
      <AISettingsView auth={auth} inboxes={[inbox]} canManageApi />
    </I18nContext.Provider>);
    expect(screen.getByRole("button", { name: "API settings" })).toBeInTheDocument();
  });

  it("keeps the shared draft across four accessible tabs and a failed save", async () => {
    vi.mocked(api.updateAiProfile).mockRejectedValueOnce(new Error("Temporary save failure"));
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    const settingsTab = screen.getByRole("tab", { name: "Settings" });
    const profileTabs = settingsTab.closest("nav")!;
    expect(within(profileTabs).getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["Settings", "Instructions", "Knowledge and tools", "Tests"]);
    expect(screen.queryByRole("textbox", { name: "Agent instructions" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Purpose" }), { target: { value: "My agent purpose" } });
    fireEvent.keyDown(settingsTab, { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: "Instructions", selected: true })).toHaveFocus();
    fireEvent.change(screen.getByRole("textbox", { name: "Agent instructions" }), { target: { value: "My instructions" } });
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Tool descriptions" }), { target: { value: "My tool descriptions" } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save the agent.");
    expect(screen.getByRole("textbox", { name: "Tool descriptions" })).toHaveValue("My tool descriptions");
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("textbox", { name: "Purpose" })).toHaveValue("My agent purpose");
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    expect(screen.getByRole("textbox", { name: "Agent instructions" })).toHaveValue("My instructions");
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledTimes(2));
    expect(api.updateAiProfile).toHaveBeenLastCalledWith(auth, profile.id, expect.objectContaining({ description: "My agent purpose", instructions: "My instructions", tool_instructions: "My tool descriptions" }));
  });

  it.each([false, true])("edits a used instruction inline and preserves unsaved settings (instructions dirty: %s)", async (instructionsDirty) => {
    const run = { id: "passed-test", status: "passed", created_at: "2026-09-09T10:00:00Z", snapshot: { scenario: { name: "Контакт" }, profile: { instructions: "Instructions at the time of the test.", tool_instructions: profile.tool_instructions } }, result: { trace: [] } };
    vi.mocked(api.aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (path.endsWith("/recommendations")) return {
        summary: "Уточните условия", stale: false,
        recommendations: [{ target: "agent_instructions", reason: "Проверка ID описана недостаточно точно", suggested_text: "Предлагаемая проверка contact_id" }],
      };
      if (path === "test-runs/passed-test/sources/instructions") return options?.method === "PATCH"
        ? { text: "Reviewed saved instructions", revision: "revision-2" }
        : { text: profile.instructions, revision: "revision-1" };
      if (path === "test-runs/passed-test") return run;
      return { items: path === "test-runs" ? [run] : [] };
    });
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Internal agent name" }), { target: { value: "Unsaved name" } });
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    if (instructionsDirty) fireEvent.change(screen.getByRole("textbox", { name: "Agent instructions" }), { target: { value: "Unsaved instructions" } });
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Tool descriptions" }), { target: { value: "Unsaved tools" } });
    fireEvent.click(screen.getByRole("tab", { name: "Tests" }));
    fireEvent.click((await screen.findByRole("img", { name: "Пройдено" })).closest("summary")!);
    expect(vi.mocked(api.aiTestRequest).mock.calls.some((call) => call[2].includes("passed-test"))).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Рекомендации по инструкциям" }));
    fireEvent.click(await screen.findByRole("button", { name: "Редактировать источник" }));
    const sourceEditor = await screen.findByRole("textbox", { name: "Текст для следующих запусков" });
    expect(sourceEditor).toHaveValue(profile.instructions);
    expect(screen.getByRole("region", { name: "Текст в запуске" })).toHaveTextContent("Instructions at the time of the test.");
    expect(screen.getByRole("tab", { name: "Tests", selected: true })).toBeInTheDocument();
    fireEvent.change(sourceEditor, { target: { value: "Reviewed saved instructions" } });
    fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
    await screen.findByText("Изменения сохранены. Повторите тест, чтобы проверить новую версию.");
    expect(api.aiTestRequest).toHaveBeenCalledWith(auth, profile.id, "test-runs/passed-test/sources/instructions", {
      method: "PATCH", signal: expect.any(AbortSignal), body: JSON.stringify({ text: "Reviewed saved instructions", expected_revision: "revision-1" }),
    });
    expect(screen.getByRole("region", { name: "Текст в запуске" })).toHaveTextContent("Instructions at the time of the test.");
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("textbox", { name: "Internal agent name" })).toHaveValue("Unsaved name");
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    expect(screen.getByRole("textbox", { name: "Agent instructions" })).toHaveValue(instructionsDirty ? "Unsaved instructions" : "Reviewed saved instructions");
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    expect(screen.getByRole("textbox", { name: "Tool descriptions" })).toHaveValue("Unsaved tools");
    expect(api.updateAiProfile).not.toHaveBeenCalled();
    expect(vi.mocked(api.aiTestRequest).mock.calls.filter((call) => call[3]?.method).map((call) => call[3]?.method)).toEqual(["POST", "PATCH"]);
  });

  it("blocks conflicting profile actions during a source save while allowing a switch to Settings", async () => {
    const run = { id: "pending-source", status: "passed", created_at: "2026-09-09T10:00:00Z", snapshot: { scenario: { name: "Проверка инструкции" }, profile: { instructions: profile.instructions } }, result: { trace: [] } };
    let finishSave!: (value: unknown) => void;
    vi.mocked(api.aiTestRequest).mockImplementation(async (_auth, _profile, path, options) => {
      if (path === "test-runs/pending-source/sources/instructions") return options?.method === "PATCH"
        ? new Promise((resolve) => { finishSave = resolve; })
        : { text: profile.instructions, revision: "revision-before" };
      if (path === "test-runs/pending-source") return run;
      return { items: path === "test-runs" ? [run] : [] };
    });
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Internal agent name" }), { target: { value: "Unsaved agent name" } });
    fireEvent.click(screen.getByRole("tab", { name: "Tests" }));
    fireEvent.click((await screen.findByRole("img", { name: "Пройдено" })).closest("summary")!);
    fireEvent.click(screen.getByRole("button", { name: "Использованные инструкции" }));
    fireEvent.change(await screen.findByRole("textbox", { name: "Текст для следующих запусков" }), { target: { value: "Saved without racing profile updates" } });
    fireEvent.click(screen.getByRole("button", { name: "Сохранить изменения" }));
    expect(await screen.findByRole("button", { name: "Сохраняем…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "New agent" })).toBeDisabled();
    expect(screen.getByRole("tab", { name: "Model connections" })).toBeDisabled();
    expect(screen.getByRole("button", { name: new RegExp(secondProfile.name) })).toBeDisabled();
    expect(screen.getByRole("tab", { name: "Settings" })).toBeEnabled();
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("tab", { name: "Settings", selected: true })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save agent" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete agent" })).toBeDisabled();
    expect(screen.getByRole("textbox", { name: "Internal agent name" })).toHaveValue("Unsaved agent name");
    expect(api.updateAiProfile).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    expect(screen.getByRole("textbox", { name: "Agent instructions" })).toBeDisabled();
    await act(async () => finishSave({ text: "Saved without racing profile updates", revision: "revision-after" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Save agent" })).toBeEnabled());
    expect(screen.getByRole("button", { name: "New agent" })).toBeEnabled();
    expect(screen.getByRole("tab", { name: "Model connections" })).toBeEnabled();
    expect(screen.getByRole("button", { name: new RegExp(secondProfile.name) })).toBeEnabled();
    expect(screen.getByRole("textbox", { name: "Agent instructions" })).toHaveValue("Saved without racing profile updates");
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("textbox", { name: "Internal agent name" })).toHaveValue("Unsaved agent name");
    expect(api.updateAiProfile).not.toHaveBeenCalled();
  });

  it.each(["saved", "failed"] as const)("keeps skill assignments mounted until the request is %s", async (outcome) => {
    let finishSave!: () => void;
    vi.mocked(api.managementApiRequest).mockImplementation(async (_auth, path) => {
      if (path === "/api/v1/ai/skills") return { items: [{ id: "invoice-skill", name: "Review invoices", description: "", instructions: "Check totals.", ai_profile_ids: [] }] };
      if (path === `/api/v1/ai/profiles/${profile.id}/skills`) return new Promise((resolve, reject) => {
        finishSave = () => outcome === "saved"
          ? resolve({ skill_ids: ["invoice-skill"] })
          : reject(new Error("Temporary assignment failure"));
      });
      return { projects: [], departments: [], items: [] };
    });
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Internal agent name" }), { target: { value: "Unsaved agent name" } });
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "Review invoices" }));

    expect(screen.getByRole("checkbox", { name: "Review invoices" })).toBeDisabled();
    for (const tab of screen.getAllByRole("tab")) expect(tab).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save agent" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Delete agent" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Escalation agent/ })).toBeDisabled();
    expect(api.managementApiRequest).toHaveBeenCalledWith(auth, `/api/v1/ai/profiles/${profile.id}/skills`, {
      method: "PUT", body: JSON.stringify({ skill_ids: ["invoice-skill"] }),
    });
    await act(async () => finishSave());

    expect(screen.getByRole("checkbox", { name: "Review invoices" })).toBeEnabled();
    if (outcome === "saved") expect(screen.getByRole("checkbox", { name: "Review invoices" })).toBeChecked();
    else expect(screen.getByRole("alert")).toHaveTextContent("Could not save");
    for (const tab of screen.getAllByRole("tab")) expect(tab).toBeEnabled();
    expect(screen.getByRole("button", { name: "Save agent" })).toBeEnabled();
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("textbox", { name: "Internal agent name" })).toHaveValue("Unsaved agent name");
    expect(api.updateAiProfile).not.toHaveBeenCalled();
  });

  it("restores instructions and knowledge while preserving other unsaved agent fields", async () => {
    const restored = { ...profile, instructions: "Restored instructions", blacklist_reply_text: "Restored blacklist reply", blacklist_reply_match_language: false, knowledge_base_ids: ["restored-base"] };
    vi.mocked(api.listAiProfileBackups).mockResolvedValue([{ id: "backup", reason: "manual", created_at: "2026-09-05T12:00:00Z", knowledge_base_count: 1, article_count: 1 }]);
    vi.mocked(api.restoreAiProfileBackup).mockResolvedValue(restored);
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Internal agent name" }), { target: { value: "Unsaved agent name" } });
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Agent instructions" }), { target: { value: "Unsaved instructions" } });
    fireEvent.change(screen.getByRole("textbox", { name: "Text when added to the blacklist" }), { target: { value: "Unsaved blacklist reply" } });
    fireEvent.click(screen.getByRole("button", { name: "Backup" }));
    await waitFor(() => expect(screen.getByRole("button", { name: /Restore backup from/ })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: /Restore backup from/ }));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Agent instructions" })).toHaveValue("Restored instructions"));
    expect(screen.getByRole("textbox", { name: "Text when added to the blacklist" })).toHaveValue("Restored blacklist reply");
    expect(screen.getByRole("checkbox", { name: "Match the message language" })).not.toBeChecked();
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    expect(screen.getByRole("textbox", { name: "Internal agent name" })).toHaveValue("Unsaved agent name");
    await waitFor(() => expect(screen.getByRole("button", { name: "Save agent" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(auth, profile.id, expect.objectContaining({ name: "Unsaved agent name", instructions: "Restored instructions", blacklist_reply_text: "Restored blacklist reply", blacklist_reply_match_language: false, knowledge_base_ids: ["restored-base"] })));
    confirm.mockRestore();
  });

  it("keeps operator handoff wording inside the agent instructions", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));

    expect(screen.getByRole("textbox", { name: "Agent instructions" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Operator handoff message" })).not.toBeInTheDocument();
  });

  it("saves the blacklist text and keeps language matching disabled after reopening", async () => {
    render(<I18nContext.Provider value={createI18n("ru", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Инструкции" }));
    const text = screen.getByRole("textbox", { name: "Текст при добавлении в ЧС" });
    const matchLanguage = screen.getByRole("checkbox", { name: "Учитывать язык сообщения" });
    expect(text).toHaveValue(profile.blacklist_reply_text);
    expect(text).toHaveAttribute("maxlength", "4000");
    expect(matchLanguage).toBeChecked();
    const customText = "  Вы заблокированы из-за спама.\nСвяжитесь по почте на сайте.  ";
    fireEvent.change(text, { target: { value: customText } });
    fireEvent.click(matchLanguage);
    fireEvent.click(screen.getByRole("button", { name: "Сохранить агента" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(auth, profile.id, expect.objectContaining({
      blacklist_reply_text: customText,
      blacklist_reply_match_language: false,
      instructions: profile.instructions,
    })));
    await waitFor(() => expect(screen.getByRole("button", { name: "Сохранить агента" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: /Escalation agent/ }));
    fireEvent.click(screen.getByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Инструкции" }));
    expect(screen.getByRole("textbox", { name: "Текст при добавлении в ЧС" })).toHaveValue(customText);
    expect(screen.getByRole("checkbox", { name: "Учитывать язык сообщения" })).not.toBeChecked();
  });

  it("uses channel fallback and enables language matching for legacy profile responses", async () => {
    const legacyProfile: Partial<api.AiProfile> = { ...profile };
    delete legacyProfile.blacklist_reply_text;
    delete legacyProfile.blacklist_reply_match_language;
    savedProfile = legacyProfile as api.AiProfile;
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    expect(screen.getByRole("textbox", { name: "Text when added to the blacklist" })).toHaveValue("");
    expect(screen.getByRole("checkbox", { name: "Match the message language" })).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(auth, profile.id, expect.objectContaining({
      blacklist_reply_text: "",
      blacklist_reply_match_language: true,
    })));
  });

  it.each(["text", "language"])("requires saving the changed blacklist %s setting before backup", async (field) => {
    vi.mocked(api.listAiProfileBackups).mockResolvedValue([]);
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    if (field === "text") {
      fireEvent.change(screen.getByRole("textbox", { name: "Text when added to the blacklist" }), { target: { value: "Updated blacklist reply" } });
    } else {
      fireEvent.click(screen.getByRole("checkbox", { name: "Match the message language" }));
    }
    fireEvent.click(screen.getByRole("button", { name: "Backup" }));
    expect(await screen.findByRole("button", { name: "Create backup" })).toBeDisabled();
    expect(screen.getByText(/Save changes to instructions, tool descriptions, blacklist reply settings/)).toBeInTheDocument();
  });

  it("keeps proxy enabled while editing the agent and saves proxy settings separately", async () => {
    vi.mocked(api.managementApiRequest).mockImplementation(async (_auth, path, init) => {
      if (path.endsWith("/proxy")) {
        return init?.method === "PUT"
          ? { ...JSON.parse(String(init.body)), token_configured: true }
          : { enabled: false, service_url: "", region: null, country: null, token_configured: false };
      }
      return { projects: [], departments: [], items: [] };
    });
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    const enabled = await screen.findByRole("checkbox", { name: "Connect a proxy" });
    fireEvent.click(enabled);
    expect(enabled).toBeChecked();
    fireEvent.change(screen.getByRole("textbox", { name: /Allowed domains/ }), { target: { value: "xe.com\nwww.xe.com" } });
    expect(enabled).toBeChecked();
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    expect(screen.getAllByRole("group", { name: "Skills" })).toHaveLength(1);
    expect(screen.getAllByRole("group", { name: "Proxy" })).toHaveLength(1);
    expect(enabled).toBeChecked();
    fireEvent.change(screen.getByLabelText("Proxy discovery URL", { exact: false }), { target: { value: "https://proxy.example/api/proxies/random" } });
    fireEvent.change(screen.getByLabelText("Bearer token", { exact: false }), { target: { value: "test-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Save proxy" }));
    await screen.findByText("Proxy settings saved");
    expect(enabled).toBeChecked();
    expect(api.managementApiRequest).toHaveBeenCalledWith(auth, `/api/v1/ai/profiles/${profile.id}/proxy`, {
      method: "PUT", body: JSON.stringify({ enabled: true, service_url: "https://proxy.example/api/proxies/random", region: null, country: null, token: "test-token", clear_token: false }),
    });
    expect(api.updateAiProfile).not.toHaveBeenCalled();
  });

  it("shows one working proxy form after switching agents from knowledge and tools", async () => {
    vi.mocked(api.managementApiRequest).mockImplementation(async (_auth, path) => path.endsWith("/proxy")
      ? { enabled: false, service_url: "", region: null, country: null, token_configured: false }
      : { projects: [], departments: [], items: [] });
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    await screen.findByRole("button", { name: /Support agent/ });
    for (const name of [/Support agent/, /Escalation agent/, /Support agent/]) {
      fireEvent.click(screen.getByRole("button", { name }));
      fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
      await screen.findByRole("button", { name: "Save proxy" });
      expect(screen.getAllByRole("group", { name: "Proxy" })).toHaveLength(1);
      const enabled = screen.getByRole("checkbox", { name: "Connect a proxy" });
      expect(enabled).not.toBeChecked();
      fireEvent.click(enabled);
      expect(enabled).toBeChecked();
    }
  });

  it("loads and saves independent agent capability switches", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    const httpGet = screen.getByRole("checkbox", { name: /^HTTP GET/ });
    const httpPost = screen.getByRole("checkbox", { name: /^HTTP POST/ });
    const shell = screen.getByRole("checkbox", { name: /^Shell/ });
    expect(httpGet).toBeChecked();
    expect(httpPost).not.toBeChecked();
    expect(shell).toBeChecked();
    expect(shell).toBeEnabled();
    expect(screen.getByText(/headless browser page reading/)).toHaveTextContent("allowed HTTPS domains");
    expect(screen.getByText(/HTTPS domains allowed in this agent’s settings/)).toBeInTheDocument();
    expect(screen.getByText(/POST can have external side effects/)).toBeInTheDocument();
    expect(screen.getByText(/isolated runner without network access and with a temporary filesystem/)).toHaveTextContent(
      "HTTP access is controlled separately.",
    );

    fireEvent.click(httpGet);
    fireEvent.click(httpPost);
    fireEvent.click(shell);
    expect(httpGet).not.toBeChecked();
    expect(httpPost).toBeChecked();
    expect(shell).not.toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth,
      profile.id,
      expect.objectContaining({
        capabilities: {
          http_get: false,
          http_post: true,
          shell: false,
        },
      }),
    ));
  });

  it("defaults every capability to off for a new agent", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "New agent" }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    const httpGet = screen.getByRole("checkbox", { name: /^HTTP GET/ });
    const httpPost = screen.getByRole("checkbox", { name: /^HTTP POST/ });
    const shell = screen.getByRole("checkbox", { name: /^Shell/ });
    expect(httpGet).not.toBeChecked();
    expect(httpPost).not.toBeChecked();
    expect(shell).not.toBeChecked();

    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    expect(screen.getByRole("textbox", { name: "Text when added to the blacklist" })).toHaveValue("");
    expect(screen.getByRole("checkbox", { name: "Match the message language" })).toBeChecked();
    fireEvent.click(screen.getByRole("tab", { name: "Settings" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Internal agent name" }), {
      target: { value: "Restricted agent" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.createAiProfile).toHaveBeenCalledWith(
      auth,
      expect.objectContaining({
        blacklist_reply_text: "",
        blacklist_reply_match_language: true,
        capabilities: {
          http_get: false,
          http_post: false,
          shell: false,
        },
      }),
    ));
  });

  it.each(["new", "existing"] as const)("guides a %s active agent to missing fields before saving", async (kind) => {
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><AISettingsView auth={auth} inboxes={[inbox]} /></I18nContext.Provider>);
    fireEvent.click(await screen.findByRole("button", { name: kind === "new" ? "New agent" : /Support agent/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Internal agent name" }), { target: { value: "Active support agent" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Status" }), { target: { value: "active" } });
    fireEvent.change(screen.getByRole("combobox", { name: "Model connection" }), { target: { value: "" } });
    fireEvent.click(screen.getByRole("tab", { name: "Instructions" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Agent instructions" }), { target: { value: " \n\t " } });
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    const connection = await screen.findByRole("combobox", { name: "Model connection" });
    expect(screen.getByRole("tab", { name: "Settings", selected: true })).toBeInTheDocument();
    await waitFor(() => expect(connection).toHaveFocus());
    expect(connection).toHaveAttribute("aria-invalid", "true");
    expect(connection).toHaveAccessibleDescription("Select a model connection to activate the agent, or save it as a draft.");
    expect(api.createAiProfile).not.toHaveBeenCalled();
    expect(api.updateAiProfile).not.toHaveBeenCalled();

    fireEvent.change(connection, { target: { value: provider.id } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    const instructions = await screen.findByRole("textbox", { name: "Agent instructions" });
    expect(screen.getByRole("tab", { name: "Instructions", selected: true })).toBeInTheDocument();
    await waitFor(() => expect(instructions).toHaveFocus());
    expect(instructions).toHaveAttribute("aria-invalid", "true");
    expect(instructions).toHaveAccessibleDescription("Add instructions to activate the agent, or save it as a draft.");
    expect(api.createAiProfile).not.toHaveBeenCalled();
    expect(api.updateAiProfile).not.toHaveBeenCalled();

    fireEvent.change(instructions, { target: { value: "Answer support questions." } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    const input = expect.objectContaining({ name: "Active support agent", status: "active", provider_connection_id: provider.id, instructions: "Answer support questions." });
    await waitFor(() => {
      if (kind === "new") expect(api.createAiProfile).toHaveBeenCalledWith(auth, input);
      else expect(api.updateAiProfile).toHaveBeenCalledWith(auth, profile.id, input);
    });
    expect(await screen.findByRole("status")).toHaveTextContent("Agent settings saved.");
  });

  it("saves automatic joining for every new conversation", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    const autoJoin = screen.getByRole("checkbox", {
      name: /Automatically take every new conversation/,
    });
    fireEvent.click(autoJoin);
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth,
      profile.id,
      expect.objectContaining({ auto_join_new_conversations: true }),
    ));
    expect(autoJoin).toBeChecked();
  });

  it("saves tool descriptions separately from behavior and permissions", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    const tools = screen.getByRole("textbox", { name: "Tool descriptions" });
    expect(tools).toHaveValue(profile.tool_instructions);
    fireEvent.change(tools, { target: { value: "Updated API description" } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth, profile.id, expect.objectContaining({
        instructions: profile.instructions,
        tool_instructions: "Updated API description",
        capabilities: profile.capabilities,
      }),
    ));
  });

  it("saves agent-specific HTTP domains without changing tool instructions", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    const domains = screen.getByRole("textbox", { name: /^Allowed domains/ });
    expect(domains).toHaveValue("example.com");
    fireEvent.change(domains, { target: { value: "api.example.org\n\n support.example.net " } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth, profile.id, expect.objectContaining({
        http_allowed_hosts: ["api.example.org", "support.example.net"],
        tool_instructions: profile.tool_instructions,
        capabilities: profile.capabilities,
      }),
    ));
  });

  it("saves public HTTPS access as an explicit agent choice", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    fireEvent.change(screen.getByRole("combobox", { name: /^HTTP access/ }), { target: { value: "public" } });
    expect(screen.queryByRole("textbox", { name: /^Allowed domains/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth, profile.id, expect.objectContaining({ http_allowed_hosts: ["*"] }),
    ));
  });

  it("loads and saves whether the agent may resolve conversations", async () => {
    savedProfile = { ...profile, can_resolve_conversations: true };
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    const canResolve = screen.getByRole("checkbox", {
      name: /Allow the agent to resolve conversations/,
    });
    expect(canResolve).toBeChecked();

    fireEvent.click(canResolve);
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth,
      profile.id,
      expect.objectContaining({ can_resolve_conversations: false }),
    ));
    expect(canResolve).not.toBeChecked();
  });

  it("saves independent Telegram notification event switches", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    const visitor = screen.getByRole("checkbox", { name: /New website visitor/ });
    const message = screen.getByRole("checkbox", { name: /New customer message/ });
    const handoff = screen.getByRole("checkbox", { name: /Operator request/ });
    expect(visitor).not.toBeChecked();
    expect(message).not.toBeChecked();
    expect(handoff).toBeChecked();

    fireEvent.click(visitor);
    fireEvent.click(message);
    fireEvent.click(handoff);
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth,
      profile.id,
      expect.objectContaining({
        telegram_notifications: {
          new_visitor: true,
          new_message: true,
          operator_request: false,
        },
      }),
    ));
  });

  it("saves the languages the agent can reply in as a comma-separated list", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    const languages = screen.getByRole("textbox", { name: "Reply languages" });
    expect(languages).toHaveAttribute("placeholder", "ru, en, de");
    expect(screen.getByText(/language codes separated by commas/)).toBeInTheDocument();

    fireEvent.change(languages, { target: { value: "ru, en, de" } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth,
      profile.id,
      expect.objectContaining({ language: "ru, en, de" }),
    ));
  });

  it("keeps a new localized display name in sync while the internal name is typed", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "New agent" }));
    const internalName = screen.getByRole("textbox", { name: "Internal agent name" });
    fireEvent.change(internalName, { target: { value: "Widget" } });
    fireEvent.change(internalName, { target: { value: "Widget operator" } });
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Main website/ }));

    expect(screen.getByRole("textbox", { name: "Display name · ru" })).toHaveValue(
      "Widget operator",
    );
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.createAiProfile).toHaveBeenCalledWith(
      auth,
      expect.objectContaining({
        name: "Widget operator",
        public_identities: [{ language: "ru", display_name: "Widget operator" }],
      }),
    ));
  });

  it("loads AI channel options and assigns the agent to exact project channels", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    expect(api.listAiChannelOptions).toHaveBeenCalledWith(auth);
    const channelAssignment = screen.getByRole("checkbox", { name: /Main website/ });
    expect(channelAssignment).toBeChecked();
    expect(screen.getByText(/Widget · Active · Customer support/)).toBeInTheDocument();

    fireEvent.click(channelAssignment);
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth,
      profile.id,
      expect.objectContaining({ channel_ids: [] }),
    ));
  });

  it("shows client identities only while an active Widget channel is selected", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    expect(screen.getByRole("heading", { name: "Web widget profiles by language" })).toBeInTheDocument();
    expect(screen.getByText(/not in Telegram, email, or other channels/)).toBeInTheDocument();
    expect(screen.getByText(/only when an active Widget channel is selected/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("checkbox", { name: /Main website/ }));

    expect(screen.queryByRole("heading", { name: "Web widget profiles by language" })).not.toBeInTheDocument();
  });

  it("hides client identities when the selected Widget channel is not active", async () => {
    vi.mocked(api.listAiChannelOptions).mockResolvedValue([{ ...channel, status: "disabled" }]);
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));

    expect(screen.queryByRole("heading", { name: "Web widget profiles by language" })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /New website visitor/ })).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /New customer message/ })).toBeInTheDocument();
  });

  it("saves and uploads the client identity for one reply language", async () => {
    const avatarFile = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], "kate.png", {
      type: "image/png",
    });
    vi.mocked(api.uploadAiProfilePublicIdentityAvatar).mockResolvedValue(profile);
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Display name · en" }), {
      target: { value: "Catherine" },
    });
    fireEvent.change(screen.getByLabelText("Avatar · en"), {
      target: { files: [avatarFile] },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth,
      profile.id,
      expect.objectContaining({
        public_identities: [{ language: "en", display_name: "Catherine" }],
      }),
    ));
    await waitFor(() => expect(api.uploadAiProfilePublicIdentityAvatar).toHaveBeenCalledWith(
      auth,
      profile.id,
      "en",
      avatarFile,
    ));
  });

  it("keeps stored secrets write-only while saving visible fields and new secrets", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("tab", { name: "Knowledge and tools" }));
    expect(screen.getByRole("group", { name: "Agent variables" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Secrets" })).not.toBeInTheDocument();
    const storedSecret = screen.getByLabelText("Secret value");
    expect(storedSecret).toHaveValue("");
    expect(storedSecret).toHaveAttribute("placeholder", "Leave blank to keep the stored value");
    expect(screen.queryByDisplayValue("database-secret")).not.toBeInTheDocument();

    fireEvent.change(screen.getByDisplayValue("db.internal"), { target: { value: "db.prod" } });
    fireEvent.click(screen.getByRole("button", { name: "Add secret" }));
    const newSecretValue = screen.getByPlaceholderText("Enter secret value");
    const newSecretRow = newSecretValue.closest(".ai-credential-row") as HTMLElement;
    fireEvent.change(within(newSecretRow).getByLabelText("Secret"), { target: { value: "API_TOKEN" } });
    fireEvent.change(newSecretValue, { target: { value: "new-secret" } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(
      auth,
      profile.id,
      expect.objectContaining({
        custom_fields: [{ key: "DB_HOST", value: "db.prod" }],
        secrets: [
          { key: "DB_PASSWORD", value: null },
          { key: "API_TOKEN", value: "new-secret" },
        ],
      }),
    ));
    expect(JSON.stringify(savedProfile)).not.toContain("new-secret");
  });

  it("keeps Tasks out of the AI settings tabs", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    const tabs = await screen.findByRole("tablist", { name: "AI settings sections" });

    expect(within(tabs).getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
      "Agents",
      "Model connections",
      "Skills",
    ]);
    expect(within(tabs).queryByRole("tab", { name: "Tasks" })).not.toBeInTheDocument();
    expect(api.listAiTasks).not.toHaveBeenCalled();
  });

  it("filters model connections without losing the current draft and can start a new connection", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(screen.getByRole("tab", { name: "Model connections" }));
    const list = screen.getByRole("complementary", { name: "Model connections" });
    fireEvent.click(await within(list).findByRole("button", { name: /OpenAI/ }));
    fireEvent.change(screen.getByLabelText("Connection name"), { target: { value: "My connection draft" } });
    const search = within(list).getByRole("searchbox", { name: "Find a connection" });
    fireEvent.change(search, { target: { value: "anthropic" } });
    expect(within(list).queryByRole("button", { name: /OpenAI/ })).not.toBeInTheDocument();
    expect(within(list).getByRole("button", { name: /Anthropic/ })).toBeInTheDocument();
    expect(screen.getByLabelText("Connection name")).toHaveValue("My connection draft");
    fireEvent.change(search, { target: { value: "unmatched" } });
    expect(within(list).getByRole("status")).toHaveTextContent("No connections match your search.");
    fireEvent.click(within(list).getByRole("button", { name: "Add connection" }));
    expect(screen.getByLabelText("Connection name")).toHaveValue("");
    expect(screen.getByRole("heading", { name: "Create model connection" })).toBeInTheDocument();
    expect(api.updateAiProvider).not.toHaveBeenCalled();
    expect(api.createAiProvider).not.toHaveBeenCalled();
  });

  it("keeps an assigned hidden model selected while editing an agent", async () => {
    vi.mocked(api.listAiProviders).mockResolvedValue([anthropicProvider]);
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    const selection = screen.getByRole("combobox", { name: "Model connection" });
    expect(selection).toHaveValue(provider.id);
    expect(within(selection).getByRole("option", { name: "Assigned model connection" })).toHaveProperty("selected", true);
    fireEvent.change(selection, { target: { value: anthropicProvider.id } });
    fireEvent.change(selection, { target: { value: provider.id } });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));
    await waitFor(() => expect(api.updateAiProfile).toHaveBeenCalledWith(auth, profile.id, expect.objectContaining({ provider_connection_id: provider.id })));
  });

  it.each(["new", "existing"] as const)("closes the %s model editor when saved visibility excludes this workspace", async (kind) => {
    vi.mocked(api.listAiProviders).mockResolvedValueOnce([provider]).mockResolvedValue([]);
    vi.mocked(api.createAiProvider).mockResolvedValue(provider);
    vi.mocked(api.updateAiProvider).mockResolvedValue(provider);
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(screen.getByRole("tab", { name: "Model connections" }));
    const list = screen.getByRole("complementary", { name: "Model connections" });
    const existing = await within(list).findByRole("button", { name: /OpenAI/ });
    if (kind === "existing") fireEvent.click(existing);
    else {
      fireEvent.click(screen.getByRole("button", { name: "New connection" }));
      fireEvent.change(screen.getByLabelText("Connection name"), { target: { value: "Shared model" } });
      fireEvent.change(screen.getByLabelText("API base URL"), { target: { value: provider.base_url } });
      fireEvent.change(screen.getByLabelText("Default model"), { target: { value: provider.default_model } });
    }
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Save connection" })).not.toBeInTheDocument());
    expect(screen.queryByLabelText("Connection name")).not.toBeInTheDocument();
    expect(kind === "new" ? api.createAiProvider : api.updateAiProvider).toHaveBeenCalledTimes(1);
  });

  it("does not offer the removed Letta provider type", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(screen.getByRole("tab", { name: "Model connections" }));
    fireEvent.click(screen.getByRole("button", { name: "New connection" }));

    const providerType = screen.getByRole("combobox", { name: "Provider type" });
    expect(within(providerType).queryByRole("option", { name: "Letta agent" })).not.toBeInTheDocument();
    expect(Array.from((providerType as HTMLSelectElement).options, (option) => option.value)).toEqual([
      "openai",
      "anthropic",
      "openai_compatible",
    ]);
  });

  it("uploads an avatar for an AI agent after saving its profile", async () => {
    const avatarFile = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], "agent.png", {
      type: "image/png",
    });
    vi.mocked(api.uploadAiProfileAvatar).mockImplementation(async (_auth, _profileId, file) => {
      expect(file).toBe(avatarFile);
      savedProfile = {
        ...savedProfile,
        avatar_url: "/public/v1/avatars/00000000-0000-0000-0000-000000000021",
      };
      return savedProfile;
    });

    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <AISettingsView auth={auth} inboxes={[inbox]} />
      </I18nContext.Provider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: /Support agent/ }));
    fireEvent.click(screen.getByText("Internal avatar", { selector: "summary" }));
    fireEvent.change(screen.getByLabelText("Internal avatar"), {
      target: { files: [avatarFile] },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save agent" }));

    await waitFor(() => expect(api.uploadAiProfileAvatar).toHaveBeenCalledWith(
      auth,
      profile.id,
      avatarFile,
    ));
  });

});
