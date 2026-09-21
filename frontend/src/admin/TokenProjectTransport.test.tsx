import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const requests: Request[] = [];

beforeEach(() => {
  vi.resetModules();
  requests.length = 0;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init);
    requests.push(request);
    return new Response(JSON.stringify({ items: [], ticket: "scoped-ticket" }), {
      status: 200, headers: { "Content-Type": "application/json" },
    });
  }));
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
  document.cookie = "tz_csrf=; max-age=0; path=/";
  document.cookie = "tz_csrf=; max-age=0; path=/";
});

describe("token project transport", () => {
  it("uses the product project and CSRF names for session, token and multipart requests", async () => {
    const api = await import("../api");
    const auth = { kind: "session" as const, projectId: "lite-project" };
    document.cookie = "tz_csrf=standard-csrf; path=/";
    document.cookie = "tz_csrf=lite-csrf; path=/";
    await api.getCurrentActor(auth);
    await api.createOperatorRealtimeTicket(auth, "lite-inbox");
    await api.managementApiRequest(auth, "/api/v1/ai/profiles", { method: "POST", body: new FormData() });
    await api.getCurrentActor({ kind: "access_token", token: "lite-token", projectId: "lite-project" });
    for (const request of requests) {
      expect(request.headers.get("X-Tz-Project-Id")).toBe("lite-project");
      if (request.method === "POST") expect(request.headers.get("X-CSRF-Token")).toBe("lite-csrf");
    }
    expect(requests[3].headers.get("Authorization")).toBe("Bearer lite-token");
    expect(requests[3].credentials).toBe("omit");

    document.cookie = "tz_csrf=; max-age=0; path=/";
    await api.managementApiRequest(auth, "/api/v1/ai/profiles", { method: "POST", body: "{}" });
    expect(requests[4].headers.get("X-CSRF-Token")).toBe("");
  });

  it("carries the selected project on API, realtime and multipart requests", async () => {
    const api = await import("../api");
    const auth = { kind: "access_token" as const, token: "all-project-token", projectId: "selected-project" };
    await api.getCurrentActor(auth);
    await api.listProjects(auth);
    await api.listInboxes(auth);
    await api.createOperatorRealtimeTicket(auth, "selected-inbox");
    await api.managementApiRequest(auth, "/api/v1/ai/profiles", { method: "POST", body: "{}" });
    await api.managementApiRequest(auth, "/api/v1/ai/skills/generate", { method: "POST", body: new FormData() });
    expect(requests).toHaveLength(6);
    for (const request of requests) {
      expect(request.headers.get("X-Tz-Project-Id")).toBe("selected-project");
      expect(request.headers.get("Authorization")).toBe("Bearer all-project-token");
      expect(request.headers.get("X-CSRF-Token")).toBeNull();
      expect(request.credentials).toBe("omit");
    }
    expect(await requests[3].json()).toEqual({ inbox_id: "selected-inbox" });
    expect(requests[5].headers.get("Content-Type")).toContain("multipart/form-data");
  });

  it("keeps project selection separate for concurrent requests using the same token", async () => {
    const { getCurrentActor } = await import("../api");
    await Promise.all(["project-a", "project-b"].map((projectId) => getCurrentActor({ kind: "access_token", token: "shared-token", projectId })));
    expect(requests.map((request) => request.headers.get("X-Tz-Project-Id")).sort()).toEqual(["project-a", "project-b"]);
  });

  it("keeps password sessions and tokens without a selection free of project overrides", async () => {
    const api = await import("../api");
    document.cookie = "tz_csrf=session-csrf; path=/";
    await api.managementApiRequest({ kind: "session" }, "/api/v1/access-tokens", { method: "POST", body: "{}" });
    await api.getCurrentActor({ kind: "access_token", token: "single-project-token" });
    expect(requests.every((request) => !request.headers.has("X-Tz-Project-Id"))).toBe(true);
    expect(requests[0].credentials).toBe("include");
    expect(requests[0].headers.get("X-CSRF-Token")).toBe("session-csrf");
    expect(requests[0].headers.get("Authorization")).toBeNull();
  });

  it("pins session reads, writes, uploads and realtime tickets to the window's project", async () => {
    const api = await import("../api");
    const auth = { kind: "session" as const, projectId: "project-a" };
    document.cookie = "tz_csrf=session-csrf; path=/";
    await api.getCurrentActor(auth);
    await api.listInboxes(auth);
    await api.createOperatorRealtimeTicket(auth, "inbox-a");
    await api.managementApiRequest(auth, "/api/v1/ai/profiles", { method: "POST", body: "{}" });
    await api.managementApiRequest(auth, "/api/v1/ai/skills/generate", { method: "POST", body: new FormData() });
    for (const request of requests) {
      expect(request.headers.get("X-Tz-Project-Id")).toBe("project-a");
      expect(request.credentials).toBe("include");
      expect(request.headers.get("Authorization")).toBeNull();
      if (request.method === "POST") expect(request.headers.get("X-CSRF-Token")).toBe("session-csrf");
    }
    expect(requests[4].headers.get("Content-Type")).toContain("multipart/form-data");
  });

  it("keeps concurrent requests with the same session scoped independently", async () => {
    const { getCurrentActor } = await import("../api");
    await Promise.all(["project-a", "project-b", "project-a"].map((projectId) => getCurrentActor({ kind: "session", projectId })));
    expect(requests.map((request) => request.headers.get("X-Tz-Project-Id"))).toEqual(["project-a", "project-b", "project-a"]);
  });
});
