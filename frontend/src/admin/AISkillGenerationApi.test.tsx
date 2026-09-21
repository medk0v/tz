import { afterEach, describe, expect, it, vi } from "vitest";
import { createAiSkill, generateAiSkill } from "../ai-skills-api";
import { ApiRequestError } from "../api";

const result = { name: "Source review", description: "Review supplied sources", instructions: "Use the supplied guidance." };
const input = { provider_connection_id: "00000000-0000-4000-8000-000000000019", goal: "Review sources", source_text: "Source text", files: [new File(["notes"], "notes.md", { type: "text/markdown" }), new File(["image"], "diagram.png", { type: "image/png" })] };

afterEach(() => {
  vi.unstubAllGlobals();
  document.cookie = "tz_csrf=; max-age=0; path=/";
});

describe("skill generation API transport", () => {
  it("posts multipart sources using session credentials and CSRF without forcing a content type", async () => {
    document.cookie = "tz_csrf=csrf-for-skill; path=/";
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(result), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    const controller = new AbortController();
    await expect(generateAiSkill({ kind: "session" }, input, controller.signal)).resolves.toEqual(result);
    const [url, request] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toMatch(/\/api\/v1\/ai\/skills\/generate$/);
    expect(request).toMatchObject({ method: "POST", credentials: "include", signal: controller.signal });
    const headers = new Headers(request.headers);
    expect(headers.get("Content-Type")).toBeNull();
    expect(headers.get("X-CSRF-Token")).toBe("csrf-for-skill");
    expect(headers.get("Authorization")).toBeNull();
    expect(request.body).toBeInstanceOf(FormData);
    const body = request.body as FormData;
    expect(body.get("provider_connection_id")).toBe(input.provider_connection_id);
    expect(body.get("goal")).toBe(input.goal);
    expect(body.get("source_text")).toBe(input.source_text);
    expect(body.getAll("file")).toEqual(input.files);
  });

  it("uses bearer authorization without cookies and omits optional blank text", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(result), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await generateAiSkill({ kind: "access_token", token: "project-token" }, { ...input, source_text: " ", files: [] });
    const request = fetchMock.mock.calls[0][1] as RequestInit;
    expect(request.credentials).toBe("omit");
    const headers = new Headers(request.headers);
    expect(headers.get("Authorization")).toBe("Bearer project-token");
    expect(headers.get("X-CSRF-Token")).toBeNull();
    expect(headers.get("Content-Type")).toBeNull();
    expect((request.body as FormData).has("source_text")).toBe(false);
    expect((request.body as FormData).getAll("file")).toEqual([]);
  });

  it("preserves structured validation errors and blocks demo requests before fetch", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { message: "book.pdf has no readable text." } }), { status: 422 }));
    vi.stubGlobal("fetch", fetchMock);
    await expect(generateAiSkill({ kind: "session" }, input)).rejects.toEqual(new ApiRequestError("book.pdf has no readable text.", 422));
    fetchMock.mockClear();
    await expect(generateAiSkill({ kind: "session", isDemo: true }, input)).rejects.toThrow("demo_readonly");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("retains JSON content type when saving reviewed instructions", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ ...result, id: "skill" }), { status: 201 }));
    vi.stubGlobal("fetch", fetchMock);
    const draft = { ...result, ai_profile_ids: [] };
    await createAiSkill({ kind: "session" }, draft);
    const request = fetchMock.mock.calls[0][1] as RequestInit;
    expect(new Headers(request.headers).get("Content-Type")).toBe("application/json");
    expect(request.body).toBe(JSON.stringify(draft));
  });
});
