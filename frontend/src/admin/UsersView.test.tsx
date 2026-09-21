import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import * as departmentApi from "../department-api";
import { createI18n, I18nContext } from "../i18n";
import { UsersView } from "./UsersView";

const edition = vi.hoisted(() => ({
  isLiteEdition: false,
  get productNamespace() { return this.isLiteEdition ? "tz" : "tzomet"; },
}));
vi.mock("../product-edition", () => edition);

vi.mock("../api", () => ({
  ApiRequestError: class extends Error {
    constructor(message: string, public status: number) { super(message); }
  },
  createProjectUser: vi.fn(), grantProjectMember: vi.fn(), listProjectMembers: vi.fn(),
  listRoles: vi.fn(), revokeProjectMember: vi.fn(), updateProjectMember: vi.fn(),
  resolveAvatarUrl: (value: string | null) => value,
}));
vi.mock("../department-api", () => ({ listDepartments: vi.fn() }));
vi.mock("./OperatorProfileDialog", () => ({ OperatorProfileDialog: () => null }));

const auth: api.OperatorAuth = { kind: "session" };
const roles: api.RoleDefinition[] = [
  { id: "admin", name: "Administrator", base_role: "admin", permissions: ["projects:manage"], access_token_permissions: [], is_system: true, member_count: 1, active_token_count: 0 },
  { id: "operator", name: "Operator", base_role: "operator", permissions: ["conversations:read"], access_token_permissions: [], is_system: true, member_count: 1, active_token_count: 0 },
];
const admin: api.ProjectMember = {
  membership_id: "membership-admin", user_id: "actor", email: "admin@example.com", display_name: "Admin", chat_display_name: "Admin", avatar_url: null,
  role: "admin", role_id: "admin", role_name: "Administrator", department_id: null, director_access: true, created_at: "2026-09-11", updated_at: "2026-09-11",
};
const user: api.ProjectMember = {
  ...admin, membership_id: "membership-user", user_id: "user", email: "user@example.com", display_name: "New User", chat_display_name: "New User",
  role: "operator", role_id: "operator", role_name: "Operator", director_access: false,
};

function renderUsers(authOverride = auth) {
  const onChanged = vi.fn();
  const onManageRoles = vi.fn();
  const result = render(<I18nContext.Provider value={createI18n("en", vi.fn())}>
    <UsersView auth={authOverride} actorId="actor" projectId="project" onChanged={onChanged} onManageRoles={onManageRoles} />
  </I18nContext.Provider>);
  return { ...result, onChanged, onManageRoles };
}

async function fillNewUser(password = "safe-password") {
  await screen.findByText("Admin", { selector: "strong" });
  fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: " New User " } });
  fireEvent.change(screen.getByRole("textbox", { name: "User email" }), { target: { value: "user@example.com" } });
  fireEvent.change(screen.getByLabelText("Password", { exact: true }), { target: { value: password } });
}

describe("UsersView", () => {
  beforeEach(() => {
    edition.isLiteEdition = false;
    vi.mocked(api.listProjectMembers).mockResolvedValue([admin]);
    vi.mocked(api.listRoles).mockResolvedValue(roles);
    vi.mocked(departmentApi.listDepartments).mockResolvedValue({
      items: [{ id: "support", name: "Support", icon: "support", sidebar_items: [], default_page: "conversations", position: 0, inbox_ids: [], member_count: 0, show_default_channels: true }],
      director_enabled: true, default_department_id: "support", can_access_director: true,
    });
    vi.mocked(api.createProjectUser).mockResolvedValue(user);
    vi.mocked(api.grantProjectMember).mockResolvedValue(user);
    vi.mocked(api.updateProjectMember).mockImplementation(async (_auth, _projectId, _membershipId, patch) => ({ ...user, ...(typeof patch === "string" ? { role_id: patch } : patch) }));
    vi.mocked(api.revokeProjectMember).mockResolvedValue();
  });

  afterEach(() => { cleanup(); vi.clearAllMocks(); });

  it("creates a user with operator access, adds them to the list and clears the password", async () => {
    const { onChanged, onManageRoles } = renderUsers();
    await fillNewUser();
    expect(screen.getByRole("combobox", { name: "Role in project" })).toHaveValue("operator");
    fireEvent.click(screen.getByRole("button", { name: "Create user" }));

    await waitFor(() => expect(api.createProjectUser).toHaveBeenCalledWith(auth, "project", {
      display_name: "New User", email: "user@example.com", password: "safe-password", role_id: "operator", department_id: null, director_access: false,
    }));
    expect(await screen.findByText("New User", { selector: "strong" })).toBeInTheDocument();
    expect(screen.getByLabelText("Password", { exact: true })).toHaveValue("");
    expect(onChanged).toHaveBeenCalledOnce();
    expect(api.grantProjectMember).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Roles and permissions" }));
    expect(onManageRoles).toHaveBeenCalledOnce();
  });

  it("grants existing accounts access without sending a password", async () => {
    renderUsers();
    await screen.findByText("Admin", { selector: "strong" });
    fireEvent.click(screen.getByRole("radio", { name: "Existing account" }));
    expect(screen.queryByLabelText("Password", { exact: true })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "User email" }), { target: { value: "user@example.com" } });
    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));
    await waitFor(() => expect(api.grantProjectMember).toHaveBeenCalledWith(auth, "project", {
      email: "user@example.com", role_id: "operator", department_id: null, director_access: false,
    }));
    expect(api.createProjectUser).not.toHaveBeenCalled();
  });

  it("requires an explicit administrator choice and applies full project access", async () => {
    vi.mocked(api.listRoles).mockResolvedValue([roles[0]]);
    renderUsers();
    await fillNewUser();
    const role = screen.getByRole("combobox", { name: "Role in project" });
    expect(role).toHaveValue("");
    expect(screen.getByRole("button", { name: "Create user" })).toBeDisabled();
    fireEvent.change(role, { target: { value: "admin" } });
    fireEvent.click(screen.getByRole("button", { name: "Create user" }));
    await waitFor(() => expect(api.createProjectUser).toHaveBeenCalledWith(auth, "project", expect.objectContaining({ role_id: "admin", department_id: null, director_access: true })));
  });

  it("explains a duplicate account and keeps the form ready to grant existing access", async () => {
    vi.mocked(api.createProjectUser).mockRejectedValue(new api.ApiRequestError("Email already exists", 409));
    renderUsers();
    await fillNewUser();
    fireEvent.click(screen.getByRole("button", { name: "Create user" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("An account with this email already exists.");
    expect(screen.queryByText("New User", { selector: "strong" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "Existing account" }));
    expect(screen.getByRole("textbox", { name: "User email" })).toHaveValue("user@example.com");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("enforces password character and UTF-8 byte limits before creation", async () => {
    renderUsers();
    await fillNewUser("a".repeat(7));
    expect(screen.getByRole("button", { name: "Create user" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Password", { exact: true }), { target: { value: "я".repeat(65) } });
    expect(screen.getByRole("button", { name: "Create user" })).toBeDisabled();
    expect(screen.getByText("This password is too long. Use a shorter password.")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Password", { exact: true }), { target: { value: "a".repeat(128) } });
    expect(screen.getByRole("button", { name: "Create user" })).toBeEnabled();
    expect(api.createProjectUser).not.toHaveBeenCalled();
  });

  it("updates and revokes another member while protecting the signed-in administrator", async () => {
    vi.mocked(api.listProjectMembers).mockResolvedValue([admin, user]);
    renderUsers();
    await screen.findByText("New User", { selector: "strong" });
    const adminRow = screen.getByText("Admin", { selector: "strong" }).closest("tr")!;
    expect(within(adminRow).getByRole("combobox", { name: "Change role for Admin" })).toBeDisabled();
    expect(within(adminRow).getByRole("button", { name: "Revoke access for Admin" })).toBeDisabled();
    const revoke = screen.getByRole("button", { name: "Revoke access for New User" });
    await waitFor(() => expect(revoke).toBeEnabled());
    fireEvent.click(revoke);
    await waitFor(() => expect(api.revokeProjectMember).toHaveBeenCalledWith(auth, "project", "membership-user"));
    await waitFor(() => expect(screen.queryByText("New User", { selector: "strong" })).not.toBeInTheDocument());
  });

  it("retries a failed member load without enabling account creation", async () => {
    vi.mocked(api.listProjectMembers).mockRejectedValueOnce(new Error("Offline"));
    renderUsers();
    const retry = await screen.findByRole("button", { name: "Try again" });
    expect(screen.getByRole("button", { name: "Create user" })).toBeDisabled();
    fireEvent.click(retry);
    expect(await screen.findByText("Admin", { selector: "strong" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("hides department and director access in Lite while creating users with the existing defaults", async () => {
    edition.isLiteEdition = true;
    renderUsers();
    await fillNewUser();
    expect(screen.queryByText(/Department access/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Control center access/)).not.toBeInTheDocument();
    expect(screen.getByText("The selected role determines the user's permissions.")).toBeInTheDocument();
    expect(screen.queryByText(/Department access limits/)).not.toBeInTheDocument();
    expect(screen.getAllByRole("columnheader")).toHaveLength(3);
    expect(within(screen.getByText("Admin", { selector: "strong" }).closest("tr")!).getAllByRole("cell")).toHaveLength(3);
    fireEvent.click(screen.getByRole("button", { name: "Create user" }));
    await waitFor(() => expect(api.createProjectUser).toHaveBeenCalledWith(auth, "project", {
      display_name: "New User", email: "user@example.com", password: "safe-password", role_id: "operator", department_id: null, director_access: false,
    }));
  });

  it("preserves hidden department access when changing a non-admin role in Lite", async () => {
    edition.isLiteEdition = true;
    vi.mocked(api.listProjectMembers).mockResolvedValue([admin, { ...user, department_id: "support" }]);
    vi.mocked(api.listRoles).mockResolvedValue([...roles, { ...roles[1], id: "custom", name: "Custom operator" }]);
    renderUsers();
    await screen.findByText("New User", { selector: "strong" });
    expect(screen.queryByRole("combobox", { name: /Department access/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /Control center access/ })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("combobox", { name: "Change role for New User" }), { target: { value: "custom" } });
    await waitFor(() => expect(api.updateProjectMember).toHaveBeenCalledWith(auth, "project", "membership-user", { role_id: "custom" }));
  });

  it("keeps member update errors visible in Lite after hiding the department column", async () => {
    edition.isLiteEdition = true;
    vi.mocked(api.listProjectMembers).mockResolvedValue([admin, user]);
    vi.mocked(api.updateProjectMember).mockRejectedValue(new Error("Access update failed"));
    renderUsers();
    await screen.findByText("New User", { selector: "strong" });
    fireEvent.change(screen.getByRole("combobox", { name: "Change role for New User" }), { target: { value: "admin" } });
    const row = screen.getByText("New User", { selector: "strong" }).closest("tr")!;
    expect(await within(row).findByRole("alert")).toHaveTextContent("Access update failed");
    expect(within(row).getByRole("combobox", { name: "Change role for New User" })).toHaveAccessibleDescription("Access update failed");
  });
});
