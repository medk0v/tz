import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { I18nContext, createI18n } from "../i18n";
import { CurrentPageRoute, MemoryPageRoute } from "./MemoryPageRoute";
import { OnlineVisitorsView } from "./OnlineVisitorsView";
import { usePageRoute } from "./page-route";

vi.mock("../api", () => ({
  listOnlineVisitorWidgets: vi.fn(),
  listOnlineVisitors: vi.fn(),
  startVisitorConversation: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const firstInbox: api.Inbox = {
  id: "00000000-0000-4000-8000-000000000010",
  project_id: "00000000-0000-4000-8000-000000000011",
  name: "Customer support",
  status: "active",
  created_at: "2026-08-29T00:00:00Z",
};
const secondInbox: api.Inbox = {
  ...firstInbox,
  id: "00000000-0000-4000-8000-000000000012",
  name: "Sales",
};
const firstWidget: api.OnlineVisitorWidget = {
  id: "00000000-0000-4000-8000-000000000020",
  inbox_id: firstInbox.id,
  name: "Website chat",
};
const secondWidget: api.OnlineVisitorWidget = {
  id: "00000000-0000-4000-8000-000000000021",
  inbox_id: secondInbox.id,
  name: "Website chat",
};
const emptyVisitors: api.OnlineVisitorList = {
  items: [],
  online_window_seconds: 90,
};

interface RenderViewOptions {
  activeInboxId?: string | null;
  realtimeEvent?: api.RealtimeEvent | null;
  onInboxChange?: (inboxId: string) => void;
  canStartConversation?: boolean;
  onOpenConversation?: (conversationId: string, inboxId: string) => void;
}

function viewNode({
  activeInboxId = firstInbox.id,
  realtimeEvent = null,
  onInboxChange = vi.fn(),
  canStartConversation = true,
  onOpenConversation = vi.fn(),
}: RenderViewOptions = {}) {
  return (
    <I18nContext.Provider value={createI18n("en", vi.fn())}>
      <OnlineVisitorsView
        auth={auth}
        inboxes={[firstInbox, secondInbox]}
        activeInboxId={activeInboxId}
        realtimeEvent={realtimeEvent}
        onInboxChange={onInboxChange}
        canStartConversation={canStartConversation}
        onOpenConversation={onOpenConversation}
      />
    </I18nContext.Provider>
  );
}

/** The page as the app shows it: the active Inbox follows `onInboxChange`, and the URL can move like Back/Forward. */
function RoutedOnlineVisitors({ onInboxChange, historyTarget }: { onInboxChange: (inboxId: string) => void; historyTarget: string[] }) {
  const [activeInboxId, setActiveInboxId] = useState<string | null>(firstInbox.id);
  const route = usePageRoute();
  return <>
    <OnlineVisitorsView auth={auth} inboxes={[firstInbox, secondInbox]} activeInboxId={activeInboxId} realtimeEvent={null} onInboxChange={(inboxId) => { onInboxChange(inboxId); setActiveInboxId(inboxId); }} canStartConversation onOpenConversation={vi.fn()} />
    <button type="button" onClick={() => void route.navigate(historyTarget)}>Move in history</button>
    <CurrentPageRoute />
  </>;
}

function renderRouted(segments: string[], { onInboxChange = vi.fn(), historyTarget = [] as string[] } = {}) {
  render(<I18nContext.Provider value={createI18n("en", vi.fn())}><MemoryPageRoute initialSegments={segments}><RoutedOnlineVisitors onInboxChange={onInboxChange} historyTarget={historyTarget} /></MemoryPageRoute></I18nContext.Provider>);
}

const pageRoute = () => screen.getByTestId("page-route").textContent;

describe("OnlineVisitorsView", () => {
  beforeEach(() => {
    vi.mocked(api.listOnlineVisitorWidgets).mockResolvedValue([secondWidget, firstWidget]);
    vi.mocked(api.listOnlineVisitors).mockResolvedValue(emptyVisitors);
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  const visitor: api.OnlineVisitor = {
    session_id: "visitor-session",
    contact_id: "visitor-contact",
    channel_id: firstWidget.id,
    widget_name: firstWidget.name,
    conversation_id: null,
    client_ip: null,
    user_agent: null,
    geo_ip: null,
    user_agent_details: null,
    origin: "https://example.test",
    page_url: null,
    page_title: null,
    referrer: null,
    language: "en",
    first_seen_at: "2026-09-02T00:00:00Z",
    last_seen_at: "2026-09-02T00:00:00Z",
  };

  it.each([null, "existing-conversation"])("starts or reuses the visitor's conversation (%s) and opens the returned Inbox", async (conversationId) => {
    vi.mocked(api.listOnlineVisitors).mockResolvedValue({ ...emptyVisitors, items: [{ ...visitor, conversation_id: conversationId }] });
    let finish!: (value: { id: string; inbox_id: string }) => void;
    vi.mocked(api.startVisitorConversation).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const onOpenConversation = vi.fn();
    render(viewNode({ onOpenConversation }));

    fireEvent.click(await screen.findByRole("button", { name: "Start conversation" }));
    expect(screen.getByRole("button", { name: "Opening…" })).toBeDisabled();
    expect(onOpenConversation).not.toHaveBeenCalled();
    expect(api.startVisitorConversation).toHaveBeenCalledExactlyOnceWith(auth, visitor.session_id);
    await act(async () => finish({ id: "ready-conversation", inbox_id: firstInbox.id }));
    expect(onOpenConversation).toHaveBeenCalledExactlyOnceWith("ready-conversation", firstInbox.id);
  });

  it("keeps the visitor list available when starting fails", async () => {
    vi.mocked(api.listOnlineVisitors).mockResolvedValue({ ...emptyVisitors, items: [visitor] });
    vi.mocked(api.startVisitorConversation).mockRejectedValueOnce(new Error("Visitor went offline"));
    const onOpenConversation = vi.fn();
    render(viewNode({ onOpenConversation }));
    fireEvent.click(await screen.findByRole("button", { name: "Start conversation" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not start the conversation");
    expect(screen.getByRole("button", { name: "Start conversation" })).toBeEnabled();
    expect(onOpenConversation).not.toHaveBeenCalled();
  });

  it("does not offer starting a conversation without reply permission", async () => {
    vi.mocked(api.listOnlineVisitors).mockResolvedValue({ ...emptyVisitors, items: [visitor] });
    render(viewNode({ canStartConversation: false }));
    await screen.findByText("Not started");
    expect(screen.queryByRole("button", { name: "Start conversation" })).not.toBeInTheDocument();
    expect(api.startVisitorConversation).not.toHaveBeenCalled();
  });

  it("selects the widget in the active Inbox and switches Inbox with another widget", async () => {
    const onInboxChange = vi.fn();
    render(viewNode({ onInboxChange }));

    const widgetSelect = await screen.findByRole("combobox", { name: "Widget" });
    await waitFor(() => expect(widgetSelect).toHaveValue(firstWidget.id));
    await waitFor(() => expect(api.listOnlineVisitors).toHaveBeenCalledWith(auth, firstWidget.id));
    expect(onInboxChange).not.toHaveBeenCalled();
    expect(screen.getByRole("option", { name: "Website chat — Customer support" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Website chat — Sales" })).toBeInTheDocument();

    fireEvent.change(widgetSelect, { target: { value: secondWidget.id } });

    expect(onInboxChange).toHaveBeenCalledWith(secondInbox.id);
    await waitFor(() => expect(api.listOnlineVisitors).toHaveBeenLastCalledWith(auth, secondWidget.id));
    expect(widgetSelect).toHaveValue(secondWidget.id);
  });

  it("moves realtime scope when the active Inbox has no widget", async () => {
    const onInboxChange = vi.fn();
    vi.mocked(api.listOnlineVisitorWidgets).mockResolvedValue([secondWidget]);

    render(viewNode({ onInboxChange }));

    await waitFor(() => expect(onInboxChange).toHaveBeenCalledWith(secondInbox.id));
    await waitFor(() => expect(api.listOnlineVisitors).toHaveBeenCalledWith(auth, secondWidget.id));
    expect(screen.getByRole("combobox", { name: "Widget" })).toHaveValue(secondWidget.id);
  });

  it("reloads only for a visitor event from the selected widget", async () => {
    vi.mocked(api.listOnlineVisitorWidgets).mockResolvedValue([firstWidget]);
    const rendered = render(viewNode());
    await waitFor(() => expect(api.listOnlineVisitors).toHaveBeenCalledTimes(1));

    rendered.rerender(viewNode({
      realtimeEvent: {
        event_id: "00000000-0000-4000-8000-000000000030",
        inbox_id: firstInbox.id,
        type: "visitor.entered",
        aggregate_id: "00000000-0000-4000-8000-000000000031",
        data: { channel_id: secondWidget.id },
      },
    }));
    await act(async () => {
      await new Promise((resolve) => window.setTimeout(resolve, 5));
    });
    expect(api.listOnlineVisitors).toHaveBeenCalledTimes(1);

    rendered.rerender(viewNode({
      realtimeEvent: {
        event_id: "00000000-0000-4000-8000-000000000032",
        inbox_id: firstInbox.id,
        type: "visitor.entered",
        aggregate_id: "00000000-0000-4000-8000-000000000033",
        data: { channel_id: firstWidget.id },
      },
    }));

    await waitFor(() => expect(api.listOnlineVisitors).toHaveBeenCalledTimes(2));
  });

  it("shows a distinct empty state when no active widgets are available", async () => {
    vi.mocked(api.listOnlineVisitorWidgets).mockResolvedValue([]);

    render(viewNode());

    const widgetSelect = await screen.findByRole("combobox", { name: "Widget" });
    await waitFor(() => expect(widgetSelect).toBeDisabled());
    expect(screen.getAllByText("No active website widgets are available.")).not.toHaveLength(0);
    expect(screen.getByRole("button", { name: "Refresh online visitors" })).toBeDisabled();
    expect(api.listOnlineVisitors).not.toHaveBeenCalled();
  });

  it("opens the widget named by the page URL and moves the active Inbox to it", async () => {
    const onInboxChange = vi.fn();
    renderRouted([secondWidget.id], { onInboxChange });

    const widgetSelect = await screen.findByRole("combobox", { name: "Widget" });
    await waitFor(() => expect(widgetSelect).toHaveValue(secondWidget.id));
    await waitFor(() => expect(api.listOnlineVisitors).toHaveBeenCalledWith(auth, secondWidget.id));
    expect(api.listOnlineVisitors).not.toHaveBeenCalledWith(auth, firstWidget.id);
    expect(onInboxChange).toHaveBeenCalledExactlyOnceWith(secondInbox.id);
    expect(pageRoute()).toBe(secondWidget.id);
  });

  it("names the shown widget in the URL and records widget changes", async () => {
    renderRouted([]);

    await waitFor(() => expect(pageRoute()).toBe(firstWidget.id));
    fireEvent.change(screen.getByRole("combobox", { name: "Widget" }), { target: { value: secondWidget.id } });
    expect(pageRoute()).toBe(secondWidget.id);
    await waitFor(() => expect(api.listOnlineVisitors).toHaveBeenLastCalledWith(auth, secondWidget.id));
  });

  it("shows the other widget's visitors and Inbox after Back or Forward", async () => {
    vi.mocked(api.listOnlineVisitors).mockImplementation(async (_auth, widgetId) => widgetId === secondWidget.id
      ? { ...emptyVisitors, items: [{ ...visitor, channel_id: secondWidget.id, page_title: "Pricing" }] }
      : emptyVisitors);
    const onInboxChange = vi.fn();
    renderRouted([secondWidget.id], { onInboxChange, historyTarget: [firstWidget.id] });
    expect(await screen.findByText("Pricing")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Move in history" }));

    expect(screen.getByRole("combobox", { name: "Widget" })).toHaveValue(firstWidget.id);
    expect(screen.queryByText("Pricing")).not.toBeInTheDocument();
    expect(onInboxChange).toHaveBeenLastCalledWith(firstInbox.id);
    await waitFor(() => expect(api.listOnlineVisitors).toHaveBeenLastCalledWith(auth, firstWidget.id));
    expect(await screen.findByText("No visitors are online right now.")).toBeInTheDocument();
  });

  it.each([
    [["00000000-0000-4000-8000-000000000029"], firstWidget.id],
    [[secondWidget.id, "extra"], secondWidget.id],
  ])("replaces the stale online visitors URL %j with the widget it shows", async (segments, canonical) => {
    renderRouted(segments);
    await waitFor(() => expect(pageRoute()).toBe(canonical));
    expect(screen.getByRole("combobox", { name: "Widget" })).toHaveValue(canonical);
  });

  it("leaves no widget in the URL when no widgets are available, but keeps it while they cannot load", async () => {
    vi.mocked(api.listOnlineVisitorWidgets).mockRejectedValueOnce(new Error("offline")).mockResolvedValue([]);
    renderRouted([firstWidget.id]);
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load online visitors");
    expect(pageRoute()).toBe(firstWidget.id);
    cleanup();

    renderRouted([firstWidget.id]);
    await waitFor(() => expect(pageRoute()).toBe(""));
  });
});
