import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { I18nContext, createI18n } from "../i18n";
import { SupportQualityView } from "./SupportQualityView";

vi.mock("../api", () => ({
  getSupportQuality: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const inbox: api.Inbox = {
  id: "00000000-0000-4000-8000-000000000010",
  project_id: "00000000-0000-4000-8000-000000000011",
  name: "Customer support",
  status: "active",
  created_at: "2026-08-14T00:00:00Z",
};

const report: api.SupportQualityReport = {
  period_started_at: "2026-07-15T00:00:00Z",
  summary: {
    total_resolutions: 2,
    rated_resolutions: 2,
    average_rating: 3,
    csat_percent: 50,
    response_rate_percent: 100,
    reopened_rate_percent: 50,
    average_first_response_seconds: 120,
    average_resolution_seconds: 3_600,
  },
  distribution: [
    { rating: 5, count: 1 },
    { rating: 4, count: 0 },
    { rating: 3, count: 0 },
    { rating: 2, count: 0 },
    { rating: 1, count: 1 },
  ],
  trend: [],
  available_filters: {
    operators: [{ id: "00000000-0000-4000-8000-000000000020", name: "Alice Operator" }],
    teams: [{ id: "00000000-0000-4000-8000-000000000030", name: "Support team" }],
    channels: [{ id: "00000000-0000-4000-8000-000000000040", name: "Website chat" }],
  },
  items: [
    {
      resolution_id: "00000000-0000-4000-8000-000000000101",
      conversation_id: "00000000-0000-4000-8000-000000000201",
      cycle_number: 1,
      conversation_subject: "Billing request",
      contact_id: "00000000-0000-4000-8000-000000000301",
      contact_name: "Billing customer",
      inbox_id: inbox.id,
      inbox_name: inbox.name,
      channel_id: "00000000-0000-4000-8000-000000000040",
      channel_name: "Website chat",
      channel_kind: "widget",
      responsible_actor_id: "00000000-0000-4000-8000-000000000020",
      responsible_operator_name: "Alice Operator",
      responsible_ai_profile_id: null,
      responsible_ai_profile_name: null,
      responsible_team_id: "00000000-0000-4000-8000-000000000030",
      responsible_team_name: "Support team",
      resolved_at: "2026-08-14T01:00:00Z",
      reopened_at: null,
      first_response_seconds: 90,
      resolution_seconds: 1_800,
      rating_id: "00000000-0000-4000-8000-000000000401",
      rating: 1,
      comment: "The answer did not help",
      reasons: ["unresolved"],
      rated_at: "2026-08-14T01:05:00Z",
    },
    {
      resolution_id: "00000000-0000-4000-8000-000000000102",
      conversation_id: "00000000-0000-4000-8000-000000000202",
      cycle_number: 2,
      conversation_subject: "Delivery request",
      contact_id: "00000000-0000-4000-8000-000000000302",
      contact_name: "Delivery customer",
      inbox_id: inbox.id,
      inbox_name: inbox.name,
      channel_id: "00000000-0000-4000-8000-000000000040",
      channel_name: "Website chat",
      channel_kind: "widget",
      responsible_actor_id: null,
      responsible_operator_name: null,
      responsible_ai_profile_id: "00000000-0000-4000-8000-000000000022",
      responsible_ai_profile_name: "Romanian Agent",
      responsible_team_id: "00000000-0000-4000-8000-000000000030",
      responsible_team_name: "Support team",
      resolved_at: "2026-08-14T02:00:00Z",
      reopened_at: "2026-08-14T02:30:00Z",
      first_response_seconds: 150,
      resolution_seconds: 5_400,
      rating_id: "00000000-0000-4000-8000-000000000402",
      rating: 5,
      comment: "Fast and clear",
      reasons: ["fast_response", "clear_answer"],
      rated_at: "2026-08-14T02:05:00Z",
    },
  ],
};

function renderView() {
  return render(
    <I18nContext.Provider value={createI18n("en", vi.fn())}>
      <SupportQualityView auth={auth} inboxes={[inbox]} onOpenConversation={vi.fn()} />
    </I18nContext.Provider>,
  );
}

describe("SupportQualityView", () => {
  beforeEach(() => {
    vi.mocked(api.getSupportQuality).mockResolvedValue(report);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("filters loaded resolution history without wrapping the table in a second rounded panel", async () => {
    renderView();

    expect(await screen.findByText("Billing request")).toBeInTheDocument();
    expect(screen.getByText("Delivery request")).toBeInTheDocument();
    expect(screen.getByRole("table").parentElement).toHaveClass("quality-table");
    expect(screen.getByRole("table").parentElement).not.toHaveClass("table-panel");
    expect(screen.queryByRole("combobox", { name: "Team" })).not.toBeInTheDocument();
    expect(screen.queryByText("Support team")).not.toBeInTheDocument();

    const initialRequestCount = vi.mocked(api.getSupportQuality).mock.calls.length;
    fireEvent.change(screen.getByRole("searchbox", { name: "Search history" }), {
      target: { value: "Alice" },
    });

    expect(screen.getByText("Billing request")).toBeInTheDocument();
    expect(screen.queryByText("Delivery request")).not.toBeInTheDocument();
    expect(screen.getByText("1 of 2 shown")).toBeInTheDocument();
    expect(api.getSupportQuality).toHaveBeenCalledTimes(initialRequestCount);

    fireEvent.click(screen.getByRole("button", { name: "Clear" }));
    fireEvent.change(screen.getByRole("combobox", { name: "Cycle status" }), {
      target: { value: "reopened" },
    });

    expect(screen.queryByText("Billing request")).not.toBeInTheDocument();
    expect(screen.getByText("Delivery request")).toBeInTheDocument();
  });

  it("shows and searches the AI profile responsible for a resolution", async () => {
    renderView();

    expect(await screen.findByText("Romanian Agent")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search history" }), {
      target: { value: "Romanian Agent" },
    });

    expect(screen.queryByText("Billing request")).not.toBeInTheDocument();
    expect(screen.getByText("Delivery request")).toBeInTheDocument();
    expect(screen.queryByText("Not assigned")).not.toBeInTheDocument();
  });

  it("requests rating-filtered history from the API", async () => {
    renderView();
    await screen.findByText("Billing request");

    fireEvent.change(screen.getByRole("combobox", { name: "Rating" }), {
      target: { value: "5" },
    });

    await waitFor(() => expect(api.getSupportQuality).toHaveBeenLastCalledWith(
      auth,
      expect.objectContaining({ rating: 5 }),
    ));
  });

  it("keeps the current quality summary visible while a refresh is in progress", async () => {
    let finishRefresh: ((value: api.SupportQualityReport) => void) | undefined;
    vi.mocked(api.getSupportQuality)
      .mockResolvedValueOnce(report)
      .mockImplementationOnce(() => new Promise((resolve) => {
        finishRefresh = resolve;
      }));
    renderView();

    await screen.findByText("Billing request");
    const summary = screen.getByRole("region", { name: "Quality summary" });
    expect(summary).toHaveAttribute("aria-busy", "false");

    fireEvent.change(screen.getByRole("combobox", { name: "Rating" }), {
      target: { value: "5" },
    });

    await waitFor(() => expect(summary).toHaveAttribute("aria-busy", "true"));
    expect(screen.getByText("2 responses")).toBeInTheDocument();

    finishRefresh?.(report);
    await waitFor(() => expect(summary).toHaveAttribute("aria-busy", "false"));
  });

  it("renders segmented quality signals from report values and trend aggregates", async () => {
    vi.mocked(api.getSupportQuality).mockResolvedValueOnce({
      ...report,
      trend: [
        {
          period_started_at: "2026-08-12T00:00:00Z",
          total_resolutions: 3,
          rated_resolutions: 2,
          average_rating: 3.5,
          csat_percent: 50,
          average_first_response_seconds: 90,
          average_resolution_seconds: 3_600,
        },
        {
          period_started_at: "2026-08-13T00:00:00Z",
          total_resolutions: 0,
          rated_resolutions: 0,
          average_rating: null,
          csat_percent: null,
          average_first_response_seconds: null,
          average_resolution_seconds: null,
        },
        {
          period_started_at: "2026-08-14T00:00:00Z",
          total_resolutions: 4,
          rated_resolutions: 4,
          average_rating: 4.5,
          csat_percent: 100,
          average_first_response_seconds: 120,
          average_resolution_seconds: 5_400,
        },
      ],
    });
    renderView();

    await screen.findByText("Billing request");
    const signal = (id: string) => document.querySelector(`[data-quality-signal="${id}"]`);

    expect(signal("average-rating")).toHaveAttribute("data-active-count", "9");
    expect(signal("response-rate")).toHaveAttribute("data-active-count", "9");
    expect(signal("reopened")).toHaveAttribute("data-active-count", "9");
    expect(signal("csat")).toHaveAttribute("data-point-count", "2");
    expect(signal("first-response")).toHaveAttribute("data-point-count", "2");
    expect(signal("resolution-time")).toHaveAttribute("data-point-count", "2");
    document.querySelectorAll("[data-quality-signal]").forEach((chart) => {
      expect(chart).toHaveAttribute("aria-hidden", "true");
    });
  });

  it("keeps empty quality signals neutral without invalid geometry", async () => {
    vi.mocked(api.getSupportQuality).mockResolvedValueOnce({
      ...report,
      summary: {
        total_resolutions: 0,
        rated_resolutions: 0,
        average_rating: null,
        csat_percent: null,
        response_rate_percent: 0,
        reopened_rate_percent: 0,
        average_first_response_seconds: null,
        average_resolution_seconds: null,
      },
      distribution: report.distribution.map((item) => ({ ...item, count: 0 })),
      trend: [],
      items: [],
    });
    const { container } = renderView();

    await screen.findByRole("region", { name: "Quality summary" });
    const signal = (id: string) => document.querySelector(`[data-quality-signal="${id}"]`);
    expect(signal("average-rating")).toHaveAttribute("data-active-count", "0");
    expect(signal("csat")).toHaveAttribute("data-active-count", "0");
    expect(signal("first-response")).toHaveAttribute("data-active-count", "0");
    expect(signal("resolution-time")).toHaveAttribute("data-active-count", "0");
    expect(signal("response-rate")).toHaveAttribute("data-active-count", "9");
    expect(signal("reopened")).toHaveAttribute("data-active-count", "9");
    expect(container.innerHTML).not.toMatch(/NaN|Infinity|undefined/);
  });
});
