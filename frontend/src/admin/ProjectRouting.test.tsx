import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import * as api from "../api";
import { adminPagePath } from "../entry-mode";

vi.mock("../api", async (importOriginal) => ({
  ...await importOriginal<typeof api>(),
  getCurrentActor: vi.fn(),
  loginWithPassword: vi.fn(),
  listInboxes: vi.fn().mockResolvedValue([]),
}));
vi.mock("../department-api", () => ({
  listDepartments: vi.fn().mockResolvedValue({ items: [], default_department_id: null, director_enabled: true, can_access_director: true }),
  selectDepartment: vi.fn(),
}));
vi.mock("./useOperatorPresence", () => ({ useOperatorPresence: vi.fn() }));
vi.mock("./useOperatorRealtime", () => ({ useOperatorRealtime: vi.fn() }));
vi.mock("./DirectoryViews", () => ({
  ContactsView: ({ auth }: { auth: api.OperatorAuth }) => <h1>Contacts {auth.projectId}</h1>,
}));
vi.mock("./ConversationsView", () => ({
  ConversationsView: ({ auth }: { auth: api.OperatorAuth }) => <h1>Conversations {auth.projectId}</h1>,
}));
vi.mock("./DirectorOverview", () => ({
  DirectorOverview: ({ projectId }: { projectId: string }) => <h1>Overview {projectId}</h1>,
}));

const projectA = "00000000-0000-4000-8000-000000000001";
const projectB = "00000000-0000-4000-8000-000000000002";
const pathA = adminPagePath("contacts", projectA);
const pathB = adminPagePath("contacts", projectB);

function actor(projectId: string): api.ActorContext {
  return {
    actor_id: "operator", tenant_id: "tenant", project_id: projectId, project_name: projectId,
    role: "admin", auth_method: "session", chat_display_name: "Admin", avatar_url: null,
    permissions: ["contacts:read", "conversations:read"], inbox_scope: null,
  };
}

function visit(path: string) {
  act(() => {
    window.history.pushState(null, "", path);
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.getCurrentActor).mockReset().mockImplementation(async (auth) => actor(auth.projectId ?? projectB));
  vi.mocked(api.listInboxes).mockResolvedValue([]);
  sessionStorage.clear();
  localStorage.clear();
  window.history.replaceState(null, "", pathA);
});
afterEach(cleanup);

describe("project URL routing", () => {
  it("restores both project and page on browser history navigation", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: `Contacts ${projectA}` });
    visit(pathB);
    await screen.findByRole("heading", { name: `Contacts ${projectB}` });
    visit(pathA);
    await screen.findByRole("heading", { name: `Contacts ${projectA}` });
    expect(api.getCurrentActor).toHaveBeenLastCalledWith({ kind: "session", projectId: projectA });
  });

  it.each(["failed", "pending"])("recovers the previous project after a %s project navigation", async (state) => {
    render(<App />);
    await screen.findByRole("heading", { name: `Contacts ${projectA}` });
    let finish: ((value: api.ActorContext) => void) | undefined;
    vi.mocked(api.getCurrentActor).mockImplementation(async (auth) => {
      if (auth.projectId !== projectB) return actor(projectA);
      if (state === "failed") throw new api.ApiRequestError("Forbidden", 403);
      return new Promise((resolve) => { finish = resolve; });
    });
    visit(pathB);
    await waitFor(() => expect(api.getCurrentActor).toHaveBeenLastCalledWith({ kind: "session", projectId: projectB }));
    if (state === "failed") await screen.findByRole("alert");
    visit(pathA);
    await screen.findByRole("heading", { name: `Contacts ${projectA}` });
    if (finish) await act(async () => finish?.(actor(projectB)));
    expect(screen.getByRole("heading", { name: `Contacts ${projectA}` })).toBeInTheDocument();
    expect(window.location.pathname).toBe(pathA);
  });

  it("does not open another project when the URL's project is unavailable", async () => {
    vi.mocked(api.getCurrentActor).mockRejectedValue(new api.ApiRequestError("Forbidden", 403));
    render(<App />);
    await screen.findByRole("alert");
    expect(api.getCurrentActor).toHaveBeenCalledExactlyOnceWith({ kind: "session", projectId: projectA });
    expect(api.listInboxes).not.toHaveBeenCalled();
    expect(window.location.pathname).toBe(pathA);
  });

  it("prioritizes a token's URL over its saved project", async () => {
    sessionStorage.setItem("tz-operator-token", "token");
    sessionStorage.setItem("tz-operator-token-project", projectB);
    vi.mocked(api.getCurrentActor).mockResolvedValue({ ...actor(projectA), auth_method: "access_token", access_token_project_scope: "all" });
    render(<App />);
    await screen.findByRole("heading", { name: `Contacts ${projectA}` });
    expect(api.getCurrentActor).toHaveBeenCalledExactlyOnceWith({ kind: "access_token", token: "token", projectId: projectA });
  });

  it("canonicalizes old links without dropping query parameters or fragments", async () => {
    window.history.replaceState(null, "", "/cabinet/contacts?contact=123#details");
    render(<App />);
    await screen.findByRole("heading", { name: `Contacts ${projectB}` });
    expect(window.location.pathname).toBe(pathB);
    expect(window.location.search).toBe("?contact=123");
    expect(window.location.hash).toBe("#details");
  });

});
