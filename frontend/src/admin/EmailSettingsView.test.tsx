import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { I18nContext, createI18n } from "../i18n";
import { EmailSettingsView } from "./EmailSettingsView";

vi.mock("../api", () => ({
  getEmailSettings: vi.fn(),
  updateEmailSettings: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const settings: api.EmailSettings = {
  configured: true,
  enabled: true,
  smtp_host: "smtp.example.com",
  smtp_port: 587,
  smtp_security: "starttls",
  smtp_username: "mailer@example.com",
  password_configured: true,
  from_name: "Customer Support",
  from_email: "support@example.com",
  reply_to_email: null,
  rating_page_url: "https://support.example.com/rate-chat",
  updated_at: "2026-08-19T00:00:00Z",
};

describe("EmailSettingsView", () => {
  beforeEach(() => {
    vi.mocked(api.getEmailSettings).mockResolvedValue(settings);
    vi.mocked(api.updateEmailSettings).mockResolvedValue(settings);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("keeps the stored password while saving non-secret SMTP fields", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <EmailSettingsView auth={auth} canManage onBack={vi.fn()} />
      </I18nContext.Provider>,
    );

    expect(await screen.findByDisplayValue("smtp.example.com")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Sender name"), { target: { value: "Tzomet Support" } });
    fireEvent.click(screen.getByRole("button", { name: "Save email settings" }));

    await waitFor(() => expect(api.updateEmailSettings).toHaveBeenCalledWith(
      auth,
      expect.objectContaining({
        from_name: "Tzomet Support",
        smtp_password: null,
        clear_smtp_password: false,
      }),
    ));
  });

  it("clears the previous success notice when a repeated save fails", async () => {
    const i18n = createI18n("en", vi.fn());
    render(<I18nContext.Provider value={i18n}><EmailSettingsView auth={auth} canManage onBack={vi.fn()} /></I18nContext.Provider>);
    await screen.findByDisplayValue("smtp.example.com");
    fireEvent.click(screen.getByRole("button", { name: "Save email settings" }));
    expect(await screen.findByRole("status")).toHaveTextContent(i18n.t("emailSettings.saved"));

    vi.mocked(api.updateEmailSettings).mockRejectedValueOnce(new Error("Unavailable"));
    fireEvent.click(screen.getByRole("button", { name: "Save email settings" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(i18n.t("emailSettings.error"));
    expect(screen.queryByText(i18n.t("emailSettings.saved"))).not.toBeInTheDocument();
  });
});
