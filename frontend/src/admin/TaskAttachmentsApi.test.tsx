import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiRequestError, downloadTaskScreenshot } from "../api";

afterEach(() => vi.unstubAllGlobals());

describe("task attachment download", () => {
  it.each([
    ["attachment; filename=\"fallback\"; filename*=UTF-8''%D0%9F%D0%BB%D0%B0%D0%BD%20%2B%201.docx", "План + 1.docx"],
    ["attachment; filename=\"requirements.pdf\"", "requirements.pdf"],
  ])("preserves the original filename from %s", async (disposition, name) => {
    const response = new Response("file content", {
      headers: { "Content-Type": "application/octet-stream", "Content-Disposition": disposition },
    });
    vi.spyOn(response, "blob").mockResolvedValue(new Blob(["file content"], { type: "application/octet-stream" }));
    const fetchMock = vi.fn().mockResolvedValue(response);
    vi.stubGlobal("fetch", fetchMock);
    const file = await downloadTaskScreenshot({ kind: "session", projectId: "project-1" }, "attachment");
    expect(file.name).toBe(name);
    expect(file.type).toBe("application/octet-stream");
    expect(file.size).toBe(12);
    const [, request] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(request).toMatchObject({ credentials: "include", cache: "no-store" });
    expect(new Headers(request.headers).get("X-Tz-Project-Id")).toBe("project-1");
  });

  it("reports access failures without creating a file", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { message: "Not found" } }), { status: 404 })));
    await expect(downloadTaskScreenshot({ kind: "session" }, "attachment"))
      .rejects.toEqual(new ApiRequestError("Not found", 404));
  });
});
