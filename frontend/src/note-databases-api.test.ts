import { afterEach, describe, expect, it, vi } from "vitest";
import { createPublicNoteDatabaseClient } from "./note-databases-api";

describe("public note database requests", () => {
  afterEach(() => vi.unstubAllGlobals());
  it("sends share and password grants only in JSON and omits account credentials", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ items: [], total: 0, page: 1, per_page: 20 })));
    vi.stubGlobal("fetch", fetch);
    await createPublicNoteDatabaseClient("secret-link", "unlock-grant").listRecords("table-id", { q: "Launch", per_page: 20 });
    const [url, options] = fetch.mock.calls[0];
    expect(url).toMatch(/\/api\/v1\/public\/notes\/databases$/);
    expect(url).not.toContain("secret-link");
    expect(options).toMatchObject({ method: "POST", credentials: "omit", cache: "no-store", referrerPolicy: "no-referrer", redirect: "error" });
    expect(JSON.parse(options.body)).toEqual({ action: "records", token: "secret-link", access_token: "unlock-grant", database_id: "table-id", q: "Launch", per_page: 20 });
  });
  it("preserves conflict status for row drafts and does not retry a rejected mutation", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: { message: "stale record", code: "conflict" } }), { status: 409 }));
    vi.stubGlobal("fetch", fetch);
    await expect(createPublicNoteDatabaseClient("link").updateRecord("db", "record", { values: {}, content_markdown: "Draft", expected_version: 3 }))
      .rejects.toMatchObject({ status: 409, code: "conflict" });
    expect(fetch).toHaveBeenCalledTimes(1);
  });
});
