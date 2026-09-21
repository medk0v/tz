import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import { ParentProjectWorkspace, type ProjectWorkspaceScope } from "./ParentProjectWorkspace";
import { ContactsView } from "./DirectoryViews";
import { productNamespace } from "../product-edition";

vi.mock("../api", async (original) => ({ ...await original<typeof api>(),
  listProjects: vi.fn(), getCurrentActor: vi.fn(), listInboxes: vi.fn(), listContacts: vi.fn(), listChannels: vi.fn().mockResolvedValue([]),
}));
vi.mock("./useDepartmentWorkspace", () => ({ useDepartmentWorkspace: () => ({ data: null, loading: false, error: null, reload: vi.fn() }) }));
vi.mock("./useOperatorRealtime", () => ({ useOperatorRealtime: vi.fn() }));
const auth: api.OperatorAuth = { kind: "session", projectId: "parent" };
const actor = (id: string): api.ActorContext => ({ actor_id: "user", tenant_id: "tenant", project_id: id, role: "admin", chat_display_name: "Admin", avatar_url: null, auth_method: "session", permissions: ["projects:read", "contacts:read", "conversations:read"], inbox_scope: null });
const parentActor = actor("parent");
const project = (id: string, parent: string | null = null) => ({ id, name: id, parent_project_id: parent, status: "active" } as api.Project);
const navigate = vi.fn(() => true);
const dirty = vi.fn();
const scopes = new Map<string, ProjectWorkspaceScope>();
let scopeContainer: HTMLElement;
const canShowPage = (value: api.ActorContext) => value.permissions.includes("contacts:read");
const renderWorkspace = (scope?: ProjectWorkspaceScope) => {
  if (scope) scopes.set(scope.actor.project_id!, scope);
  return <div>Data for {scope?.actor.project_id ?? "standalone"}</div>;
};
function workspace(overrides: Partial<React.ComponentProps<typeof ParentProjectWorkspace>> = {}) {
  return (<I18nContext.Provider value={createI18n("en", vi.fn())}>
    <main className="content" data-page-width="full"><ParentProjectWorkspace scopeContainer={scopeContainer} projects={[project("parent"), project("child", "parent"), project("sibling"), { ...project("archived", "parent"), status: "disabled" }]} enabled auth={auth} actor={parentActor} page="contacts" canShowPage={canShowPage}
      onNavigate={navigate} onDepartmentSelect={async () => {}} onBeforeChange={() => true} onDirtyChange={dirty} renderWorkspace={renderWorkspace} {...overrides} /></main>
  </I18nContext.Provider>);
}
function show(overrides: Partial<React.ComponentProps<typeof ParentProjectWorkspace>> = {}) { return render(workspace(overrides)); }
function selectProject(value: string) {
  const trigger = screen.getByRole("button", { name: "Show data" });
  if (trigger.getAttribute("aria-expanded") !== "true") fireEvent.click(trigger);
  if (value === "all") {
    const checkbox = screen.getByRole("checkbox", { name: "Current and subprojects" });
    if (!(checkbox as HTMLInputElement).checked) fireEvent.click(checkbox);
  } else {
    const label = value === "parent" ? "Current project" : value;
    for (const checkbox of within(screen.getByRole("dialog", { name: "Show data" })).getAllByRole("checkbox")) {
      if (checkbox === screen.getByRole("checkbox", { name: "Current and subprojects" })) continue;
      const shouldCheck = checkbox === screen.getByRole("checkbox", { name: label });
      if ((checkbox as HTMLInputElement).checked !== shouldCheck) fireEvent.click(checkbox);
    }
  }
  fireEvent.keyDown(trigger, { key: "Escape" });
}
const storageKey = (projectId = "parent", userId = "user", page = "contacts") => `${productNamespace}-admin-project-scope:tenant:${userId}:${projectId}:${page}`;
beforeEach(() => {
  vi.clearAllMocks(); scopes.clear(); localStorage.clear();
  scopeContainer = document.createElement("aside");
  scopeContainer.setAttribute("aria-label", "Sidebar");
  document.body.append(scopeContainer);
  vi.mocked(api.listProjects).mockResolvedValue([project("parent"), project("child", "parent"), project("sibling"), project("archived", "parent")].map((p) => p.id === "archived" ? { ...p, status: "disabled" } : p));
  vi.mocked(api.getCurrentActor).mockImplementation(async (value) => actor(value.projectId!));
  vi.mocked(api.listInboxes).mockImplementation(async (value) => [{ id: `${value.projectId}-inbox`, project_id: value.projectId!, name: "Inbox", status: "active", created_at: "2026-01-01" }]);
});
afterEach(() => { cleanup(); scopeContainer.remove(); vi.restoreAllMocks(); });
it("defaults to the current project without loading children", async () => {
  show();
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Current project");
  expect(await screen.findByText("Data for parent")).toBeVisible();
  expect(screen.queryByRole("region", { name: "child" })).not.toBeInTheDocument();
  expect(api.getCurrentActor).not.toHaveBeenCalled();
  expect(localStorage.getItem(storageKey())).toBeNull();
});
it("places the selector in the sidebar and keeps a single page directly in the content area", async () => {
  show({ renderWorkspace: (scope) => <section className="page">Page for {scope?.actor.project_id}</section> });
  await screen.findByText("Page for parent");
  expect(within(scopeContainer).getByRole("button", { name: "Show data" })).toBeVisible();
  expect(within(screen.getByRole("main")).queryByRole("button", { name: "Show data" })).not.toBeInTheDocument();
  expect(screen.getByText("Page for parent").parentElement).toBe(screen.getByRole("main"));
  expect(screen.queryByRole("heading", { name: "parent" })).not.toBeInTheDocument();
  selectProject("child");
  expect((await screen.findByText("Page for child")).parentElement).toBe(screen.getByRole("main"));
  selectProject("all");
  expect(await screen.findByRole("region", { name: "parent" })).toBeVisible();
  expect(await screen.findByRole("region", { name: "child" })).toBeVisible();
});
it.each(["child", "all"])("remembers the manually selected %s when reopening the same page", async (selection) => {
  const view = show();
  selectProject(selection);
  await screen.findByText("Data for child");
  expect(localStorage.getItem(storageKey())).toBe(selection);
  view.unmount();
  show();
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent(selection === "all" ? "Current and subprojects" : selection);
  expect(await screen.findByText("Data for child")).toBeVisible();
});
it("remembers a separate selection for each page and defaults new pages to the current project", async () => {
  const view = show();
  selectProject("all");
  await screen.findByText("Data for child");
  view.rerender(workspace({ page: "notes" }));
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Current project");
  selectProject("child");
  await screen.findByText("Data for child");
  view.rerender(workspace({ page: "contacts" }));
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Current and subprojects");
  view.rerender(workspace({ page: "notes" }));
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("child");
  expect(localStorage.getItem(storageKey())).toBe("all");
  expect(localStorage.getItem(storageKey("parent", "user", "notes"))).toBe("child");
  view.unmount();
  show({ page: "notes" });
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("child");
  expect(await screen.findByText("Data for child")).toBeVisible();
});
it("keeps preferences separate for each current project and user", async () => {
  const view = show();
  selectProject("child");
  await screen.findByText("Data for child");
  view.rerender(workspace({ auth: { kind: "session", projectId: "other" }, actor: actor("other"), projects: [project("other"), project("other-child", "other")] }));
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Current project");
  view.rerender(workspace({ actor: { ...parentActor, actor_id: "second-user" } }));
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Current project");
  view.rerender(workspace());
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("child");
});
it("falls back to the current project when the saved child is no longer available", async () => {
  localStorage.setItem(storageKey(), "archived");
  show();
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Current project");
  expect(await screen.findByText("Data for parent")).toBeVisible();
  expect(api.getCurrentActor).not.toHaveBeenCalled();
});
it("does not change or persist the selection when unsaved edits prevent switching", async () => {
  show({ onBeforeChange: () => false });
  selectProject("all");
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Current project");
  expect(localStorage.getItem(storageKey())).toBeNull();
  expect(await screen.findByText("Data for parent")).toBeVisible();
  fireEvent.click(screen.getByRole("button", { name: "Show data" }));
  expect(screen.getByRole("checkbox", { name: "Current and subprojects" })).toBePartiallyChecked();
  fireEvent.click(screen.getByRole("checkbox", { name: "Current and subprojects" }));
  expect(screen.getByRole("checkbox", { name: "Current and subprojects" })).toBePartiallyChecked();
  expect(screen.getByRole("checkbox", { name: "Current project" })).toBeChecked();
});
it("still allows selection when browser storage is unavailable", async () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => { throw new Error("Storage blocked"); });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("Storage blocked"); });
  show();
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Current project");
  selectProject("child");
  expect(await screen.findByText("Data for child")).toBeVisible();
});
it("groups own and child data, excludes unrelated and archived projects, and filters either project", async () => {
  show();
  selectProject("all");
  await screen.findByText("Data for child");
  expect(within(screen.getByRole("region", { name: "parent" })).getByText("Data for parent")).toBeVisible();
  expect(screen.queryByText("Data for sibling")).not.toBeInTheDocument();
  expect(api.getCurrentActor).toHaveBeenCalledExactlyOnceWith({ kind: "session", projectId: "child" });
  selectProject("child");
  expect(screen.queryByRole("region", { name: "parent" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open project" }));
  expect(navigate).toHaveBeenLastCalledWith("contacts", "child");
  selectProject("parent");
  expect(await screen.findByText("Data for parent")).toBeVisible();
});
it("keeps a subproject isolated from its parent and siblings", async () => {
  const childAuth: api.OperatorAuth = { kind: "session", projectId: "child" };
  show({ auth: childAuth, actor: actor("child") });
  await act(async () => {});
  expect(screen.getByText("Data for standalone")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Show data" })).not.toBeInTheDocument();
  expect(api.getCurrentActor).not.toHaveBeenCalled();
});
it("uses each child's permissions and reports failed authorization without showing its data", async () => {
  vi.mocked(api.getCurrentActor).mockRejectedValue(new Error("Forbidden"));
  show();
  selectProject("all");
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not load projects");
  expect(screen.queryByText("Data for child")).not.toBeInTheDocument();
  expect(await screen.findByText("Data for parent")).toBeVisible();
});
it("does not render a child module without its own permission", async () => {
  vi.mocked(api.getCurrentActor).mockResolvedValue({ ...actor("child"), permissions: [] });
  show();
  selectProject("all");
  await screen.findByText("Data for parent");
  await waitFor(() => expect(api.getCurrentActor).toHaveBeenCalled());
  expect(screen.queryByText("Data for child")).not.toBeInTheDocument();
});
it("aggregates unsaved edits across projects and sends cross-page navigation to the owning project", async () => {
  show(); selectProject("all"); await screen.findByText("Data for child");
  act(() => { scopes.get("parent")!.dirtyChange("templates", true); scopes.get("child")!.dirtyChange("templates", true); scopes.get("parent")!.dirtyChange("templates", false); });
  expect(dirty).toHaveBeenLastCalledWith("templates", true);
  act(() => scopes.get("child")!.dirtyChange("templates", false));
  expect(dirty).toHaveBeenLastCalledWith("templates", false);
  scopes.get("child")!.navigate("conversations", undefined, ["child-inbox", "conversation"]);
  expect(navigate).toHaveBeenLastCalledWith("conversations", "child", ["child-inbox", "conversation"]);
});
it("keeps contacts and statistics scoped in the real contact views with independent tab focus", async () => {
  vi.mocked(api.listContacts).mockImplementation(async (value) => ({ items: [{ id: value.projectId!, project_id: value.projectId!, display_name: `Contact ${value.projectId}`, channel_kinds: [], last_activity_at: "2026-09-20T10:00:00Z", is_blocked: false, email: null, created_at: "2026-09-20T10:00:00Z", updated_at: "2026-09-20T10:00:00Z", browser_timezone: null, client_ip: null, geo_ip: null, user_agent_details: null }], page: 1, per_page: 10, total: 1, total_pages: 1, statistics: { contacts: 1, countries: [] } as unknown as api.ContactStatistics }));
  show({ renderWorkspace: (scope) => <ContactsView auth={scope?.auth ?? auth} /> });
  selectProject("all");
  await screen.findByText("Contact child");
  expect(screen.getByText("Contact parent")).toBeVisible();
  const childRegion = screen.getByRole("region", { name: "child" });
  fireEvent.keyDown(within(childRegion).getByRole("tab", { name: "Contacts" }), { key: "ArrowRight" });
  expect(within(childRegion).getByRole("tab", { name: "Statistics" })).toHaveFocus();
  expect(within(screen.getByRole("region", { name: "parent" })).getByRole("tab", { name: "Contacts" })).toHaveAttribute("aria-selected", "true");
});
it("leaves disabled editions and standalone workspaces unchanged", () => {
  show({ enabled: false });
  expect(screen.getByText("Data for standalone")).toBeVisible();
  expect(api.listProjects).not.toHaveBeenCalled();
});

it("loads available channels in each project even without conversations", async () => {
  const readChannels = { ...parentActor, permissions: [...parentActor.permissions, "channels:read"] };
  vi.mocked(api.getCurrentActor).mockImplementation(async (value) => ({ ...actor(value.projectId!), permissions: readChannels.permissions }));
  vi.mocked(api.listChannels).mockImplementation(async (value) => [{ id: `${value.projectId}-channel`, name: "Empty widget", kind: "widget", project_id: value.projectId!, inbox_id: `${value.projectId}-inbox` } as api.ChannelConnection]);
  show({ page: "conversations", actor: readChannels });
  selectProject("all");
  await screen.findByText("Data for child");
  expect(scopes.get("parent")!.channels?.map((channel) => channel.id)).toEqual(["parent-channel"]);
  expect(scopes.get("child")!.channels?.map((channel) => channel.id)).toEqual(["child-channel"]);
});

it("selects and remembers a subset of children while keeping the picker open", async () => {
  const projects = [project("parent"), project("alpha", "parent"), project("beta", "parent"), project("gamma", "parent")];
  const view = show({ projects });
  await screen.findByText("Data for parent");
  fireEvent.click(screen.getByRole("button", { name: "Show data" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Current project" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "alpha" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "beta" }));
  expect(screen.getByRole("dialog", { name: "Show data" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Show data" })).toHaveTextContent("Selected projects: 2");
  expect(screen.getByRole("checkbox", { name: "Current and subprojects" })).toBePartiallyChecked();
  expect(await screen.findByText("Data for beta")).toBeVisible();
  expect(await screen.findByText("Data for alpha")).toBeVisible();
  expect(screen.queryByText("Data for parent")).not.toBeInTheDocument();
  expect(api.getCurrentActor).not.toHaveBeenCalledWith(expect.objectContaining({ projectId: "gamma" }));
  expect(localStorage.getItem(storageKey())).toBe(JSON.stringify(["alpha", "beta"]));
  view.unmount();
  show({ projects });
  expect(await screen.findByText("Data for alpha")).toBeVisible();
  expect(await screen.findByText("Data for beta")).toBeVisible();
  expect(screen.queryByText("Data for parent")).not.toBeInTheDocument();
});

it("searches long lists without losing hidden selections and restores focus on Escape", async () => {
  show({ projects: [project("parent"), ...Array.from({ length: 20 }, (_, index) => project(`child-${index}`, "parent"))] });
  fireEvent.click(screen.getByRole("button", { name: "Show data" }));
  const search = screen.getByRole("searchbox", { name: "Search projects" });
  expect(search).toHaveFocus();
  fireEvent.change(search, { target: { value: "child-19" } });
  expect(screen.queryByRole("checkbox", { name: "Current project" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: "child-19" }));
  expect(await screen.findByText("Data for child-19")).toBeVisible();
  expect(await screen.findByText("Data for parent")).toBeVisible();
  fireEvent.change(search, { target: { value: "no such project" } });
  expect(screen.getByText("No projects found")).toBeVisible();
  fireEvent.keyDown(search, { key: "Escape" });
  expect(screen.getByRole("button", { name: "Show data" })).toHaveFocus();
  expect(screen.queryByRole("dialog", { name: "Show data" })).not.toBeInTheDocument();
});

it("clears all projects and remembers an intentionally empty selection", async () => {
  const view = show();
  selectProject("all");
  await screen.findByText("Data for child");
  fireEvent.click(screen.getByRole("button", { name: "Show data" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Current and subprojects" }));
  expect(screen.getByText("Select projects to show their data.")).toBeVisible();
  expect(screen.queryByText("Data for parent")).not.toBeInTheDocument();
  expect(screen.queryByText("Data for child")).not.toBeInTheDocument();
  view.unmount();
  show();
  expect(screen.getByText("Select projects to show their data.")).toBeVisible();
});

it("allows clicking a project label after Safari blurs the search without a focus target", async () => {
  show();
  fireEvent.click(screen.getByRole("button", { name: "Show data" }));
  fireEvent.blur(screen.getByRole("searchbox", { name: "Search projects" }), { relatedTarget: null });
  const picker = screen.getByRole("dialog", { name: "Show data" });
  fireEvent.click(within(picker).getByText("child"));
  expect(screen.getByRole("checkbox", { name: "child" })).toBeChecked();
  expect(picker).toBeVisible();
  expect(await screen.findByText("Data for child")).toBeVisible();
  fireEvent.pointerDown(document.body);
  expect(screen.queryByRole("dialog", { name: "Show data" })).not.toBeInTheDocument();
});

it("closes the picker when keyboard focus moves outside", () => {
  show();
  fireEvent.click(screen.getByRole("button", { name: "Show data" }));
  fireEvent.blur(screen.getByRole("searchbox", { name: "Search projects" }), { relatedTarget: screen.getByRole("main") });
  expect(screen.queryByRole("dialog", { name: "Show data" })).not.toBeInTheDocument();
});

it("ignores inaccessible projects in a saved subset", async () => {
  localStorage.setItem(storageKey(), JSON.stringify(["child", "sibling", "archived", "unknown"]));
  show();
  expect(await screen.findByText("Data for child")).toBeVisible();
  expect(api.getCurrentActor).toHaveBeenCalledExactlyOnceWith({ kind: "session", projectId: "child" });
});

it.each(["child", "all", '["archived"]', '{"invalid":true}'])("restores the saved selection %s safely", async (selection) => {
  localStorage.setItem(storageKey(), selection);
  show();
  expect(await screen.findByText(selection === "child" || selection === "all" ? "Data for child" : "Data for parent")).toBeVisible();
});
