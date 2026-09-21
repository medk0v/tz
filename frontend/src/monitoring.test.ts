import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as Sentry from "@sentry/browser";
import { initMonitoring } from "./monitoring";

vi.mock("@sentry/browser", () => ({ init: vi.fn() }));

beforeEach(() => {
  vi.stubEnv("VITE_SENTRY_DSN", "https://public@example.test/1");
  vi.clearAllMocks();
  window.history.replaceState(null, "", "/cabinet");
});
afterEach(() => { vi.unstubAllEnvs(); window.history.replaceState(null, "", "/"); });

function options() {
  initMonitoring("admin");
  return vi.mocked(Sentry.init).mock.calls[0][0]!;
}

describe("public note telemetry privacy", () => {
  it("does not report document events or breadcrumbs on a shared page", () => {
    const config = options();
    window.history.replaceState(null, "", "/notes/share/_private-token?note=private-id");
    expect(config.beforeSend!({ type: undefined, request: { url: "http://localhost/api/v1/public/notes/resolve" } }, {})).toBeNull();
    expect(config.beforeBreadcrumb!({ category: "ui.click", message: "private title" }, {})).toBeNull();
  });
  it("drops errors with a public document URL and redacts navigation tokens after leaving", () => {
    const config = options();
    expect(config.beforeSend!({ type: undefined, request: { url: "https://example.test/notes/share/_private-token" } }, {})).toBeNull();
    expect(config.beforeBreadcrumb!({ category: "navigation", data: { from: "/notes/share/_private-token?note=id", to: "/cabinet" } }, {}))
      .toMatchObject({ data: { from: "/notes/share/[redacted]", to: "/cabinet" } });
  });
});
