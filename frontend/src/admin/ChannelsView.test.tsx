import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import * as customChannelApi from "../custom-ai-channel-api";
import { createI18n, I18nContext } from "../i18n";
import { ChannelsView } from "./ChannelsView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";

const edition = vi.hoisted(() => ({
  isLiteEdition: false,
  get productNamespace() { return this.isLiteEdition ? "tz" : "tzomet"; },
}));
vi.mock("../product-edition", () => edition);
vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  listChannels: vi.fn(), createWidgetChannel: vi.fn(), getEmailSettings: vi.fn(),
}));
vi.mock("../custom-ai-channel-api", () => ({ listCustomAiChannels: vi.fn() }));

const auth: api.OperatorAuth = { kind: "session" };
const inbox: api.Inbox = { id: "inbox-1", project_id: "project-1", name: "Support", status: "active", created_at: "2026-09-01T00:00:00Z" };
const widget: api.ChannelConnection = {
  id: "widget-1", project_id: "project-1", inbox_id: inbox.id, public_id: "public-1", kind: "widget", name: "Website chat", status: "active",
  allowed_origins: ["https://example.com"], updated_at: "2026-09-01T00:00:00Z",
  blacklist_reply: { default_language: "en", translations: { en: "You are blocked due to spam." } },
};
const telegramBot: api.ChannelConnection = { ...widget, id: "telegram-1", public_id: "public-2", kind: "telegram_bot", name: "Support bot", allowed_origins: null };
const phone: api.ChannelConnection = {
  ...widget, id: "phone-1", public_id: "public-3", kind: "phone", name: "Support line", allowed_origins: null,
  phone_number: "+37360000000", phone_gateway: "asterisk", phone_greeting: "Hello!", phone_transfer_number: null, phone_max_call_seconds: 900,
};

function showChannels(segments: string[] = [], { canManage = true, showDefaultChannels = true } = {}) {
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><MemoryPageRoute initialSegments={segments}><ChannelsView auth={auth} inboxes={[inbox]} canManage={canManage} showDefaultChannels={showDefaultChannels} /><CurrentPageRoute /></MemoryPageRoute></I18nContext.Provider>);
}

const pageRoute = () => screen.getByTestId("page-route").textContent;

beforeEach(() => {
  edition.isLiteEdition = false;
  vi.mocked(api.listChannels).mockResolvedValue([widget, telegramBot, phone]);
  vi.mocked(api.getEmailSettings).mockResolvedValue({
    configured: false, enabled: false, smtp_host: "", smtp_port: 587, smtp_security: "starttls", smtp_username: "", password_configured: false,
    from_name: "", from_email: "", reply_to_email: null, rating_page_url: null, updated_at: null,
  } as unknown as api.EmailSettings);
  vi.mocked(customChannelApi.listCustomAiChannels).mockResolvedValue([]);
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

describe("channels page URLs", () => {
  it("opens the widget section named by the URL and keeps sections, list and catalog in the URL", async () => {
    showChannels(["widgets", widget.id, "launcher"]);
    expect(await screen.findByRole("heading", { level: 1, name: widget.name })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Launcher" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("tab", { name: "Appearance" }));
    expect(screen.getByRole("tab", { name: "Appearance" })).toHaveAttribute("aria-selected", "true");
    expect(pageRoute()).toBe(`widgets/${widget.id}/appearance`);
    fireEvent.keyDown(screen.getByRole("tab", { name: "Appearance" }), { key: "ArrowRight" });
    expect(pageRoute()).toBe(`widgets/${widget.id}/install`);
    fireEvent.click(screen.getByRole("tab", { name: "General" }));
    expect(pageRoute()).toBe(`widgets/${widget.id}`);
    fireEvent.click(screen.getByRole("button", { name: "Back to widgets" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Website widgets" })).toBeInTheDocument();
    expect(pageRoute()).toBe("widgets");
    fireEvent.click(screen.getByRole("button", { name: widget.name }));
    expect(await screen.findByRole("heading", { level: 1, name: widget.name })).toBeInTheDocument();
    expect(pageRoute()).toBe(`widgets/${widget.id}`);
    fireEvent.click(screen.getByRole("button", { name: "Back to widgets" }));
    fireEvent.click(screen.getByRole("button", { name: "Back to channels" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Channels" })).toBeInTheDocument();
    expect(pageRoute()).toBe("");
  });

  it("keeps the creation form's section in the URL and names the saved widget after creating it", async () => {
    const created: api.ChannelConnection = { ...widget, id: "widget-created", public_id: "public-created", name: "Docs chat", allowed_origins: ["https://docs.example"] };
    vi.mocked(api.createWidgetChannel).mockResolvedValue(created);
    showChannels(["widgets"]);
    fireEvent.click((await screen.findAllByRole("button", { name: "Create widget" }))[0]);
    expect(await screen.findByRole("heading", { level: 1, name: "New website widget" })).toBeInTheDocument();
    expect(pageRoute()).toBe("widgets/new");
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Docs chat" } });
    fireEvent.change(screen.getByLabelText(/^Allowed origins/), { target: { value: "https://docs.example" } });
    fireEvent.click(screen.getByRole("tab", { name: "Installation" }));
    expect(pageRoute()).toBe("widgets/new/install");
    fireEvent.click(screen.getByRole("button", { name: "Create widget" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Docs chat" })).toBeInTheDocument();
    expect(screen.getByText("Widget created.")).toBeInTheDocument();
    expect(pageRoute()).toBe(`widgets/${created.id}/install`);
    expect(screen.getByRole("tab", { name: "Installation" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getAllByText(new RegExp(`data-widget-id="${created.public_id}"`)).length).toBeGreaterThan(0);
  });

  it.each([
    ["Website widgets", "widgets", "Website widgets"],
    ["Telegram", "telegram-bots", "Telegram bots"],
    ["Email", "email", "Rating invitation email"],
  ])("opens %s from the catalog at /%s and returns to the catalog", async (type, segments, heading) => {
    showChannels();
    fireEvent.click(within((await screen.findByText(type)).closest("tr")!).getByRole("button", { name: "Open" }));
    expect(await screen.findByRole("heading", { level: 1, name: heading })).toBeInTheDocument();
    expect(pageRoute()).toBe(segments);
    fireEvent.click(screen.getByRole("button", { name: "Back to channels" }));
    expect(await screen.findByRole("heading", { level: 1, name: "Channels" })).toBeInTheDocument();
    expect(pageRoute()).toBe("");
  });

  it.each([
    [["telegram-bots"], "Telegram bots"],
    [["email"], "Rating invitation email"],
    [["widgets", "new"], "New website widget"],
    [["blacklist", telegramBot.id], "Blacklist reply settings · Support bot"],
  ])("opens %j directly from the URL", async (segments, heading) => {
    showChannels(segments);
    expect(await screen.findByRole("heading", { level: 1, name: heading })).toBeInTheDocument();
    expect(pageRoute()).toBe(segments.join("/"));
  });

  it("records the blacklist settings of a channel opened from its list", async () => {
    showChannels(["widgets"]);
    fireEvent.click(await screen.findByRole("button", { name: "Blacklist reply" }));
    expect(await screen.findByRole("heading", { level: 1, name: `Blacklist reply settings · ${widget.name}` })).toBeInTheDocument();
    expect(pageRoute()).toBe(`blacklist/${widget.id}`);
    fireEvent.click(screen.getByRole("button", { name: "Back to channels" }));
    expect(pageRoute()).toBe("");
  });

  it.each([
    [["unknown"], ""],
    [["widgets", "missing-widget"], "widgets"],
    [["widgets", telegramBot.id], "widgets"],
    [["widgets", widget.id, "general"], `widgets/${widget.id}`],
    [["widgets", widget.id, "unknown"], `widgets/${widget.id}`],
    [["telegram-bots", "extra"], "telegram-bots"],
    [["email", "extra"], "email"],
    [["blacklist"], ""],
    [["blacklist", "missing-channel"], ""],
    [["custom-ai"], ""],
    [["custom-ai", "missing-channel"], ""],
  ])("replaces the stale channels URL %j with the screen it shows", async (segments, canonical) => {
    showChannels(segments);
    await waitFor(() => expect(pageRoute()).toBe(canonical));
  });

  it.each([
    [["widgets", "new"], "widgets"],
    [["custom-ai", "new"], ""],
  ])("replaces the manager-only URL %j for read-only operators", async (segments, canonical) => {
    showChannels(segments, { canManage: false });
    await waitFor(() => expect(pageRoute()).toBe(canonical));
  });

  it.each([[["widgets"]], [["telegram-bots"]], [["email"]], [["blacklist", widget.id]]])(
    "shows the catalog for the standard channel URL %j when the department hides standard channels", async (segments) => {
      showChannels(segments, { showDefaultChannels: false });
      await waitFor(() => expect(pageRoute()).toBe(""));
      expect(screen.getByRole("heading", { level: 1, name: "Channels" })).toBeInTheDocument();
    },
  );

  it("does not open phone numbers", async () => {
    showChannels(["phones", phone.id]);
    await waitFor(() => expect(pageRoute()).toBe(""));
    expect(screen.getByRole("heading", { level: 1, name: "Channels" })).toBeInTheDocument();
  });

  it("keeps the URL while channels cannot be loaded", async () => {
    vi.mocked(api.listChannels).mockRejectedValue(new Error("offline"));
    showChannels(["widgets", widget.id]);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(pageRoute()).toBe(`widgets/${widget.id}`);
  });
});
