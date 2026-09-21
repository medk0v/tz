import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { I18nContext, createI18n } from "../i18n";
import { RolesView } from "./RolesView";

const edition = vi.hoisted(() => ({
  isLiteEdition: false,
  get productNamespace() { return this.isLiteEdition ? "tz" : "tzomet"; },
}));
vi.mock("../product-edition", () => edition);

vi.mock("../api", () => ({
  listRoles: vi.fn(),
  createRole: vi.fn(),
  deleteRole: vi.fn(),
  updateRole: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const roles: api.RoleDefinition[] = [
  {
    id: "admin",
    name: "Administrator",
    base_role: "admin",
    permissions: [
      "projects:read",
      "projects:manage",
      "conversations:read",
      "conversations:reply",
      "conversations:close",
      "contacts:read",
      "channels:read",
      "channels:manage",
      "access_tokens:manage",
      "visitor_network:read",
      "quality:read",
      "quality:read_all",
    ],
    access_token_permissions: [
      "projects:read",
      "conversations:read",
      "conversations:reply",
      "conversations:close",
      "contacts:read",
      "channels:read",
      "channels:manage",
    ],
    is_system: true,
    member_count: 1,
    active_token_count: 0,
  },
  {
    id: "manager",
    name: "Manager",
    base_role: "manager",
    permissions: ["projects:read", "conversations:read", "channels:read", "quality:read", "quality:read_all"],
    access_token_permissions: ["projects:read", "conversations:read", "channels:read", "quality:read", "quality:read_all"],
    is_system: false,
    member_count: 0,
    active_token_count: 0,
  },
  {
    id: "operator",
    name: "Operator",
    base_role: "operator",
    permissions: ["projects:read", "conversations:read", "conversations:reply"],
    access_token_permissions: ["projects:read", "conversations:read", "conversations:reply"],
    is_system: false,
    member_count: 0,
    active_token_count: 0,
  },
];

describe("RolesView", () => {
  beforeEach(() => {
    edition.isLiteEdition = false;
    vi.mocked(api.listRoles).mockResolvedValue(roles);
    vi.mocked(api.createRole).mockResolvedValue(roles[2]);
    vi.mocked(api.deleteRole).mockResolvedValue();
    vi.mocked(api.updateRole).mockImplementation(async (_auth, roleId, input) => ({
      ...roles.find((item) => item.id === roleId)!,
      ...input,
      access_token_permissions: input.permissions,
    }));
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("updates an editable role and keeps visitor network access available to tokens", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <RolesView auth={auth} />
      </I18nContext.Provider>,
    );

    const visitorCheckboxes = await screen.findAllByRole("checkbox", { name: /View visitor network data/ });
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /View reviews/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /View system status/ })).not.toBeInTheDocument();
    const operatorVisitorCheckbox = visitorCheckboxes.at(-1)!;
    fireEvent.click(operatorVisitorCheckbox);
    expect(within(operatorVisitorCheckbox.parentElement!).queryByText("Password session only")).not.toBeInTheDocument();
    fireEvent.click(screen.getAllByRole("button", { name: "Save role" }).at(-1)!);

    await waitFor(() => expect(api.updateRole).toHaveBeenCalledWith(
      auth,
      "operator",
      {
        name: "Operator",
        base_role: "operator",
        permissions: ["projects:read", "conversations:read", "conversations:reply", "visitor_network:read"],
      },
    ));
  });

  it("grants note reading with editing and revokes editing when reading is removed", async () => {
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><RolesView auth={auth} /></I18nContext.Provider>);
    const edit = (await screen.findAllByRole("checkbox", { name: /^Edit notes/ })).at(-1)!;
    const read = screen.getAllByRole("checkbox", { name: /^View notes/ }).at(-1)!;
    fireEvent.click(edit);
    expect(read).toBeChecked();
    fireEvent.click(read);
    expect(edit).not.toBeChecked();
    expect(screen.getAllByRole("button", { name: "Save role" }).at(-1)!).toBeDisabled();
  });

  it("offers no process permissions in Lite", async () => {
    edition.isLiteEdition = true;
    render(<I18nContext.Provider value={createI18n("ru", vi.fn())}><RolesView auth={auth} /></I18nContext.Provider>);
    expect((await screen.findAllByRole("checkbox", { name: /^Просмотр диалогов/ })).length).toBeGreaterThan(0);
    expect(screen.queryByRole("checkbox", { name: /процесс/i })).not.toBeInTheDocument();
  });

  it("removes dependent conversation actions when conversation reading is disabled", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <RolesView auth={auth} />
      </I18nContext.Provider>,
    );

    const readCheckboxes = await screen.findAllByRole("checkbox", { name: /View conversations/ });
    fireEvent.click(readCheckboxes.at(-1)!);
    fireEvent.click(screen.getAllByRole("button", { name: "Save role" }).at(-1)!);

    await waitFor(() => expect(api.updateRole).toHaveBeenCalledWith(
      auth,
      "operator",
      { name: "Operator", base_role: "operator", permissions: ["projects:read"] },
    ));
  });

  it("controls project-wide quality visibility with an explicit permission", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <RolesView auth={auth} />
      </I18nContext.Provider>,
    );

    const allQualityCheckboxes = await screen.findAllByRole("checkbox", { name: /View all support quality/ });
    fireEvent.click(allQualityCheckboxes.at(-1)!);
    fireEvent.click(screen.getAllByRole("button", { name: "Save role" }).at(-1)!);

    await waitFor(() => expect(api.updateRole).toHaveBeenCalledWith(
      auth,
      "operator",
      {
        name: "Operator",
        base_role: "operator",
        permissions: [
          "projects:read",
          "conversations:read",
          "conversations:reply",
          "quality:read",
          "quality:read_all",
        ],
      },
    ));
  });

  it("removes project-wide quality access with its base permission", async () => {
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <RolesView auth={auth} />
      </I18nContext.Provider>,
    );

    const ownQualityCheckboxes = await screen.findAllByRole("checkbox", { name: /View own support quality/ });
    fireEvent.click(ownQualityCheckboxes[1]);
    fireEvent.click(screen.getAllByRole("button", { name: "Save role" })[0]);

    await waitFor(() => expect(api.updateRole).toHaveBeenCalledWith(
      auth,
      "manager",
      {
        name: "Manager",
        base_role: "manager",
        permissions: ["projects:read", "conversations:read", "channels:read"],
      },
    ));
  });

  it("keeps blacklist management dependent on contact reading", async () => {
    render(<I18nContext.Provider value={createI18n("en", vi.fn())}><RolesView auth={auth} /></I18nContext.Provider>);
    const management = (await screen.findAllByRole("checkbox", { name: /Manage contact blacklist/ })).at(-1)!;
    const reading = screen.getAllByRole("checkbox", { name: /View contacts/ }).at(-1)!;
    fireEvent.click(management);
    expect(management).toBeChecked();
    expect(reading).toBeChecked();
    fireEvent.click(reading);
    expect(management).not.toBeChecked();
    expect(reading).not.toBeChecked();
  });

  it("creates and deletes unused roles", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(
      <I18nContext.Provider value={createI18n("en", vi.fn())}>
        <RolesView auth={auth} />
      </I18nContext.Provider>,
    );

    fireEvent.change(await screen.findByPlaceholderText("Senior support"), { target: { value: "Night shift" } });
    fireEvent.click(screen.getByRole("button", { name: "Create role" }));
    await waitFor(() => expect(api.createRole).toHaveBeenCalledWith(auth, {
      name: "Night shift",
      base_role: "operator",
      permissions: [
        "projects:read",
        "conversations:read",
        "conversations:reply",
        "conversations:close",
        "contacts:read",
        "contacts:manage",        "channels:read",
        "quality:read",
        "tasks:own",
      ],
    }));

    fireEvent.click(screen.getByRole("button", { name: "Delete role Manager" }));
    await waitFor(() => expect(api.deleteRole).toHaveBeenCalledWith(auth, "manager"));
    confirm.mockRestore();
  });
});
