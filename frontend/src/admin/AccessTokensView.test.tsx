import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { I18nContext, createI18n } from "../i18n";
import { AccessTokensView } from "./AccessTokensView";

const edition = vi.hoisted(() => ({
  isLiteEdition: false,
  get productNamespace() { return this.isLiteEdition ? "tz" : "tzomet"; },
}));
vi.mock("../product-edition", () => edition);

vi.mock("../api", () => ({
  createAccessToken: vi.fn(),
  deleteAccessToken: vi.fn(),
  listAccessTokenManagement: vi.fn(),
  listProjects: vi.fn(),
  listRoles: vi.fn(),
  revokeAccessToken: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const role: api.RoleDefinition = {
  id: "operator",
  name: "Operator",
  base_role: "operator",
  permissions: ["projects:read", "ai:manage"],
  access_token_permissions: ["projects:read", "ai:manage"],
  is_system: false,
  member_count: 0,
  active_token_count: 1,
};
const projects: api.Project[] = [
  { id: "project-one", name: "Support", slug: "support", status: "active", role: "admin", current: true, inbox_count: 1, conversation_count: 0, channel_count: 0, team_count: 0, member_count: 1, created_at: "2026-09-11", updated_at: "2026-09-11" },
  { id: "project-two", name: "Sales", slug: "sales", status: "active", role: "admin", current: false, inbox_count: 1, conversation_count: 0, channel_count: 0, team_count: 0, member_count: 1, created_at: "2026-09-11", updated_at: "2026-09-11" },
];
const token: api.AccessToken = {
  id: "019d0000-0000-7000-8000-000000000001",
  name: "AI contractor",
  role: "operator",
  role_id: "operator",
  role_name: "Operator",
  permissions: ["projects:read", "ai:manage"],
  project_scope: "project",
  project_id: "project-one",
  role_project_id: "project-one",
  inbox_scope: null,
  expires_at: "2099-08-19T12:00:00Z",
  revoked_at: null,
  last_used_at: null,
  created_at: "2026-08-19T12:00:00Z",
};

function renderTokens() {
  return render(<I18nContext.Provider value={createI18n("en", vi.fn())}>
    <AccessTokensView auth={auth} projectId="project-one" />
  </I18nContext.Provider>);
}

describe("AccessTokensView", () => {
  beforeEach(() => {
    edition.isLiteEdition = false;
    vi.mocked(api.listRoles).mockResolvedValue([role]);
    vi.mocked(api.listAccessTokenManagement).mockResolvedValue({ items: [token], can_create_all_projects: true });
    vi.mocked(api.listProjects).mockResolvedValue(projects);
    vi.mocked(api.createAccessToken).mockResolvedValue({ ...token, id: "created-token", name: "New token", secret: "one-time-secret" });
    vi.mocked(api.revokeAccessToken).mockResolvedValue();
    vi.mocked(api.deleteAccessToken).mockResolvedValue();
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.restoreAllMocks();
  });

  it("issues Lite tokens for the fixed workspace without project controls", async () => {
    edition.isLiteEdition = true;
    renderTokens();
    await screen.findByText("AI contractor");
    expect(screen.queryByRole("combobox", { name: "Project" })).not.toBeInTheDocument();
    expect(screen.queryByText("All projects")).not.toBeInTheDocument();
    const row = screen.getByText("AI contractor").closest("tr")!;
    expect(within(row).getByText("Operator workspace")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Support access" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));
    await waitFor(() => expect(api.createAccessToken).toHaveBeenCalledWith(auth, {
      name: "Support access", role_id: "operator", expires_in_days: 30, project_scope: "project", project_id: "project-one",
    }));
  });

  it("excludes administrator roles while allowing an assignable system operator role", async () => {
    vi.mocked(api.listRoles).mockResolvedValue([
      { ...role, id: "admin", name: "Administrator", base_role: "admin", is_system: true },
      { ...role, is_system: true },
    ]);
    renderTokens();
    await screen.findByText("AI contractor");
    const selector = screen.getByRole("combobox", { name: "Role" });
    expect(selector).toHaveValue("operator");
    expect(within(selector).queryByRole("option", { name: "Administrator" })).not.toBeInTheDocument();
    expect(within(selector).getByRole("option", { name: "Operator" })).toBeInTheDocument();
  });

});
