import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { VisitorIntelligence } from "../api";
import { I18nContext, createI18n } from "../i18n";
import { VisitorIntelligenceDetails } from "./VisitorIntelligenceDetails";

const intelligence: VisitorIntelligence = {
  session_count: 1,
  first_seen_at: "2026-08-29T00:00:00Z",
  last_seen_at: "2026-08-29T00:00:00Z",
  observations: [{
    session_id: "00000000-0000-4000-8000-000000000001",
    widget_language: "ru",
    client_ip: "146.70.188.241",
    client_ip_source: "trusted_proxy",
    user_agent: null,
    geo_ip: {
      country_code: "BG",
      country: "Bulgaria",
      city: "Sofia",
      time_zone: "Europe/Sofia",
    },
    user_agent_details: null,
    accept_language: null,
    client_hints: null,
    client_context: null,
    origin: "https://example.com",
    captured_at: "2026-08-29T00:00:00Z",
  }],
};

describe("VisitorIntelligenceDetails", () => {
  afterEach(cleanup);

  it("shows the visitor country flag before the GeoIP location", () => {
    render(
      <I18nContext.Provider value={createI18n("ru", vi.fn())}>
        <VisitorIntelligenceDetails intelligence={intelligence} status="ready" />
      </I18nContext.Provider>,
    );

    expect(screen.getByText("🇧🇬")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByText("Sofia, Bulgaria (BG)")).toBeInTheDocument();
  });
});
