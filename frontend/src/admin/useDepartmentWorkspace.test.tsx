import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ActorContext } from "../api";
import { listDepartments, type DepartmentList } from "../department-api";
import { useDepartmentWorkspace } from "./useDepartmentWorkspace";

vi.mock("../department-api", () => ({ listDepartments: vi.fn() }));
const auth = { kind: "session" } as const;
const actor = { actor_id: "operator-a", project_id: "project-a", department_id: "support", is_director: false } as ActorContext;
const data: DepartmentList = {
  items: [{ id: "support", name: "Support", icon: "headset", sidebar_items: ["conversations"], default_page: "conversations", position: 0, inbox_ids: ["support-inbox"], member_count: 1 }],
  default_department_id: "support", director_enabled: true, can_access_director: false,
};
beforeEach(() => { vi.mocked(listDepartments).mockReset(); });
afterEach(cleanup);

it("does not load department metadata for a project access token without a department", async () => {
  const tokenAuth = { kind: "access_token", token: "project-token" } as const;
  vi.mocked(listDepartments).mockRejectedValueOnce(new Error("Forbidden"));
  const tokenActor: ActorContext = {
    ...actor, auth_method: "access_token", department_id: null,
    department_restricted: false, can_access_director: false,
  };
  const { result } = renderHook(() => useDepartmentWorkspace(tokenAuth, tokenActor));

  await waitFor(() => expect(result.current.enabled).toBe(false));
  expect(result.current.loading).toBe(false);
  expect(result.current.data).toBeNull();
  expect(result.current.error).toBeUndefined();
  expect(listDepartments).not.toHaveBeenCalled();
});

it("loads the assigned department's metadata for a department access token", async () => {
  const tokenAuth = { kind: "access_token", token: "department-token" } as const;
  vi.mocked(listDepartments).mockResolvedValueOnce(data);
  const { result } = renderHook(() => useDepartmentWorkspace(tokenAuth, {
    ...actor, auth_method: "access_token", department_restricted: true, can_access_director: false,
  }));

  await waitFor(() => expect(result.current.data).toEqual(data));
  expect(result.current.enabled).toBe(true);
  expect(listDepartments).toHaveBeenCalledWith(tokenAuth, actor.project_id);
});

it("keeps the current workspace mounted while refreshing settings and after a background failure", async () => {
  let rejectRefresh!: (error: Error) => void;
  vi.mocked(listDepartments).mockResolvedValueOnce(data).mockImplementationOnce(() => new Promise((_, reject) => { rejectRefresh = reject; }));
  const { result } = renderHook(() => useDepartmentWorkspace(auth, actor));
  await waitFor(() => expect(result.current.data).toEqual(data));

  act(() => result.current.reload());
  expect(result.current.loading).toBe(false);
  expect(result.current.data).toEqual(data);
  await act(async () => rejectRefresh(new Error("Unavailable")));
  expect(result.current.data).toEqual(data);
  expect(result.current.error).toEqual(new Error("Unavailable"));
});

it("discards a pending response from the previous department", async () => {
  let resolvePrevious!: (value: DepartmentList) => void;
  const nextData = { ...data, items: [] };
  vi.mocked(listDepartments).mockImplementationOnce(() => new Promise((resolve) => { resolvePrevious = resolve; })).mockResolvedValueOnce(nextData);
  const { result, rerender } = renderHook(({ current }) => useDepartmentWorkspace(auth, current), { initialProps: { current: actor } });
  rerender({ current: { ...actor, department_id: "sales" } });
  await waitFor(() => expect(result.current.data).toEqual(nextData));
  await act(async () => resolvePrevious(data));
  expect(result.current.data).toEqual(nextData);
});

it("does not expose the previous employee's cached department metadata", async () => {
  vi.mocked(listDepartments).mockResolvedValueOnce(data).mockImplementationOnce(() => new Promise(() => {}));
  const { result, rerender } = renderHook(({ current }) => useDepartmentWorkspace(auth, current), { initialProps: { current: actor } });
  await waitFor(() => expect(result.current.data).toEqual(data));
  rerender({ current: { ...actor, actor_id: "operator-b" } });
  expect(result.current.loading).toBe(true);
  expect(result.current.data).toBeNull();
});

it("reloads metadata when the same employee's effective inbox access narrows", async () => {
  vi.mocked(listDepartments).mockResolvedValueOnce(data).mockImplementationOnce(() => new Promise(() => {}));
  const { result, rerender } = renderHook(({ current }) => useDepartmentWorkspace(auth, current), { initialProps: { current: actor } });
  await waitFor(() => expect(result.current.data).toEqual(data));
  rerender({ current: { ...actor, inbox_scope: ["support-inbox"], department_restricted: true } });
  expect(result.current.loading).toBe(true);
  expect(result.current.data).toBeNull();
});
