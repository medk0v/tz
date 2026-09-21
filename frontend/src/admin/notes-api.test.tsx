import { afterEach, expect, it, vi } from "vitest";
import { managementApiRequest } from "../api";
import { moveNote } from "../notes-api";

vi.mock("../api", () => ({ managementApiRequest: vi.fn() }));
afterEach(() => vi.resetAllMocks());
it("sends a project-scoped metadata-only move with explicit nullable placement and expected version", async () => {
  const auth = { kind: "session", projectId: "project-a" } as const;
  const result = { note: { id: "page", parent_id: null, version: 8, sort_order: 2 }, items: [] };
  vi.mocked(managementApiRequest).mockResolvedValue(result);
  expect(await moveNote(auth, "page", { parent_id: null, before_id: null, expected_version: 7 })).toBe(result);
  expect(managementApiRequest).toHaveBeenCalledExactlyOnceWith(auth, "/api/v1/notes/page/move", {
    method: "POST", body: '{"parent_id":null,"before_id":null,"expected_version":7}',
  });
});
