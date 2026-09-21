import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { createI18n, I18nContext } from "../i18n";
import { InboxRoutingView } from "./InboxRoutingView";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";
import { changeDateTimeField, dateTimeFieldInput } from "./date-time-field-test-utils";
import { usePageRoute } from "./page-route";

vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return {
    ...actual,
    getInboxRouting: vi.fn(),
    updateInboxRouting: vi.fn(),
  };
});

const auth: api.OperatorAuth = { kind: "session" };
const inbox: api.Inbox = {
  id: "00000000-0000-4000-8000-000000000010",
  project_id: "00000000-0000-4000-8000-000000000011",
  name: "Customer support",
  status: "active",
  created_at: "2026-08-29T00:00:00Z",
};
const secondInbox: api.Inbox = {
  ...inbox,
  id: "00000000-0000-4000-8000-000000000099",
  name: "Sales",
};
const channelId = "00000000-0000-4000-8000-000000000012";
const priorityQueueId = "00000000-0000-4000-8000-000000000021";
const billingQueueId = "00000000-0000-4000-8000-000000000022";
const ruleId = "00000000-0000-4000-8000-000000000031";
const intervalId = "00000000-0000-4000-8000-000000000041";
const nadiaId = "00000000-0000-4000-8000-000000000051";
const levId = "00000000-0000-4000-8000-000000000052";

const routingResponse: api.InboxRoutingResponse = {
  configuration: {
    inbox_id: inbox.id,
    project_id: inbox.project_id,
    configured: true,
    version: 7,
    enabled: true,
    assignment_strategy: "least_active",
    default_queue_id: priorityQueueId,
    queues: [
      { id: priorityQueueId, name: "Priority", status: "active", member_ids: [nadiaId] },
      { id: billingQueueId, name: "Billing", status: "active", member_ids: [] },
    ],
    rules: [{
      id: ruleId,
      position: 1,
      enabled: true,
      channel_id: channelId,
      language: "en",
      queue_id: priorityQueueId,
    }],
    coverage: {
      timezone: "Europe/Istanbul",
      weekly_intervals: [{
        id: intervalId,
        weekday: 1,
        starts_at: "09:00",
        ends_at: "18:00",
      }],
      outside_hours_action: "ai",
      no_operator_action: "queue",
    },
    sla: {
      clock: "working_hours",
      unassigned_warning_seconds: 180,
      first_response_seconds: 300,
      resolution_seconds: 1_800,
      escalation_queue_id: billingQueueId,
    },
    telegram: {
      new_visitor: false,
      new_message: false,
      operator_request: true,
      unassigned_warning: true,
      sla_breach: true,
      chat_ids: ["-1001234567890"],
      bot_token_configured: true,
    },
  },
  channel_options: [{
    id: channelId,
    name: "Main website",
    kind: "widget",
    status: "active",
  }],
  operator_options: [
    {
      id: nadiaId,
      display_name: "Nadia Cohen",
      email: "nadia@example.com",
      avatar_url: null,
      online: true,
    },
    {
      id: levId,
      display_name: "Lev Bar",
      email: "lev@example.com",
      avatar_url: null,
      online: false,
    },
  ],
};

function savedResponse(input: api.UpdateInboxRoutingInput): api.InboxRoutingResponse {
  return {
    ...routingResponse,
    configuration: {
      ...routingResponse.configuration,
      ...input.configuration,
      configured: true,
      version: 8,
      telegram: {
        ...input.configuration.telegram,
        bot_token_configured: input.telegram_bot_token.action === "clear"
          ? false
          : routingResponse.configuration.telegram.bot_token_configured
            || input.telegram_bot_token.action === "replace",
      },
    },
  };
}

/** Moves the page URL the way browser Back/Forward does after the shell confirmed leaving the draft. */
function HistoryMove({ segments }: { segments: string[] }) {
  const route = usePageRoute();
  return <button type="button" onClick={() => void route.navigate(segments)}>Move in history</button>;
}

function renderView({
  availableInboxes = [inbox],
  onDirtyChange,
  segments = [],
  historyTarget,
}: {
  availableInboxes?: api.Inbox[];
  onDirtyChange?: (dirty: boolean) => void;
  segments?: string[];
  historyTarget?: string[];
} = {}) {
  return render(
    <I18nContext.Provider value={createI18n("en", vi.fn())}>
      <MemoryPageRoute initialSegments={segments}>
        <InboxRoutingView
          auth={auth}
          inboxes={availableInboxes}
          onDirtyChange={onDirtyChange}
        />
        <CurrentPageRoute />
        {historyTarget && <HistoryMove segments={historyTarget} />}
      </MemoryPageRoute>
    </I18nContext.Provider>,
  );
}

const pageRoute = () => screen.getByTestId("page-route").textContent;

describe("InboxRoutingView", () => {
  beforeEach(() => {
    vi.mocked(api.getInboxRouting).mockResolvedValue(routingResponse);
    vi.mocked(api.updateInboxRouting).mockImplementation(async (_auth, _inboxId, input) => (
      savedResponse(input)
    ));
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.restoreAllMocks();
  });

  it("renders the channel-to-rule-to-queue workflow and all fallback scenarios", async () => {
    const view = renderView();

    expect(await screen.findByRole("button", { name: /^Main website:/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Rule 1:/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Priority: 1 operators/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Hours & AI fallback:/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Telegram alerts:/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^SLA:/ })).toHaveTextContent("Resolve 30m");
    expect(view.container.querySelector("canvas")).toBeInTheDocument();
  });

  it("saves an exact versioned draft with 1-based rules, interval IDs, operators and resolution SLA", async () => {
    renderView();

    fireEvent.change(await screen.findByLabelText("And language is"), { target: { value: "RU" } });
    fireEvent.blur(screen.getByLabelText("And language is"));
    fireEvent.click(screen.getByRole("button", { name: /^Priority: 1 operators/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Lev Bar/ }));
    fireEvent.click(screen.getByRole("button", { name: /^SLA:/ }));
    fireEvent.change(screen.getByLabelText("Resolution, min"), { target: { value: "45" } });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    const expectedConfiguration: api.InboxRoutingWrite = {
      enabled: true,
      assignment_strategy: "least_active",
      default_queue_id: priorityQueueId,
      queues: [
        { id: priorityQueueId, name: "Priority", status: "active", member_ids: [nadiaId, levId] },
        { id: billingQueueId, name: "Billing", status: "active", member_ids: [] },
      ],
      rules: [{
        id: ruleId,
        position: 1,
        enabled: true,
        channel_id: channelId,
        language: "ru",
        queue_id: priorityQueueId,
      }],
      coverage: {
        timezone: "Europe/Istanbul",
        weekly_intervals: [{
          id: intervalId,
          weekday: 1,
          starts_at: "09:00",
          ends_at: "18:00",
        }],
        outside_hours_action: "ai",
        no_operator_action: "queue",
      },
      sla: {
        clock: "working_hours",
        unassigned_warning_seconds: 180,
        first_response_seconds: 300,
        resolution_seconds: 2_700,
        escalation_queue_id: billingQueueId,
      },
      telegram: {
        new_visitor: false,
        new_message: false,
        operator_request: true,
        unassigned_warning: true,
        sla_breach: true,
        chat_ids: ["-1001234567890"],
      },
    };
    await waitFor(() => expect(api.updateInboxRouting).toHaveBeenCalledWith(
      auth,
      inbox.id,
      {
        expected_version: 7,
        configuration: expectedConfiguration,
        telegram_bot_token: { action: "preserve" },
      },
      expect.stringMatching(/^[0-9a-f-]{36}$/),
    ));
    expect(await screen.findByRole("status")).toHaveTextContent("Routing configuration saved.");
  });

  it("validates the resolution target against first response before saving", async () => {
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /^SLA:/ }));
    fireEvent.change(screen.getByLabelText("Resolution, min"), { target: { value: "5" } });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The resolution target must be later than the first-response target.",
    );
    expect(api.updateInboxRouting).not.toHaveBeenCalled();
  });

  it("submits a write-only Telegram token, clears it from the UI and handles a 409 conflict", async () => {
    vi.mocked(api.updateInboxRouting).mockRejectedValue(new api.ApiRequestError("stale configuration", 409));
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /^Telegram alerts:/ }));
    const token = screen.getByLabelText("Bot token");
    expect(token).toHaveValue("");
    expect(token).toHaveAttribute("placeholder", "A token is configured; type to replace it");
    fireEvent.change(token, { target: { value: "123456:private-routing-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    await waitFor(() => expect(api.updateInboxRouting).toHaveBeenCalled());
    expect(vi.mocked(api.updateInboxRouting).mock.calls[0][2].telegram_bot_token).toEqual({
      action: "replace",
      value: "123456:private-routing-token",
    });
    expect(await screen.findByRole("alert")).toHaveTextContent("Routing was changed elsewhere.");
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
    expect(screen.queryByDisplayValue("123456:private-routing-token")).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("123456:private-routing-token");
  });

  it("normalizes blank and padded Telegram recipient lines before validation", async () => {
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /^Telegram alerts:/ }));
    fireEvent.change(screen.getByLabelText("Recipient chat IDs"), {
      target: { value: " -1001234567890 \n\n-1009876543210,\n" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    await waitFor(() => expect(api.updateInboxRouting).toHaveBeenCalled());
    expect(vi.mocked(api.updateInboxRouting).mock.calls[0][2].configuration.telegram.chat_ids)
      .toEqual(["-1001234567890", "-1009876543210"]);
  });

  it("keeps a Telegram token and idempotency key for retry after a temporary failure", async () => {
    vi.mocked(api.updateInboxRouting).mockRejectedValueOnce(
      new api.ApiRequestError("temporary failure", 503),
    );
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /^Telegram alerts:/ }));
    const token = screen.getByLabelText("Bot token");
    fireEvent.change(token, { target: { value: "123456:private-routing-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save routing");
    expect(token).toHaveValue("123456:private-routing-token");
    const firstKey = vi.mocked(api.updateInboxRouting).mock.calls[0][3];

    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Routing configuration saved.");
    expect(vi.mocked(api.updateInboxRouting).mock.calls[1][3]).toBe(firstKey);
    expect(token).toHaveValue("");
  });

  it("uses a new idempotency key when the Telegram token changes after a network failure", async () => {
    vi.mocked(api.updateInboxRouting).mockRejectedValueOnce(new TypeError("Failed to fetch"));
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /^Telegram alerts:/ }));
    const token = screen.getByLabelText("Bot token");
    fireEvent.change(token, { target: { value: "123456:first-private-routing-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save routing");
    const firstKey = vi.mocked(api.updateInboxRouting).mock.calls[0][3];

    fireEvent.change(token, { target: { value: "123456:second-private-routing-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    expect(await screen.findByRole("status")).toHaveTextContent("Routing configuration saved.");
    expect(vi.mocked(api.updateInboxRouting).mock.calls[1][3]).not.toBe(firstKey);
    expect(vi.mocked(api.updateInboxRouting).mock.calls[1][2].telegram_bot_token).toEqual({
      action: "replace",
      value: "123456:second-private-routing-token",
    });
  });

  it("supports Telegram-only routing with no queues and disables Add rule until a default queue exists", async () => {
    vi.mocked(api.getInboxRouting).mockResolvedValueOnce({
      ...routingResponse,
      configuration: {
        ...routingResponse.configuration,
        configured: false,
        version: 0,
        enabled: false,
        assignment_strategy: "manual",
        default_queue_id: null,
        queues: [],
        rules: [],
        coverage: {
          timezone: "UTC",
          weekly_intervals: [],
          outside_hours_action: "ai",
          no_operator_action: "ai",
        },
        sla: {
          clock: "elapsed",
          unassigned_warning_seconds: null,
          first_response_seconds: null,
          resolution_seconds: null,
          escalation_queue_id: null,
        },
        telegram: {
          new_visitor: false,
          new_message: false,
          operator_request: false,
          unassigned_warning: false,
          sla_breach: false,
          chat_ids: [],
          bot_token_configured: false,
        },
      },
    });
    renderView();

    const addRule = await screen.findByRole("button", { name: "Add rule" });
    expect(addRule).toBeDisabled();
    expect(await screen.findByText(/create and select an active default queue/i)).toBeInTheDocument();
    expect(addRule).toHaveAttribute("title", expect.stringContaining("active default queue"));
    fireEvent.click(screen.getByRole("button", { name: /^Telegram alerts:/ }));
    expect(screen.getByText(/these settings control Telegram alerts/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /^New message/ }));
    fireEvent.change(screen.getByLabelText("Recipient chat IDs"), {
      target: { value: "-1001234567890\n" },
    });
    fireEvent.change(screen.getByLabelText("Bot token"), {
      target: { value: "123456:private-routing-token" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    await waitFor(() => expect(api.updateInboxRouting).toHaveBeenCalled());
    const request = vi.mocked(api.updateInboxRouting).mock.calls[0][2];
    expect(request.expected_version).toBe(0);
    expect(request.configuration.queues).toEqual([]);
    expect(request.configuration.default_queue_id).toBeNull();
    expect(request.configuration.telegram.chat_ids).toEqual(["-1001234567890"]);
  });

  it("keeps selectors and editor fields disabled until an in-flight save finishes", async () => {
    let finishSave: (() => void) | undefined;
    vi.mocked(api.updateInboxRouting).mockImplementationOnce(async (_auth, _inboxId, input) => (
      new Promise<api.InboxRoutingResponse>((resolve) => {
        finishSave = () => resolve(savedResponse(input));
      })
    ));
    renderView({ availableInboxes: [inbox, secondInbox] });

    const language = await screen.findByLabelText("And language is");
    fireEvent.change(language, { target: { value: "ru" } });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    await waitFor(() => expect(api.updateInboxRouting).toHaveBeenCalledTimes(1));
    expect(screen.getByLabelText("Inbox")).toBeDisabled();
    expect(language).toBeDisabled();
    finishSave?.();
    expect(await screen.findByRole("status")).toHaveTextContent("Routing configuration saved.");
  });

  it("does not let an old dirty draft save into a newly selected Inbox while it loads", async () => {
    let finishLoad: ((value: api.InboxRoutingResponse) => void) | undefined;
    renderView({ availableInboxes: [inbox, secondInbox] });

    fireEvent.change(await screen.findByLabelText("And language is"), { target: { value: "ru" } });
    vi.spyOn(window, "confirm").mockReturnValue(true);
    vi.mocked(api.getInboxRouting).mockImplementationOnce(() => (
      new Promise<api.InboxRoutingResponse>((resolve) => {
        finishLoad = resolve;
      })
    ));
    fireEvent.change(screen.getByLabelText("Inbox"), { target: { value: secondInbox.id } });

    await waitFor(() => expect(api.getInboxRouting).toHaveBeenLastCalledWith(auth, secondInbox.id));
    expect(screen.getByLabelText("Inbox")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save routing" })).toBeDisabled();
    expect(api.updateInboxRouting).not.toHaveBeenCalled();

    finishLoad?.({
      ...routingResponse,
      configuration: { ...routingResponse.configuration, inbox_id: secondInbox.id },
    });
    await waitFor(() => expect(screen.getByLabelText("Inbox")).toBeEnabled());
  });

  it("shows and removes a saved queue member that is no longer eligible", async () => {
    const unavailableId = "00000000-0000-4000-8000-000000000059";
    vi.mocked(api.getInboxRouting).mockResolvedValueOnce({
      ...routingResponse,
      configuration: {
        ...routingResponse.configuration,
        queues: [
          { ...routingResponse.configuration.queues[0], member_ids: [nadiaId, unavailableId] },
          routingResponse.configuration.queues[1],
        ],
      },
    });
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /^Priority: 2 operators/ }));
    fireEvent.click(screen.getByRole("button", { name: `Remove unavailable operator ${unavailableId}` }));
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    await waitFor(() => expect(api.updateInboxRouting).toHaveBeenCalled());
    expect(vi.mocked(api.updateInboxRouting).mock.calls[0][2].configuration.queues[0].member_ids)
      .toEqual([nadiaId]);
  });

  it("warns before unloading a dirty workflow and reports dirty state to the app shell", async () => {
    const onDirtyChange = vi.fn();
    renderView({ onDirtyChange });

    fireEvent.change(await screen.findByLabelText("And language is"), { target: { value: "ru" } });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));
    const event = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  });

  it("opens the inbox named by the page URL and records inbox changes in the URL", async () => {
    renderView({ availableInboxes: [inbox, secondInbox], segments: [secondInbox.id] });

    await waitFor(() => expect(api.getInboxRouting).toHaveBeenCalledWith(auth, secondInbox.id));
    const inboxSelect = screen.getByLabelText("Inbox");
    await waitFor(() => expect(inboxSelect).toBeEnabled());
    expect(inboxSelect).toHaveValue(secondInbox.id);
    expect(api.getInboxRouting).not.toHaveBeenCalledWith(auth, inbox.id);

    fireEvent.change(inboxSelect, { target: { value: inbox.id } });
    expect(pageRoute()).toBe("");
    await waitFor(() => expect(api.getInboxRouting).toHaveBeenLastCalledWith(auth, inbox.id));
    await waitFor(() => expect(inboxSelect).toBeEnabled());
    expect(inboxSelect).toHaveValue(inbox.id);

    fireEvent.change(inboxSelect, { target: { value: secondInbox.id } });
    expect(pageRoute()).toBe(secondInbox.id);
    await waitFor(() => expect(api.getInboxRouting).toHaveBeenLastCalledWith(auth, secondInbox.id));
  });

  it.each([
    [[inbox.id], "", inbox.id],
    [["00000000-0000-4000-8000-000000000098"], "", inbox.id],
    [[secondInbox.id, "extra"], secondInbox.id, secondInbox.id],
  ])("replaces the stale routing URL %j with the inbox it shows", async (segments, canonical, shownInboxId) => {
    renderView({ availableInboxes: [inbox, secondInbox], segments });

    await waitFor(() => expect(pageRoute()).toBe(canonical));
    await waitFor(() => expect(screen.getByLabelText("Inbox")).toBeEnabled());
    expect(screen.getByLabelText("Inbox")).toHaveValue(shownInboxId);
    expect(api.getInboxRouting).toHaveBeenLastCalledWith(auth, shownInboxId);
  });

  it("discards the unsaved draft at once when Back or Forward switches the inbox", async () => {
    const onDirtyChange = vi.fn();
    const confirm = vi.spyOn(window, "confirm");
    renderView({ availableInboxes: [inbox, secondInbox], onDirtyChange, historyTarget: [secondInbox.id] });

    vi.mocked(api.getInboxRouting).mockImplementation(async (_auth, inboxId) => inboxId === secondInbox.id ? {
      ...routingResponse,
      configuration: {
        ...routingResponse.configuration,
        inbox_id: secondInbox.id,
        rules: [{ ...routingResponse.configuration.rules[0], language: "de" }],
      },
    } : routingResponse);
    fireEvent.change(await screen.findByLabelText("And language is"), { target: { value: "ru" } });
    await waitFor(() => expect(onDirtyChange).toHaveBeenLastCalledWith(true));

    fireEvent.click(screen.getByRole("button", { name: "Move in history" }));

    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
    expect(screen.getByRole("button", { name: "Save routing" })).toBeDisabled();
    const unload = new Event("beforeunload", { cancelable: true });
    window.dispatchEvent(unload);
    expect(unload.defaultPrevented).toBe(false);
    await waitFor(() => expect(api.getInboxRouting).toHaveBeenLastCalledWith(auth, secondInbox.id));
    expect(await screen.findByLabelText("And language is")).toHaveValue("de");
    expect(screen.getByLabelText("Inbox")).toHaveValue(secondInbox.id);
    expect(screen.getByRole("button", { name: "Save routing" })).toBeDisabled();
    expect(onDirtyChange).toHaveBeenLastCalledWith(false);
    expect(confirm).not.toHaveBeenCalled();
    expect(api.updateInboxRouting).not.toHaveBeenCalled();
  });

  it("edits working-hours intervals through the time fields and saves them as HH:mm", async () => {
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /^Hours & AI fallback:/ }));
    const startsAt = screen.getByLabelText("Starts at");
    const endsAt = screen.getByLabelText("Ends at");
    expect(startsAt).toHaveTextContent("9:00 AM");
    expect(dateTimeFieldInput(startsAt)).toHaveValue("09:00");
    expect(dateTimeFieldInput(endsAt)).toHaveValue("18:00");
    expect(within(startsAt.closest(".routing-interval-row") as HTMLElement).queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();

    changeDateTimeField(startsAt, "10:30");
    expect(startsAt).toHaveTextContent("10:30 AM");
    fireEvent.click(endsAt);
    const picker = within(screen.getByRole("dialog", { name: "Choose time" }));
    fireEvent.click(within(picker.getByRole("listbox", { name: "Hours" })).getByRole("option", { name: /^7\sPM$/ }));
    fireEvent.click(picker.getByRole("button", { name: "Done" }));
    expect(dateTimeFieldInput(endsAt)).toHaveValue("19:00");
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    await waitFor(() => expect(api.updateInboxRouting).toHaveBeenCalledTimes(1));
    expect(vi.mocked(api.updateInboxRouting).mock.calls[0][2].configuration.coverage.weekly_intervals).toEqual([
      { id: intervalId, weekday: 1, starts_at: "10:30", ends_at: "19:00" },
    ]);
  });

  it("rejects a working-hours SLA target beyond 520 weeks of the configured schedule", async () => {
    vi.mocked(api.getInboxRouting).mockResolvedValueOnce({
      ...routingResponse,
      configuration: {
        ...routingResponse.configuration,
        coverage: {
          ...routingResponse.configuration.coverage,
          weekly_intervals: [{
            ...routingResponse.configuration.coverage.weekly_intervals[0],
            ends_at: "09:01",
          }],
        },
      },
    });
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /^SLA:/ }));
    fireEvent.change(screen.getByLabelText("Resolution, min"), { target: { value: "600" } });
    fireEvent.click(screen.getByRole("button", { name: "Save routing" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "An SLA target exceeds 520 weeks of the configured working schedule.",
    );
    expect(api.updateInboxRouting).not.toHaveBeenCalled();
  });
});
