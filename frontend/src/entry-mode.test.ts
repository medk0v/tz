import { describe, expect, it, vi } from "vitest";
import adminHtml from "../index.html?raw";
import { conversationLinkInboxId } from "./admin/conversation-route";
import {
  adminPagePath,
  isAdminPagePath,
  legacyAdminPagePath,
  resolveAdminPage,
  resolveAdminPageSegments,
  resolveProjectId,
  resolveAppRoute,
  resolvePublicNoteToken,
} from "./entry-mode";

const projectId = "00000000-0000-4000-8000-000000000001";
const inboxId = "00000000-0000-4000-8000-000000000002";
const conversationId = "00000000-0000-4000-8000-000000000003";

describe("public notes boot", () => {
  it.each([false, true] as const)("starts with system dark=%s without reading cabinet preferences", (dark) => {
    const page = new DOMParser().parseFromString(adminHtml, "text/html");
    const bootstrap = page.querySelector("script")?.textContent;
    expect(bootstrap).toBeTruthy();
    const storage = { getItem: vi.fn(() => "plum"), setItem: vi.fn() };
    new Function("document", "location", "window", "localStorage", bootstrap!)(page, { pathname: "/notes/share/public-handbook" }, { matchMedia: () => ({ matches: dark }) }, storage);
    expect(page.documentElement.dataset.palette).toBe("graphite");
    expect(page.documentElement.dataset.theme).toBe("light");
    expect(page.documentElement.style.colorScheme).toBe("light");
    expect(page.querySelector('meta[name="theme-color"]')?.getAttribute("content")).toBe("#f5f5f3");
    expect(page.querySelector('meta[name="referrer"]')?.getAttribute("content")).toBe("no-referrer");
    expect(page.querySelector('meta[name="robots"]')?.getAttribute("content")).toBe("noindex, nofollow");
    expect(storage.getItem).not.toHaveBeenCalled(); expect(storage.setItem).not.toHaveBeenCalled();
  });
});

describe("public note links", () => {
  it("opens without the cabinet", () => {
    expect(resolveAppRoute("/notes/share/team-notes")).toBe("public_notes");
    expect(resolveAppRoute("/notes/share/_secret/")).toBe("public_notes");
    expect(resolvePublicNoteToken("/notes/share/_secret/")).toBe("_secret");
    expect(resolvePublicNoteToken("/notes/share/team-notes")).toBe("team-notes");
  });
  it.each(["/notes/share", "/notes/share/a%2Fb", "/notes/share/%E0", "/notes/share/a/b", `/notes/share/${"a".repeat(129)}`])(
    "rejects invalid token paths without entering an authenticated page: %s", (path) => {
      expect(resolvePublicNoteToken(path)).toBeNull();
      expect(resolveAppRoute(path)).toBe("public_notes");
    },
  );
});

describe("nested cabinet page paths", () => {
  it.each([
    [["agents", "agent-1"], `/cabinet/p/${projectId}/ai/agents/agent-1`],
    [["agents", "agent 1", "instructions"], `/cabinet/p/${projectId}/ai/agents/agent%201/instructions`],
  ])("round-trips AI page segments %j", (segments, path) => {
    expect(adminPagePath("ai", projectId, segments)).toBe(path);
    expect(resolveAdminPage(`${path}/`)).toBe("ai");
    expect(resolveAdminPageSegments(`${path}/`)).toEqual(segments);
    expect(resolveProjectId(path)).toBe(projectId);
    expect(isAdminPagePath(path)).toBe(true);
  });

  it("round-trips project note links and new note forms", () => {
    for (const noteId of ["00000000-0000-4000-8000-000000000004", "new"]) {
      const path = adminPagePath("notes", projectId, [noteId]);
      expect(path).toBe(`/cabinet/p/${projectId}/notes/${noteId}`);
      expect(resolveAdminPage(path)).toBe("notes");
      expect(resolveAdminPageSegments(path)).toEqual([noteId]);
      expect(resolveProjectId(path)).toBe(projectId);
      expect(isAdminPagePath(path)).toBe(true);
    }
  });

  it("keeps the bare cabinet path for conversations and nests opened conversations under a named parent", () => {
    expect(adminPagePath("conversations", projectId)).toBe(`/cabinet/p/${projectId}`);
    const path = adminPagePath("conversations", projectId, [inboxId, conversationId]);
    expect(path).toBe(`/cabinet/p/${projectId}/conversations/${inboxId}/${conversationId}`);
    expect(resolveAdminPage(path)).toBe("conversations");
    expect(resolveAdminPageSegments(path)).toEqual([inboxId, conversationId]);
    expect(resolveAdminPageSegments(`/cabinet/p/${projectId}`)).toEqual([]);
    expect(conversationLinkInboxId(path)).toBe(inboxId);
    expect(conversationLinkInboxId(`/cabinet/p/${projectId}/team/${inboxId}/${conversationId}`)).toBeNull();
  });

  it.each([`/cabinet/p/${projectId}/team/%E0`, `/cabinet/p/${projectId}/team/a%2Fb`, `/cabinet/p/${projectId}/teams/agents`])(
    "rejects a malformed or unknown nested path: %s", (path) => {
      expect(isAdminPagePath(path)).toBe(false);
      expect(resolveAdminPageSegments(path)).toEqual([]);
    },
  );

  it("maps legacy record query links to nested paths and keeps other query parameters", () => {
    expect(legacyAdminPagePath(`/cabinet/p/${projectId}/ai`, "?agent=agent-1&from=backup"))
      .toBe(`/cabinet/p/${projectId}/ai/agents/agent-1?from=backup`);
    expect(legacyAdminPagePath("/cabinet/knowledge-base", "?base=base-1"))
      .toBe("/cabinet/knowledge-base/base-1");
    expect(legacyAdminPagePath("/cabinet/ai/agents/agent-2", "?agent=agent-1")).toBeNull();
    expect(legacyAdminPagePath("/cabinet/team", "?agent=agent-1")).toBeNull();
    expect(legacyAdminPagePath("/cabinet/ai", "")).toBeNull();
  });
});
