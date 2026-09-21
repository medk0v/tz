import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import { ChannelBlacklistSettings } from "./ChannelBlacklistSettings";
import { ChannelsView } from "./ChannelsView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(), updateChannelBlacklist: vi.fn(), listChannels: vi.fn(), createTelegramBotChannel: vi.fn(),
}));
const auth: api.OperatorAuth = { kind: "session" };
const channel: api.ChannelConnection = {
  id: "channel-1", project_id: "project-1", inbox_id: "inbox-1", public_id: "public-1", kind: "widget", name: "Website",
  status: "active", updated_at: "2026-09-01T00:00:00Z",
  blacklist_reply: { default_language: "en", translations: { en: "You are blocked due to spam. Contact the email address listed on our website.", ru: "Вы заблокированы из-за спама. Свяжитесь с нами по почте, указанной на сайте." } },
};
const inbox: api.Inbox = { id: "inbox-1", project_id: "project-1", name: "Support", status: "active", created_at: "2026-09-01T00:00:00Z" };
function show(canManage = true, onUpdated = vi.fn()) {
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><ChannelBlacklistSettings auth={auth} channel={channel} canManage={canManage} onUpdated={onUpdated} onBack={vi.fn()} /></I18nContext.Provider>);
}
function showChannels() {
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><MemoryPageRoute><ChannelsView auth={auth} inboxes={[inbox]} canManage /><CurrentPageRoute /></MemoryPageRoute></I18nContext.Provider>);
}
const pageRoute = () => screen.getByTestId("page-route").textContent;
afterEach(cleanup);
beforeEach(() => {
  vi.mocked(api.updateChannelBlacklist).mockReset().mockResolvedValue(channel);
  vi.mocked(api.listChannels).mockReset().mockResolvedValue([channel, { ...channel, id: "telegram-1", kind: "telegram_bot", name: "Telegram support" }]);
});

describe("channel blacklist replies", () => {
  it("saves an edited translation and selected fallback while preserving other languages", async () => {
    const onUpdated = vi.fn();
    show(true, onUpdated);
    fireEvent.change(screen.getByLabelText("Reply message (ru)"), { target: { value: "  Свяжитесь по почте на сайте.  " } });
    fireEvent.change(screen.getByLabelText("Default reply language"), { target: { value: "ru" } });
    fireEvent.click(screen.getByRole("button", { name: "Save blacklist reply" }));
    await screen.findByRole("status");
    expect(api.updateChannelBlacklist).toHaveBeenCalledWith(auth, channel.id, { default_language: "ru", translations: { en: channel.blacklist_reply.translations.en, ru: "Свяжитесь по почте на сайте." } });
    expect(onUpdated).toHaveBeenCalledWith(channel);
  });

  it("adds a language, prevents duplicate tags, and protects the default translation from removal", async () => {
    show();
    expect(screen.getByRole("button", { name: "Remove language en" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Add language" }));
    const languageFields = screen.getAllByRole("textbox", { name: /^Language code/ });
    fireEvent.change(languageFields[2], { target: { value: "EN" } });
    const messageFields = screen.getAllByRole("textbox", { name: /^Reply message/ });
    fireEvent.change(messageFields[2], { target: { value: "A separate reply." } });
    fireEvent.click(screen.getByRole("button", { name: "Save blacklist reply" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Use unique language codes");
    expect(api.updateChannelBlacklist).not.toHaveBeenCalled();
    fireEvent.change(languageFields[2], { target: { value: "PT_BR" } });
    fireEvent.click(screen.getByRole("button", { name: "Save blacklist reply" }));
    await waitFor(() => expect(api.updateChannelBlacklist).toHaveBeenCalledWith(auth, channel.id, expect.objectContaining({ translations: expect.objectContaining({ "pt-br": "A separate reply." }) })));
  });

  it("preserves the draft after a save failure and prevents editing without permission", async () => {
    vi.mocked(api.updateChannelBlacklist).mockRejectedValue(new Error("forbidden"));
    show();
    fireEvent.change(screen.getByLabelText("Reply message (en)"), { target: { value: "Custom reply" } });
    fireEvent.click(screen.getByRole("button", { name: "Save blacklist reply" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save the blacklist reply");
    expect(screen.getByLabelText("Reply message (en)")).toHaveValue("Custom reply");
    cleanup();
    show(false);
    expect(screen.getByLabelText("Reply message (en)")).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Save blacklist reply" })).not.toBeInTheDocument();
  });

  it("opens blacklist settings immediately after connecting a Telegram bot", async () => {
    vi.mocked(api.createTelegramBotChannel).mockResolvedValue({ ...channel, id: "telegram-new", kind: "telegram_bot", name: "New bot" });
    showChannels();
    const telegramRow = (await screen.findByText("Telegram")).closest("tr")!;
    fireEvent.click(within(telegramRow).getByRole("button", { name: "Open" }));
    fireEvent.change(screen.getByLabelText("Connection name"), { target: { value: "New bot" } });
    fireEvent.change(screen.getByLabelText(/Bot token/), { target: { value: "123456:abcdefghijklmnopqrstuvwxyz" } });
    fireEvent.click(screen.getByRole("button", { name: "Connect bot" }));
    const newRow = (await screen.findByText("New bot")).closest("tr")!;
    fireEvent.click(within(newRow).getByRole("button", { name: "Blacklist reply" }));
    expect(await screen.findByRole("heading", { name: "Blacklist reply settings · New bot" })).toBeInTheDocument();
    expect(screen.getByLabelText("Reply message (en)")).toHaveValue(channel.blacklist_reply.translations.en);
    expect(pageRoute()).toBe("blacklist/telegram-new");
  });

  it.each(["Website", "Telegram support"])("opens per-channel settings for %s from the catalog", async (name) => {
    showChannels();
    const picker = await screen.findByLabelText("Blacklist reply settings");
    const option = within(picker).getByRole("option", { name });
    fireEvent.change(picker, { target: { value: option.getAttribute("value") } });
    expect(await screen.findByRole("heading", { name: `Blacklist reply settings · ${name}` })).toBeInTheDocument();
    expect(screen.getByLabelText("Reply message (ru)")).toHaveValue(channel.blacklist_reply.translations.ru);
    expect(pageRoute()).toBe(`blacklist/${option.getAttribute("value")}`);
  });
});
