import { afterEach, describe, expect, it, vi } from "vitest";
import { PublicNoteShareError, publicNoteSharePath, resolvePublicNotes, unlockPublicNotes, updateNoteShare, updatePublicNote } from "./note-shares-api";

afterEach(() => vi.unstubAllGlobals());

describe("public note requests", () => {
  it("updates the existing link's full settings at its version in the selected project", async () => {
    const settings = { theme_palette: "paper", theme_mode: "light", allow_theme_change: false,
      include_descendants: true, included_note_ids: ["child-id"], can_edit: false, expected_version: 3 } as const;
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ id: "share-id", version: 4 })));
    vi.stubGlobal("fetch", fetch);
    await updateNoteShare({ kind: "access_token", token: "workspace-token", projectId: "project-id" }, "share-id", { ...settings, included_note_ids: [...settings.included_note_ids] });
    const [url, options] = fetch.mock.calls[0] as [string, RequestInit];
    expect(url).toMatch(/\/api\/v1\/notes\/shares\/share-id$/);
    expect(options).toMatchObject({ method: "PATCH", credentials: "omit", headers: { Authorization: "Bearer workspace-token", "X-Tz-Project-Id": "project-id" } });
    expect(JSON.parse(options.body as string)).toEqual(settings);
    expect(options.body).not.toContain("password");
  });

  it("keeps access credentials in JSON and excludes cabinet credentials and referrers", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ items: [], note: null, can_edit: false, root_note_id: null })));
    vi.stubGlobal("fetch", fetch);
    await resolvePublicNotes("_secret", "session-secret", "note-id");
    const [url, options] = fetch.mock.calls[0] as [string, RequestInit];
    expect(url).toMatch(/\/api\/v1\/public\/notes\/resolve$/);
    expect(url).not.toContain("secret");
    expect(options).toMatchObject({ method: "POST", credentials: "omit", cache: "no-store", referrerPolicy: "no-referrer", redirect: "error" });
    expect(options.headers).toEqual({ "Content-Type": "application/json" });
    expect(JSON.parse(options.body as string)).toEqual({ token: "_secret", access_token: "session-secret", note_id: "note-id" });
  });

  it("unlocks without persisting a password and exposes the response status for password and conflict UI", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { code: "unauthorized", message: "authentication required" } }), { status: 401 }));
    vi.stubGlobal("fetch", fetch);
    await expect(unlockPublicNotes("_secret", "private password")).rejects.toMatchObject({ status: 401, code: "unauthorized" });
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({ token: "_secret", password: "private password" });
    fetch.mockResolvedValue(new Response(JSON.stringify({ error: { code: "conflict", message: "changed" } }), { status: 409 }));
    await expect(updatePublicNote("_secret", undefined, "note-id", { title: "Page", body: "Draft", icon: "", expected_version: 1 }))
      .rejects.toBeInstanceOf(PublicNoteShareError);
    expect(fetch.mock.calls[1][1].method).toBe("PATCH");
  });

  it("builds share links with an optional selected page", () => {
    expect(publicNoteSharePath("team-notes")).toBe("/notes/share/team-notes");
    expect(publicNoteSharePath("_secret", "child-id")).toBe("/notes/share/_secret?note=child-id");
  });
});
