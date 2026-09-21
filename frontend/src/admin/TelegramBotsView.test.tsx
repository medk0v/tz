import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { I18nContext, createI18n } from "../i18n";
import { TelegramBotsView } from "./TelegramBotsView";

vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return {
    ...actual,
    createTelegramBotChannel: vi.fn(),
    deleteChannel: vi.fn(),
    listChannels: vi.fn(),
  };
});

const auth: api.OperatorAuth = { kind: "session" };
const inbox: api.Inbox = {
  id: "00000000-0000-4000-8000-000000000010",
  project_id: "00000000-0000-4000-8000-000000000011",
  name: "Customer support",
  status: "active",
  created_at: "2026-09-03T00:00:00Z",
};

function bot(id: string, name: string, username: string): api.ChannelConnection {
  return {
    id,
    project_id: inbox.project_id,
    inbox_id: inbox.id,
    public_id: id,
    blacklist_reply: { default_language: "en", translations: { en: "Blocked due to spam." } },
    kind: "telegram_bot",
    name,
    status: "active",
    telegram_bot_username: username,
    telegram_bot_token_configured: true,
    telegram_webhook_url: `https://support.example/telegram/v1/webhooks/${id}`,
    updated_at: "2026-09-03T00:00:00Z",
  };
}

describe("TelegramBotsView", () => {
  beforeEach(() => {
    vi.mocked(api.createTelegramBotChannel).mockResolvedValue(
      bot("00000000-0000-4000-8000-000000000030", "Sales bot", "sales_bot"),
    );
    vi.mocked(api.deleteChannel).mockResolvedValue();
    vi.mocked(api.listChannels).mockResolvedValue([
      bot("00000000-0000-4000-8000-000000000020", "Support bot", "support_bot"),
    ]);
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });

  it("lists multiple bots and connects another bot to the selected Inbox", async () => {
    const onCreated = vi.fn();
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <TelegramBotsView
          auth={auth}
          inboxes={[inbox]}
          bots={[
            bot("00000000-0000-4000-8000-000000000020", "Support bot", "support_bot"),
            bot("00000000-0000-4000-8000-000000000021", "Billing bot", "billing_bot"),
          ]}
          canManage
          onBack={vi.fn()}
          onCreated={onCreated}
          onDeleted={vi.fn()}
          onReloaded={vi.fn()}
        />
      </I18nContext.Provider>,
    );

    expect(screen.getByText("@support_bot")).toBeInTheDocument();
    expect(screen.getByText("@billing_bot")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Connection name"), {
      target: { value: "Sales bot" },
    });
    fireEvent.change(screen.getByLabelText(/Bot token/), {
      target: { value: "123456:abcdefghijklmnopqrstuvwxyz" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect bot" }));

    await waitFor(() => expect(api.createTelegramBotChannel).toHaveBeenCalledWith(auth, {
      inbox_id: inbox.id,
      name: "Sales bot",
      bot_token: "123456:abcdefghijklmnopqrstuvwxyz",
    }));
    expect(onCreated).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText(/Bot token/)).toHaveValue("");
  });

  it("refreshes a bot while webhook registration is pending", async () => {
    const onReloaded = vi.fn();
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <TelegramBotsView
          auth={auth}
          inboxes={[inbox]}
          bots={[{
            ...bot("00000000-0000-4000-8000-000000000020", "Support bot", "support_bot"),
            status: "connecting",
          }]}
          canManage
          onBack={vi.fn()}
          onCreated={vi.fn()}
          onDeleted={vi.fn()}
          onReloaded={onReloaded}
        />
      </I18nContext.Provider>,
    );

    await waitFor(() => expect(api.listChannels).toHaveBeenCalledWith(auth));
    expect(onReloaded).toHaveBeenCalledWith([
      expect.objectContaining({ status: "active", telegram_bot_username: "support_bot" }),
    ]);
  });
});
