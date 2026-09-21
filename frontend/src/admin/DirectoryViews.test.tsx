import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { listContacts, setContactBlocked, type ContactList } from "../api";
import { createI18n, I18nContext } from "../i18n";
import { changeDateTimeField, dateTimeFieldInput } from "./date-time-field-test-utils";
import { ContactsView } from "./DirectoryViews";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";

vi.mock("../api", () => ({ listContacts: vi.fn(), setContactBlocked: vi.fn() }));
const auth = { kind: "access_token", token: "test" } as const;
const response: ContactList = {
  items: [{ id: "contact-id", project_id: "project-id", is_blocked: false, display_name: "Long contact name", email: null,
    created_at: "2026-09-01T00:00:00Z", updated_at: "2026-09-01T00:00:00Z", last_activity_at: "2026-09-05T12:34:00Z",
    browser_timezone: "Europe/Berlin", channel_kinds: ["widget", "telegram_bot"], client_ip: "203.0.113.5",
    geo_ip: { country: "Germany", country_code: "DE", city: "Berlin", time_zone: "Europe/Berlin" },
    user_agent_details: { browser: "Safari", browser_version: "26.2", operating_system: "macOS", operating_system_version: "15", device_category: "pc" },
  }], total: 11, page: 1, per_page: 10, total_pages: 2,
  statistics: { contacts: 11, new_contacts: 4, returning_contacts: 7, conversations: 15, widget_sessions: 20, widget_contacts: 9, telegram_contacts: 3, countries: [{ country: "Germany", country_code: "DE", contacts: 11 }] },
};
function show(canManage = false) {
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><ContactsView auth={auth} canManage={canManage} /></I18nContext.Provider>);
}
function showAt(segments: string[]) {
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><MemoryPageRoute initialSegments={segments}><ContactsView auth={auth} /><CurrentPageRoute /></MemoryPageRoute></I18nContext.Provider>);
}
const pageRoute = () => screen.getByTestId("page-route");
afterEach(cleanup);
beforeEach(() => { vi.mocked(setContactBlocked).mockReset(); vi.mocked(listContacts).mockReset().mockResolvedValue(response); });

describe("Contacts directory", () => {
  it("adds a contact to the blacklist and removes it after successful API updates", async () => {
    vi.mocked(setContactBlocked).mockResolvedValueOnce({ id: "contact-id", is_blocked: true }).mockResolvedValueOnce({ id: "contact-id", is_blocked: false });
    show(true);
    fireEvent.click(await screen.findByRole("button", { name: "Add to blacklist" }));
    expect(await screen.findByText("Blacklisted")).toBeInTheDocument();
    expect(setContactBlocked).toHaveBeenLastCalledWith(auth, "contact-id", true);
    fireEvent.click(screen.getByRole("button", { name: "Remove from blacklist" }));
    await screen.findByRole("button", { name: "Add to blacklist" });
    expect(screen.queryByText("Blacklisted")).not.toBeInTheDocument();
    expect(setContactBlocked).toHaveBeenLastCalledWith(auth, "contact-id", false);
  });

  it("keeps the previous state when blocking fails and hides controls without permission", async () => {
    vi.mocked(setContactBlocked).mockRejectedValue(new Error("forbidden"));
    show(true);
    fireEvent.click(await screen.findByRole("button", { name: "Add to blacklist" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not change the blacklist state");
    expect(screen.queryByText("Blacklisted")).not.toBeInTheDocument();
    cleanup();
    show();
    await screen.findByText("Long contact name");
    expect(screen.queryByRole("button", { name: "Add to blacklist" })).not.toBeInTheDocument();
  });

  it("uses the default 24-hour window and resets pagination for every preset", async () => {
    show();
    await screen.findByText("Long contact name");
    expect(listContacts).toHaveBeenLastCalledWith(auth, expect.objectContaining({ period: "day", page: 1 }));
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() => expect(listContacts).toHaveBeenLastCalledWith(auth, expect.objectContaining({ page: 2 })));
    for (const period of ["week", "month", "all"]) {
      fireEvent.change(screen.getByLabelText("Period"), { target: { value: period } });
      await waitFor(() => expect(listContacts).toHaveBeenLastCalledWith(auth, expect.objectContaining({ period, page: 1 })));
    }
  });

  it("requires a complete custom range and includes the entire final day", async () => {
    show();
    await screen.findByText("Long contact name");
    fireEvent.change(screen.getByLabelText("Period"), { target: { value: "custom" } });
    changeDateTimeField(screen.getByLabelText("From"), "2026-08-01");
    expect(listContacts).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status")).toHaveTextContent("Select the start and end dates.");
    changeDateTimeField(screen.getByLabelText("To"), "2026-08-07");
    await waitFor(() => expect(listContacts).toHaveBeenLastCalledWith(auth, expect.objectContaining({
      period: "custom", from: new Date("2026-08-01T00:00:00").toISOString(), to: new Date("2026-08-08T00:00:00").toISOString(),
    })));
  });

  it("keeps the ends of the custom range from crossing and resets pagination when either changes", async () => {
    show();
    await screen.findByText("Long contact name");
    fireEvent.change(screen.getByLabelText("Period"), { target: { value: "custom" } });
    changeDateTimeField(screen.getByLabelText("From"), "2026-08-10");
    changeDateTimeField(screen.getByLabelText("To"), "2026-08-15");
    await waitFor(() => expect(listContacts).toHaveBeenLastCalledWith(auth, expect.objectContaining({ period: "custom", page: 1 })));
    expect(dateTimeFieldInput(screen.getByLabelText("From"))).toHaveValue("2026-08-10");
    expect(screen.getByLabelText("To")).toHaveTextContent("Aug 15, 2026");
    fireEvent.click(screen.getByRole("button", { name: "Next page" }));
    await waitFor(() => expect(listContacts).toHaveBeenLastCalledWith(auth, expect.objectContaining({ page: 2 })));

    fireEvent.click(screen.getByLabelText("From"));
    let picker = within(screen.getByRole("dialog", { name: "Choose date" }));
    expect(picker.getByRole("button", { name: "16 August 2026" })).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(picker.getByRole("button", { name: "12 August 2026" }));
    await waitFor(() => expect(listContacts).toHaveBeenLastCalledWith(auth, expect.objectContaining({
      page: 1, from: new Date("2026-08-12T00:00:00").toISOString(), to: new Date("2026-08-16T00:00:00").toISOString(),
    })));

    fireEvent.click(screen.getByLabelText("To"));
    picker = within(screen.getByRole("dialog", { name: "Choose date" }));
    expect(picker.getByRole("button", { name: "11 August 2026" })).toHaveAttribute("aria-disabled", "true");
    expect(picker.getByRole("button", { name: "12 August 2026" })).not.toHaveAttribute("aria-disabled");

    changeDateTimeField(screen.getByLabelText("To"), "2026-08-01");
    expect(screen.getByLabelText("To")).toBeInvalid();
    expect(screen.getByRole("status")).toHaveTextContent("Select the start and end dates.");
  });

  it("shows server totals across all pages and keeps statistics in the current search", async () => {
    show();
    await screen.findByText("Long contact name");
    fireEvent.click(screen.getByRole("tab", { name: "Statistics" }));
    expect(screen.getByRole("rowheader", { name: "Unique contacts" }).closest("tr")).toHaveTextContent("11");
    expect(screen.getByText("Returning contacts").closest("tr")).toHaveTextContent("7");
    expect(screen.getByText("Active conversations").closest("tr")).toHaveTextContent("15");
    fireEvent.change(screen.getByPlaceholderText("Search contacts"), { target: { value: "Berlin" } });
    await waitFor(() => expect(listContacts).toHaveBeenLastCalledWith(auth, expect.objectContaining({ search: "Berlin", period: "day" })));
  });

  it("shows location and channel details, with IDs available through a keyboard accessible tooltip", async () => {
    show();
    await screen.findByText("Long contact name");
    expect(screen.queryByRole("columnheader", { name: "Contact ID" })).not.toBeInTheDocument();
    expect(screen.queryByRole("columnheader", { name: "Project ID" })).not.toBeInTheDocument();
    expect(screen.getByText("🇩🇪")).toBeInTheDocument();
    expect(screen.getByText("Berlin")).toBeInTheDocument();
    expect(screen.getByText("Widget")).toBeInTheDocument();
    expect(screen.getByText("Telegram")).toBeInTheDocument();
    const info = screen.getByRole("button", { name: "Additional information" });
    fireEvent.focus(info);
    expect(screen.getByRole("tooltip")).toHaveTextContent("Contact ID: contact-id");
    expect(screen.getByRole("tooltip")).toHaveTextContent("Project ID: project-id");
    fireEvent.keyDown(info, { key: "Escape" });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("opens the tab named by the page URL and keeps the URL current", async () => {
    showAt(["statistics"]);
    expect(await screen.findByRole("rowheader", { name: "Unique contacts" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Statistics" })).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByRole("tab", { name: "Contacts" }));
    expect(pageRoute()).toHaveTextContent(/^$/);
    expect(await screen.findByText("Long contact name")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("tab", { name: "Contacts" }), { key: "End" });
    expect(pageRoute()).toHaveTextContent(/^statistics$/);
    expect(screen.getByRole("tab", { name: "Statistics" })).toHaveFocus();
    expect(screen.getByRole("tab", { name: "Statistics" })).toHaveAttribute("aria-selected", "true");
  });

  it.each([[["unknown"], ""], [["statistics", "extra"], "statistics"]])("replaces the contacts URL %j with the tab it shows", async (segments, canonical) => {
    showAt(segments);
    await waitFor(() => expect(pageRoute().textContent).toBe(canonical));
  });
});
