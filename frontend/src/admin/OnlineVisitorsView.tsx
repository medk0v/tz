import { DemoActionButton } from "./DemoReadOnly";
import { useCallback, useEffect, useRef, useState } from "react";
import { useContentMotion } from "./useContentMotion";
import { RefreshCw } from "lucide-react";
import {
  listOnlineVisitorWidgets,
  listOnlineVisitors,
  startVisitorConversation,
  type Inbox,
  type OnlineVisitor,
  type OnlineVisitorWidget,
  type OperatorAuth,
  type RealtimeEvent,
} from "../api";
import { useI18n, type MessageKey } from "../i18n";
import { useCanonicalPageRoute, usePageRoute } from "./page-route";
import "./DirectoryWorkbench.css";

const DEVICE_CATEGORY_LABELS: Record<string, MessageKey> = {
  pc: "visitor.deviceCategory.desktop",
  smartphone: "visitor.deviceCategory.mobile",
  mobilephone: "visitor.deviceCategory.mobile",
  appliance: "visitor.deviceCategory.appliance",
  crawler: "visitor.deviceCategory.crawler",
  misc: "visitor.deviceCategory.other",
};

interface OnlineVisitorsViewProps {
  auth: OperatorAuth;
  inboxes: Inbox[];
  activeInboxId: string | null;
  realtimeEvent: RealtimeEvent | null;
  onInboxChange: (inboxId: string) => void;
  canStartConversation: boolean;
  onOpenConversation: (conversationId: string, inboxId: string) => void;
}

function shortId(value: string): string {
  return value.slice(0, 8);
}

export function OnlineVisitorsView({
  auth,
  inboxes,
  activeInboxId,
  realtimeEvent,
  onInboxChange,
  canStartConversation,
  onOpenConversation,
}: OnlineVisitorsViewProps) {
  const { t, formatDate } = useI18n();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const [widgets, setWidgets] = useState<OnlineVisitorWidget[]>([]);
  const [widgetsLoaded, setWidgetsLoaded] = useState(false);
  // The widget shown when the URL names none: the active Inbox's widget when the widgets loaded.
  const [defaultWidgetId, setDefaultWidgetId] = useState<string | null>(null);
  const selectedWidget = widgets.find((widget) => widget.id === route.segments[0])
    ?? widgets.find((widget) => widget.id === defaultWidgetId)
    ?? null;
  const selectedWidgetId = selectedWidget?.id ?? null;
  const selectedWidgetInboxId = selectedWidget?.inbox_id ?? null;
  const [displayedWidgetId, setDisplayedWidgetId] = useState<string | null>(null);
  const visitorsMotion = useContentMotion<HTMLDivElement>(displayedWidgetId ?? "none");
  const [visitors, setVisitors] = useState<OnlineVisitor[]>([]);
  const [onlineWindowSeconds, setOnlineWindowSeconds] = useState(90);
  const [loadingWidgets, setLoadingWidgets] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<MessageKey | null>(null);
  const [startingSessionId, setStartingSessionId] = useState<string | null>(null);
  const [startError, setStartError] = useState(false);
  const startRequestIdRef = useRef(0);
  const visitorRequestIdRef = useRef(0);
  const activeInboxIdRef = useRef(activeInboxId);
  const onInboxChangeRef = useRef(onInboxChange);
  const duplicateWidgetNames = new Set(
    widgets
      .filter((widget, index) => widgets.findIndex((item) => item.name === widget.name) !== index)
      .map((widget) => widget.name),
  );

  // The widget changes through the URL, also by Back/Forward; the list then waits for that widget's visitors.
  const [shownWidgetId, setShownWidgetId] = useState(selectedWidgetId);
  if (selectedWidgetId !== shownWidgetId) {
    setShownWidgetId(selectedWidgetId);
    setVisitors([]);
    setLoading(selectedWidgetId !== null);
    setStartError(false);
  }

  useCanonicalPageRoute(route, selectedWidgetId ? [selectedWidgetId] : [], widgetsLoaded);

  useEffect(() => {
    activeInboxIdRef.current = activeInboxId;
  }, [activeInboxId]);

  useEffect(() => {
    onInboxChangeRef.current = onInboxChange;
  }, [onInboxChange]);

  // The active Inbox follows the shown widget, so that the widget's realtime visitor events arrive.
  useEffect(() => {
    if (selectedWidgetInboxId && selectedWidgetInboxId !== activeInboxIdRef.current) {
      onInboxChangeRef.current(selectedWidgetInboxId);
    }
  }, [selectedWidgetInboxId]);

  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      visitorRequestIdRef.current += 1;
      startRequestIdRef.current += 1;
      setWidgets([]);
      setWidgetsLoaded(false);
      setDefaultWidgetId(null);
      setVisitors([]);
      setLoadingWidgets(true);
      setLoading(false);
      setError(null);
      setStartError(false);
      setStartingSessionId(null);
      void listOnlineVisitorWidgets(auth)
        .then((items) => {
          if (cancelled) return;
          const initialWidget = items.find((widget) => widget.inbox_id === activeInboxIdRef.current)
            ?? items[0]
            ?? null;
          setWidgets(items);
          setWidgetsLoaded(true);
          setDefaultWidgetId(initialWidget?.id ?? null);
          setLoadingWidgets(false);
        })
        .catch(() => {
          if (cancelled) return;
          setWidgets([]);
          setDefaultWidgetId(null);
          setVisitors([]);
          setLoadingWidgets(false);
          setLoading(false);
          setError("onlineVisitors.loadError");
        });
    }, 0);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      visitorRequestIdRef.current += 1;
      startRequestIdRef.current += 1;
    };
  }, [auth]);

  const loadVisitors = useCallback(async (showLoading = false) => {
    const widgetId = selectedWidgetId;
    const requestId = visitorRequestIdRef.current + 1;
    visitorRequestIdRef.current = requestId;
    if (!widgetId) {
      setVisitors([]);
      setLoading(false);
      return;
    }
    if (showLoading) setLoading(true);
    setError(null);
    try {
      const response = await listOnlineVisitors(auth, widgetId);
      if (requestId !== visitorRequestIdRef.current) return;
      setVisitors(response.items);
      setDisplayedWidgetId(widgetId);
      setOnlineWindowSeconds(response.online_window_seconds);
    } catch {
      if (requestId !== visitorRequestIdRef.current) return;
      setError("onlineVisitors.loadError");
    } finally {
      if (requestId === visitorRequestIdRef.current) setLoading(false);
    }
  }, [auth, selectedWidgetId]);

  useEffect(() => {
    const initialTimer = window.setTimeout(() => void loadVisitors(true), 0);
    const timer = window.setInterval(() => void loadVisitors(), 30_000);
    return () => {
      window.clearTimeout(initialTimer);
      window.clearInterval(timer);
      visitorRequestIdRef.current += 1;
    };
  }, [loadVisitors]);

  useEffect(() => {
    if (
      realtimeEvent?.type === "visitor.entered"
      && realtimeEvent.data.channel_id === selectedWidgetId
    ) {
      const timer = window.setTimeout(() => void loadVisitors(), 0);
      return () => window.clearTimeout(timer);
    }
    return undefined;
  }, [loadVisitors, realtimeEvent, selectedWidgetId]);

  function changeWidget(widgetId: string) {
    const widget = widgets.find((item) => item.id === widgetId);
    if (!widget || widget.id === selectedWidgetId) return;
    visitorRequestIdRef.current += 1;
    void navigateRoute([widget.id]);
  }

  async function startConversation(visitor: OnlineVisitor) {
    if (!canStartConversation || startingSessionId) return;
    const requestId = ++startRequestIdRef.current;
    setStartingSessionId(visitor.session_id);
    setStartError(false);
    try {
      const conversation = await startVisitorConversation(auth, visitor.session_id);
      if (requestId !== startRequestIdRef.current) return;
      onOpenConversation(conversation.id, conversation.inbox_id);
    } catch {
      if (requestId === startRequestIdRef.current) setStartError(true);
    } finally {
      if (requestId === startRequestIdRef.current) setStartingSessionId(null);
    }
  }

  return (
    <div className="page directory-page online-visitors-page">
      <header className="page-toolbar directory-page-toolbar">
        <div>
          <h1>
            {t("onlineVisitors.title")} <span className="online-visitors-count">({visitors.length})</span>
          </h1>
          <p>{t("onlineVisitors.description", { seconds: onlineWindowSeconds })}</p>
        </div>
        <div className="toolbar-actions">
          <label className="compact-field">
            <span>{t("onlineVisitors.widgetFilter")}</span>
            <select
              value={selectedWidgetId ?? ""}
              disabled={loadingWidgets || widgets.length === 0 || startingSessionId !== null}
              onChange={(event) => changeWidget(event.target.value)}
            >
              {!loadingWidgets && widgets.length === 0 && <option value="">{t("onlineVisitors.noWidgets")}</option>}
              {widgets.map((widget) => {
                const inboxName = inboxes.find((inbox) => inbox.id === widget.inbox_id)?.name;
                const label = duplicateWidgetNames.has(widget.name) && inboxName
                  ? `${widget.name} — ${inboxName}`
                  : widget.name;
                return <option value={widget.id} key={widget.id}>{label}</option>;
              })}
            </select>
          </label>
          <button className="icon-button" type="button" aria-label={t("onlineVisitors.refresh")} disabled={loadingWidgets || loading || !selectedWidgetId} onClick={() => void loadVisitors(true)}>
            <RefreshCw size={17} />
          </button>
        </div>
      </header>

      {error && <div className="admin-notice admin-notice--error" role="alert">{t(error)}</div>}
      {startError && <div className="admin-notice admin-notice--error" role="alert">{t("onlineVisitors.startError")}</div>}

      <div className="table-panel directory-page-table online-visitors-table" aria-busy={loadingWidgets || loading}>
        <div className="table-panel-header">
          <strong>{t("onlineVisitors.list")}</strong>
        </div>
        <div ref={visitorsMotion} className="online-visitors-table-scroll">
          <table>
            <colgroup>
              <col className="online-visitors-column--visitor" />
              <col className="online-visitors-column--widget" />
              <col className="online-visitors-column--page" />
              <col className="online-visitors-column--source" />
              <col className="online-visitors-column--ip" />
              <col className="online-visitors-column--timezone" />
              <col className="online-visitors-column--environment" />
              <col className="online-visitors-column--activity" />
              <col className="online-visitors-column--dialog" />
            </colgroup>
            <thead>
              <tr>
                <th>{t("onlineVisitors.visitor")}</th>
                <th>{t("onlineVisitors.widget")}</th>
                <th>{t("onlineVisitors.page")}</th>
                <th>{t("onlineVisitors.source")}</th>
                <th>{t("onlineVisitors.ip")}</th>
                <th title={t("visitor.geoTimezone")}>{t("onlineVisitors.timezone")}</th>
                <th>{t("onlineVisitors.environment")}</th>
                <th>{t("onlineVisitors.lastSeen")}</th>
                <th>{t("onlineVisitors.dialog")}</th>
              </tr>
            </thead>
            <tbody>
              {visitors.map((visitor) => {
                const geo = visitor.geo_ip;
                const location = geo
                  ? [
                      geo.city,
                      geo.country
                        ? `${geo.country}${geo.country_code ? ` (${geo.country_code})` : ""}`
                        : geo.country_code,
                    ].filter(Boolean).join(", ")
                  : "";
                const parsed = visitor.user_agent_details;
                const browser = parsed
                  ? [parsed.browser, parsed.browser_version].filter(Boolean).join(" ")
                  : "";
                const category = parsed?.device_category
                  ? t(DEVICE_CATEGORY_LABELS[parsed.device_category] ?? "visitor.deviceCategory.other")
                  : "";
                const operatingSystem = parsed
                  ? [parsed.operating_system, parsed.operating_system_version].filter(Boolean).join(" ")
                  : "";
                const environment = parsed
                  ? [operatingSystem, category].filter(Boolean).join(" · ")
                  : "";
                return (
                  <tr key={`${visitor.contact_id}:${visitor.channel_id}`}>
                    <td>
                      <div className="online-visitor-identity">
                        <strong>{t("onlineVisitors.anonymous", { id: shortId(visitor.contact_id) })}</strong>
                        <span>{t("onlineVisitors.firstSeen", {
                          date: formatDate(visitor.first_seen_at, {
                            month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
                          }),
                        })}</span>
                      </div>
                    </td>
                    <td>{visitor.widget_name}</td>
                    <td>
                      <div className="online-visitor-page">
                        <strong>{visitor.page_title || t("onlineVisitors.untitledPage")}</strong>
                        <span title={visitor.page_url ?? visitor.origin}>{visitor.page_url ?? visitor.origin}</span>
                      </div>
                    </td>
                    <td>{visitor.referrer ?? t("onlineVisitors.direct")}</td>
                    <td>
                      <div className="online-visitor-identity">
                        <strong className="mono-cell" title={visitor.client_ip ?? undefined}>{visitor.client_ip ?? "—"}</strong>
                        <span>{location || "—"}</span>
                      </div>
                    </td>
                    <td>{geo?.time_zone || "—"}</td>
                    <td title={visitor.user_agent ?? undefined}>
                      <div className="online-visitor-identity">
                        <strong>{browser || "—"}</strong>
                        <span>{environment || visitor.user_agent || "—"}</span>
                      </div>
                    </td>
                    <td>{formatDate(visitor.last_seen_at, { hour: "2-digit", minute: "2-digit", second: "2-digit" })}</td>
                    <td>
                      <div className="online-visitor-dialog">
                        <span>{visitor.conversation_id ? t("onlineVisitors.dialogStarted") : t("onlineVisitors.noDialog")}</span>
                        {canStartConversation && (
                          <DemoActionButton
                            className="secondary-button online-visitor-start"
                            type="button"
                            disabled={startingSessionId !== null}
                            onClick={() => void startConversation(visitor)}
                          >
                            {t(startingSessionId === visitor.session_id
                              ? "onlineVisitors.starting"
                              : "onlineVisitors.startConversation")}
                          </DemoActionButton>
                        )}
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        {!loadingWidgets && !loading && selectedWidgetId && visitors.length === 0 && !error && (
          <div className="empty-state">{t("onlineVisitors.empty")}</div>
        )}
        {!loadingWidgets && !loading && !selectedWidgetId && !error && (
          <div className="empty-state">{t("onlineVisitors.noWidgets")}</div>
        )}
        {(loadingWidgets || loading) && visitors.length === 0 && (
          <div className="empty-state" role="status">{t("onlineVisitors.loading")}</div>
        )}
      </div>
    </div>
  );
}
