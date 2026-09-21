import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import { IntegrationsView } from "./IntegrationsView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";
import { usePageRoute } from "./page-route";
import { ResourceVisibilityPermissionContext } from "../resource-visibility";

vi.mock("../api", () => ({
  managementApiRequest: vi.fn(),
  createApiIntegration: vi.fn(),
  deleteApiIntegration: vi.fn(),
  listApiIntegrations: vi.fn(),
  updateApiIntegration: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const supportProfile: api.ApiIntegrationAiProfile = {
  id: "00000000-0000-4000-8000-000000000011",
  name: "Support agent",
  status: "active",
  runtime_available: true,
};
const analystProfile: api.ApiIntegrationAiProfile = {
  id: "00000000-0000-4000-8000-000000000012",
  name: "Analyst agent",
  status: "active",
  runtime_available: true,
};
const unavailableProfile: api.ApiIntegrationAiProfile = {
  id: "00000000-0000-4000-8000-000000000013",
  name: "Legacy agent",
  status: "disabled",
  runtime_available: false,
};
const integration: api.ApiIntegration = {
  id: "00000000-0000-4000-8000-000000000021",
  name: "Orders API",
  key: "orders_api",
  description: "Read an order by its public identifier.",
  base_url: "https://orders.example.com",
  status: "active",
  auth_kind: "bearer",
  token_configured: true,
  actions: [{
    id: "00000000-0000-4000-8000-000000000031",
    key: "get_order",
    name: "Get order",
    description: "Return the current order state.",
    path_template: "/orders/{order_id}",
    parameter_names: ["order_id"],
  }],
  ai_profile_ids: [supportProfile.id],
  created_at: "2026-08-29T00:00:00Z",
  updated_at: "2026-08-29T00:00:00Z",
};

function renderView(canChoose = false) {
  return render(
    <I18nContext.Provider value={createI18n("en", vi.fn())}>
      <ResourceVisibilityPermissionContext.Provider value={canChoose}>
        <IntegrationsView auth={auth} canManageAi />
      </ResourceVisibilityPermissionContext.Provider>
    </I18nContext.Provider>,
  );
}

/** Moves the page URL as Back/Forward or a pasted link would. */
function RouteTo({ segments }: { segments: string[] }) {
  const route = usePageRoute();
  return <button type="button" onClick={() => void route.navigate(segments)}>{`Open /${segments.join("/")}`}</button>;
}

function renderRouted(segments: string[], moves: string[][] = []) {
  return render(
    <I18nContext.Provider value={createI18n("en", vi.fn())}>
      <MemoryPageRoute initialSegments={segments}>
        <IntegrationsView auth={auth} canManageAi />
        <CurrentPageRoute />
        {moves.map((move) => <RouteTo key={move.join("/")} segments={move} />)}
      </MemoryPageRoute>
    </I18nContext.Provider>,
  );
}

const pageRoute = () => screen.getByTestId("page-route");

describe("IntegrationsView", () => {
  beforeEach(() => {
    vi.mocked(api.managementApiRequest).mockResolvedValue({
      can_choose: true,
      projects: [{ id: "project-a", name: "Project A" }],
      departments: [{ id: "department-a", project_id: "project-a", name: "Support" }],
    });
    vi.mocked(api.listApiIntegrations).mockResolvedValue({
      items: [integration],
      ai_profiles: [supportProfile, analystProfile, unavailableProfile],
    });
    vi.mocked(api.deleteApiIntegration).mockResolvedValue();
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.restoreAllMocks();
  });

  it("hides scope controls for employees and preserves an existing integration scope", async () => {
    const visibility = { project_ids: ["project-a"], department_ids: ["department-a"] };
    vi.mocked(api.listApiIntegrations).mockResolvedValue({ items: [{ ...integration, visibility }], ai_profiles: [] });
    vi.mocked(api.updateApiIntegration).mockResolvedValue({ ...integration, visibility });
    renderView();
    fireEvent.click(await screen.findByRole("button", { name: /Orders API/ }));
    expect(screen.queryByText("Projects and departments")).not.toBeInTheDocument();
    expect(api.managementApiRequest).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }));
    await waitFor(() => expect(api.updateApiIntegration).toHaveBeenCalledWith(auth, integration.id,
      expect.objectContaining({ visibility })));
  });

  it("loads connections and creates a named GET integration with an agent assignment", async () => {
    const created: api.ApiIntegration = {
      ...integration,
      id: "00000000-0000-4000-8000-000000000022",
      name: "Billing API",
      key: "billing_api",
      base_url: "https://billing.example.com",
      description: "Read a customer invoice.",
      actions: [{
        id: "00000000-0000-4000-8000-000000000032",
        key: "get_invoice",
        name: "Get invoice",
        description: "Return an invoice by identifier.",
        path_template: "/invoices?invoice_id={invoice_id}",
        parameter_names: ["invoice_id"],
      }],
      ai_profile_ids: [analystProfile.id],
    };
    vi.mocked(api.createApiIntegration).mockResolvedValue(created);
    renderView();

    expect(await screen.findByRole("button", { name: /Orders API/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "New API connection" }));
    fireEvent.change(screen.getByLabelText("Connection name"), { target: { value: "Billing API" } });
    fireEvent.change(screen.getByLabelText("Stable key"), { target: { value: "billing_api" } });
    fireEvent.change(screen.getByLabelText("HTTPS base URL"), { target: { value: "https://billing.example.com" } });
    fireEvent.change(screen.getByLabelText("Description for AI"), { target: { value: "Read a customer invoice." } });
    fireEvent.change(screen.getByLabelText("Authentication"), { target: { value: "bearer" } });
    fireEvent.change(screen.getByLabelText("Access token"), { target: { value: "billing-secret" } });
    fireEvent.change(screen.getByLabelText("Operation key"), { target: { value: "get_invoice" } });
    fireEvent.change(screen.getByLabelText("Operation name"), { target: { value: "Get invoice" } });
    fireEvent.change(screen.getByLabelText("Operation description"), { target: { value: "Return an invoice by identifier." } });
    fireEvent.change(screen.getByLabelText("Path template"), { target: { value: "/invoices?invoice_id={invoice_id}" } });

    const unavailableAgent = screen.getByRole("checkbox", { name: /Legacy agent/ });
    expect(unavailableAgent).toBeDisabled();
    expect(screen.getByText("Runtime unavailable")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /Analyst agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }));

    await waitFor(() => expect(api.createApiIntegration).toHaveBeenCalledWith(auth, {
      name: "Billing API",
      key: "billing_api",
      description: "Read a customer invoice.",
      base_url: "https://billing.example.com",
      status: "active",
      auth_kind: "bearer",
      token: "billing-secret",
      clear_token: false,
      actions: [{
        key: "get_invoice",
        name: "Get invoice",
        description: "Return an invoice by identifier.",
        path_template: "/invoices?invoice_id={invoice_id}",
      }],
      ai_profile_ids: [analystProfile.id],
    }));
    expect(await screen.findByRole("button", { name: /Billing API/ })).toBeInTheDocument();
    expect(screen.queryByDisplayValue("billing-secret")).not.toBeInTheDocument();
  });

  it("preserves a stored token when an integration is updated without a replacement", async () => {
    vi.mocked(api.updateApiIntegration).mockResolvedValue({
      ...integration,
      description: "Read current order details.",
    });
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /Orders API/ }));
    expect(screen.getByLabelText("Access token")).toHaveValue("");
    expect(screen.getByLabelText("Access token")).toHaveAttribute("placeholder", "Leave blank to keep the stored token");
    expect(screen.queryByDisplayValue("orders-secret")).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Description for AI"), { target: { value: "Read current order details." } });
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }));

    await waitFor(() => expect(api.updateApiIntegration).toHaveBeenCalled());
    const payload = vi.mocked(api.updateApiIntegration).mock.calls[0][2];
    expect(payload).toMatchObject({
      description: "Read current order details.",
      auth_kind: "bearer",
      clear_token: false,
      ai_profile_ids: [supportProfile.id],
    });
    expect(payload).not.toHaveProperty("token");
  });

  it("clears authentication and updates explicit agent assignments", async () => {
    vi.mocked(api.updateApiIntegration).mockResolvedValue({
      ...integration,
      auth_kind: "none",
      token_configured: false,
      ai_profile_ids: [analystProfile.id],
    });
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /Orders API/ }));
    fireEvent.change(screen.getByLabelText("Authentication"), { target: { value: "none" } });
    expect(screen.queryByLabelText("Access token")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /Support agent/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Analyst agent/ }));
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }));

    await waitFor(() => expect(api.updateApiIntegration).toHaveBeenCalledWith(
      auth,
      integration.id,
      expect.objectContaining({
        auth_kind: "none",
        clear_token: true,
        ai_profile_ids: [analystProfile.id],
      }),
    ));
  });

  it("opens the connection named by the URL and keeps the URL current", async () => {
    const created: api.ApiIntegration = { ...integration, id: "00000000-0000-4000-8000-000000000022", name: "Billing API", key: "billing_api", auth_kind: "none", token_configured: false };
    vi.mocked(api.createApiIntegration).mockResolvedValue(created);
    renderRouted([integration.id]);

    expect(await screen.findByLabelText("Connection name")).toHaveValue("Orders API");
    expect(screen.getByRole("button", { name: /Orders API/ })).toHaveAttribute("aria-pressed", "true");
    expect(pageRoute().textContent).toBe(integration.id);
    fireEvent.click(screen.getByRole("button", { name: "New API connection" }));
    expect(screen.getByRole("heading", { name: "Create API connection" })).toBeInTheDocument();
    expect(pageRoute().textContent).toBe("new");
    fireEvent.change(screen.getByLabelText("Connection name"), { target: { value: "Billing API" } });
    fireEvent.change(screen.getByLabelText("Stable key"), { target: { value: "billing_api" } });
    fireEvent.change(screen.getByLabelText("HTTPS base URL"), { target: { value: "https://billing.example.com" } });
    fireEvent.change(screen.getByLabelText("Description for AI"), { target: { value: "Read a customer invoice." } });
    fireEvent.change(screen.getByLabelText("Operation key"), { target: { value: "get_invoice" } });
    fireEvent.change(screen.getByLabelText("Operation name"), { target: { value: "Get invoice" } });
    fireEvent.change(screen.getByLabelText("Operation description"), { target: { value: "Return an invoice." } });
    fireEvent.change(screen.getByLabelText("Path template"), { target: { value: "/invoices/{invoice_id}" } });
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }));

    expect(await screen.findByText("API connection saved.")).toBeInTheDocument();
    expect(pageRoute().textContent).toBe(created.id);
    expect(screen.getByRole("button", { name: /Billing API/ })).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: /Orders API/ }));
    expect(screen.getByLabelText("Connection name")).toHaveValue("Orders API");
    expect(pageRoute().textContent).toBe(integration.id);
  });

  it("follows Back and Forward with the saved connection instead of an unsaved draft", async () => {
    renderRouted([integration.id], [["new"], [integration.id]]);
    fireEvent.change(await screen.findByLabelText("Connection name"), { target: { value: "Unsaved name" } });
    fireEvent.click(screen.getByRole("button", { name: "Open /new" }));
    expect(screen.getByLabelText("Connection name")).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: `Open /${integration.id}` }));
    expect(screen.getByLabelText("Connection name")).toHaveValue("Orders API");
  });

  it.each([[["missing"], ""], [[integration.id, "extra"], integration.id]])("replaces the stale connection URL %j with what it shows", async (segments, canonical) => {
    renderRouted(segments);
    await screen.findByRole("button", { name: /Orders API/ });
    await waitFor(() => expect(pageRoute().textContent).toBe(canonical));
  });

  it("keeps a connection link when connections cannot be loaded", async () => {
    vi.mocked(api.listApiIntegrations).mockRejectedValue(new Error("offline"));
    renderRouted([integration.id]);
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load API connections.");
    expect(pageRoute().textContent).toBe(integration.id);
  });

  it("shows a generic save error without exposing a submitted token", async () => {
    vi.mocked(api.createApiIntegration).mockRejectedValue(new Error("credential rejected: private-token"));
    renderView();

    await screen.findByRole("button", { name: /Orders API/ });
    fireEvent.click(screen.getByRole("button", { name: "New API connection" }));
    fireEvent.change(screen.getByLabelText("Connection name"), { target: { value: "Private API" } });
    fireEvent.change(screen.getByLabelText("Stable key"), { target: { value: "private_api" } });
    fireEvent.change(screen.getByLabelText("HTTPS base URL"), { target: { value: "https://private.example.com" } });
    fireEvent.change(screen.getByLabelText("Description for AI"), { target: { value: "Read private records." } });
    fireEvent.change(screen.getByLabelText("Authentication"), { target: { value: "x_api_key" } });
    fireEvent.change(screen.getByLabelText("Access token"), { target: { value: "private-token" } });
    fireEvent.change(screen.getByLabelText("Operation key"), { target: { value: "get_record" } });
    fireEvent.change(screen.getByLabelText("Operation name"), { target: { value: "Get record" } });
    fireEvent.change(screen.getByLabelText("Operation description"), { target: { value: "Return one record." } });
    fireEvent.change(screen.getByLabelText("Path template"), { target: { value: "/records/{record_id}" } });
    fireEvent.click(screen.getByRole("button", { name: "Save connection" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not save the API connection.");
    expect(alert).not.toHaveTextContent("private-token");
    expect(document.body).not.toHaveTextContent("private-token");
  });
});
