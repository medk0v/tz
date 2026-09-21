import { useEffect, useId, useMemo, useState } from "react";
import { MessageSquare, RefreshCw, Search, Star } from "lucide-react";
import {
  getSupportQuality,
  type Inbox,
  type OperatorAuth,
  type SupportQualityItem,
  type SupportQualityReport,
} from "../api";
import { useI18n, type MessageKey, type Translate } from "../i18n";
import "./SupportQualityView.css";

interface SupportQualityViewProps {
  auth: OperatorAuth;
  inboxes: Inbox[];
  onOpenConversation: (conversationId: string, inboxId: string) => void;
}

const reasonKeys: Record<string, MessageKey> = {
  slow_response: "quality.reason.slowResponse",
  unresolved: "quality.reason.unresolved",
  unclear_answer: "quality.reason.unclearAnswer",
  had_to_repeat: "quality.reason.hadToRepeat",
  fast_response: "quality.reason.fastResponse",
  resolved: "quality.reason.resolved",
  clear_answer: "quality.reason.clearAnswer",
  friendly: "quality.reason.friendly",
};

type HistoryStateFilter = "all" | "reopened" | "closed";

type QualityMicroSignal =
  | { kind: "matrix"; value: number | null; maximum: number }
  | { kind: "series"; values: Array<number | null>; minimum: number; maximum?: number };

interface QualitySummaryMetric {
  id: string;
  label: string;
  value: string;
  note: string;
  signal: QualityMicroSignal;
}

const MICRO_SERIES_SLOTS = 28;
const MICRO_MATRIX_COLUMNS = 18;
const MICRO_MATRIX_ROWS = 3;

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}

function QualityMicroChart({ id, signal }: { id: string; signal: QualityMicroSignal }) {
  if (signal.kind === "matrix") {
    const cellCount = MICRO_MATRIX_COLUMNS * MICRO_MATRIX_ROWS;
    const hasValue = signal.value !== null && Number.isFinite(signal.value) && signal.maximum > 0;
    const activeCount = hasValue
      ? MICRO_MATRIX_ROWS * 3
      : 0;
    const activeCenter = hasValue
      ? Math.round(clamp((signal.value ?? 0) / signal.maximum, 0, 1) * (MICRO_MATRIX_COLUMNS - 1))
      : -1;
    const activeStart = clamp(activeCenter - 1, 0, MICRO_MATRIX_COLUMNS - 3);
    const activeEnd = activeStart + 2;
    const cellSize = 5.4;
    const columnGap = 3.8;
    const rowGap = 4.8;
    const matrixWidth = MICRO_MATRIX_COLUMNS * cellSize + (MICRO_MATRIX_COLUMNS - 1) * columnGap;
    const startX = (180 - matrixWidth) / 2;

    return (
      <svg
        className="quality-micro-chart quality-micro-chart--matrix"
        viewBox="0 0 180 44"
        preserveAspectRatio="xMidYMid meet"
        aria-hidden="true"
        focusable="false"
        data-quality-signal={id}
        data-active-count={activeCount}
      >
        {Array.from({ length: cellCount }, (_, index) => {
          const column = Math.floor(index / MICRO_MATRIX_ROWS);
          const row = index % MICRO_MATRIX_ROWS;
          const isActive = hasValue && column >= activeStart && column <= activeEnd;
          const edgeDistance = Math.min(column, MICRO_MATRIX_COLUMNS - column - 1);
          const opacity = 0.32 + Math.min(1, edgeDistance / 4) * 0.68;

          return (
            <rect
              className={isActive ? "quality-micro-cell quality-micro-cell--active" : "quality-micro-cell"}
              x={startX + column * (cellSize + columnGap)}
              y={6 + row * (cellSize + rowGap)}
              width={cellSize}
              height={cellSize}
              rx="1.2"
              opacity={opacity}
              key={index}
            />
          );
        })}
      </svg>
    );
  }

  const values = signal.values.slice(-MICRO_SERIES_SLOTS);
  const finiteValues = values.filter((value): value is number => value !== null && Number.isFinite(value));
  const maximum = signal.maximum ?? Math.max(signal.minimum, ...finiteValues);
  const range = maximum - signal.minimum;
  const leadingSlots = MICRO_SERIES_SLOTS - values.length;
  const highlightedIndexes = new Set(values
    .map((value, index) => value !== null && Number.isFinite(value) ? index + leadingSlots : -1)
    .filter((index) => index >= 0)
    .slice(-5));
  const gap = 2.6;
  const chartPadding = 2;
  const barWidth = (180 - chartPadding * 2 - gap * (MICRO_SERIES_SLOTS - 1)) / MICRO_SERIES_SLOTS;

  return (
    <svg
      className="quality-micro-chart quality-micro-chart--series"
      viewBox="0 0 180 44"
      preserveAspectRatio="none"
      aria-hidden="true"
      focusable="false"
      data-quality-signal={id}
      data-point-count={finiteValues.length}
      data-active-count={finiteValues.slice(-5).length}
    >
      {Array.from({ length: MICRO_SERIES_SLOTS }, (_, index) => {
        const value = index < leadingSlots ? null : values[index - leadingSlots];
        const hasValue = value !== null && Number.isFinite(value);
        const normalized = !hasValue
          ? 0
          : range <= 0
            ? 0.58
            : clamp((value - signal.minimum) / range, 0, 1);
        const height = hasValue ? 8 + normalized * 28 : 7;
        const isHighlighted = hasValue && highlightedIndexes.has(index);
        const edgeDistance = Math.min(index, MICRO_SERIES_SLOTS - index - 1);
        const opacity = 0.28 + Math.min(1, edgeDistance / 5) * 0.72;

        return (
          <rect
            className={isHighlighted
              ? "quality-micro-bar quality-micro-bar--active"
              : hasValue
                ? "quality-micro-bar quality-micro-bar--data"
                : "quality-micro-bar"}
            x={chartPadding + index * (barWidth + gap)}
            y={(44 - height) / 2}
            width={barWidth}
            height={height}
            rx={barWidth / 2}
            opacity={opacity}
            key={index}
          />
        );
      })}
    </svg>
  );
}

function formatDuration(seconds: number | null, t: Translate): string {
  if (seconds === null || !Number.isFinite(seconds)) return "—";
  if (seconds < 60) return t("quality.duration.seconds", { value: Math.max(1, Math.round(seconds)) });
  if (seconds < 3_600) return t("quality.duration.minutes", { value: Math.round(seconds / 60) });
  if (seconds < 86_400) return t("quality.duration.hours", { value: (seconds / 3_600).toFixed(1) });
  return t("quality.duration.days", { value: (seconds / 86_400).toFixed(1) });
}

function shortConversationId(id: string): string {
  return id.slice(0, 8);
}

function StarScore({ rating, label }: { rating: number; label: string }) {
  return (
    <span className="quality-star-score" aria-label={label}>
      <span aria-hidden="true">
        {[1, 2, 3, 4, 5].map((value) => (
          <Star size={14} strokeWidth={1.7} fill={value <= rating ? "currentColor" : "none"} key={value} />
        ))}
      </span>
      <strong>{rating}</strong>
    </span>
  );
}

function QualityTrend({ report }: { report: SupportQualityReport }) {
  const instanceId = useId();
  const { formatDate, t } = useI18n();
  const ratedPoints = report.trend
    .map((point, index) => ({ ...point, index }))
    .filter((point): point is typeof point & { average_rating: number } => point.average_rating !== null);
  const chartWidth = 640;
  const chartHeight = 164;
  const plotLeft = 34;
  const plotRight = 622;
  const plotTop = 18;
  const plotBottom = 126;
  const xFor = (index: number) => report.trend.length <= 1
    ? (plotLeft + plotRight) / 2
    : plotLeft + index * (plotRight - plotLeft) / (report.trend.length - 1);
  const yFor = (rating: number) => plotBottom - (rating - 1) * (plotBottom - plotTop) / 4;
  const linePoints = ratedPoints
    .map((point) => `${xFor(point.index)},${yFor(point.average_rating)}`)
    .join(" ");

  return (
    <section className="quality-chart-panel" aria-labelledby={`${instanceId}-quality-trend-title`}>
      <header>
        <div>
          <h2 id={`${instanceId}-quality-trend-title`}>{t("quality.trend")}</h2>
          <p>{t("quality.trendDescription")}</p>
        </div>
      </header>
      {ratedPoints.length === 0 ? (
        <div className="quality-chart-empty">{t("quality.noTrend")}</div>
      ) : (
        <figure className="quality-trend-figure">
          <svg viewBox={`0 0 ${chartWidth} ${chartHeight}`} role="img" aria-label={t("quality.trendChartLabel")}>
            {[1, 2, 3, 4, 5].map((rating) => (
              <g key={rating}>
                <line className="quality-chart-grid" x1={plotLeft} x2={plotRight} y1={yFor(rating)} y2={yFor(rating)} />
                <text className="quality-chart-axis" x="8" y={yFor(rating) + 4}>{rating}</text>
              </g>
            ))}
            {ratedPoints.length > 1 && <polyline className="quality-chart-line" pathLength="1" points={linePoints} />}
            {ratedPoints.map((point) => (
              <circle className="quality-chart-point" cx={xFor(point.index)} cy={yFor(point.average_rating)} r="4" key={point.period_started_at}>
                <title>{`${formatDate(point.period_started_at, { month: "short", day: "numeric" })}: ${point.average_rating.toFixed(2)}`}</title>
              </circle>
            ))}
          </svg>
          <figcaption>
            <span>{formatDate(report.trend[0].period_started_at, { month: "short", day: "numeric" })}</span>
            <span>{formatDate(report.trend.at(-1)!.period_started_at, { month: "short", day: "numeric" })}</span>
          </figcaption>
        </figure>
      )}
    </section>
  );
}

function RatingDistribution({ report }: { report: SupportQualityReport }) {
  const instanceId = useId();
  const { t } = useI18n();
  const maximum = Math.max(1, ...report.distribution.map((item) => item.count));

  return (
    <section className="quality-distribution-panel" aria-labelledby={`${instanceId}-quality-distribution-title`}>
      <header>
        <h2 id={`${instanceId}-quality-distribution-title`}>{t("quality.distribution")}</h2>
        <p>{t("quality.distributionDescription")}</p>
      </header>
      <div className="quality-distribution">
        {report.distribution.map((item) => (
          <div key={item.rating}>
            <span><Star size={13} fill="currentColor" />{item.rating}</span>
            <i><b style={{ width: `${item.count / maximum * 100}%` }} /></i>
            <strong>{item.count}</strong>
          </div>
        ))}
      </div>
    </section>
  );
}

function QualityFeedbackCell({ item }: { item: SupportQualityItem }) {
  const { t } = useI18n();
  const comment = item.comment?.trim();

  if (item.rating === null) {
    return <span className="quality-no-rating">{t("quality.notRated")}</span>;
  }

  return (
    <div className="quality-feedback-cell">
      <StarScore rating={item.rating} label={t("quality.ratingOutOfFive", { rating: item.rating })} />
      {comment && <p>{comment}</p>}
      {item.reasons.length > 0 && (
        <div className="quality-reasons">
          {item.reasons.map((reason) => (
            <span key={reason}>{reasonKeys[reason] ? t(reasonKeys[reason]) : reason}</span>
          ))}
        </div>
      )}
    </div>
  );
}

export function SupportQualityView({ auth, inboxes, onOpenConversation }: SupportQualityViewProps) {
  const instanceId = useId();
  const { formatDate, t } = useI18n();
  const [periodDays, setPeriodDays] = useState(30);
  const [inboxId, setInboxId] = useState("");
  const [operatorId, setOperatorId] = useState("");
  const [channelId, setChannelId] = useState("");
  const [rating, setRating] = useState("");
  const [historySearch, setHistorySearch] = useState("");
  const [historyState, setHistoryState] = useState<HistoryStateFilter>("all");
  const [report, setReport] = useState<SupportQualityReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [reloadToken, setReloadToken] = useState(0);

  const updateReportFilter = (update: () => void) => {
    setLoading(true);
    setError(false);
    update();
  };

  useEffect(() => {
    let active = true;
    getSupportQuality(auth, {
      inboxId: inboxId || undefined,
      operatorId: operatorId || undefined,
      channelId: channelId || undefined,
      periodDays,
      rating: rating === "" ? undefined : Number(rating),
      limit: 100,
    })
      .then((nextReport) => {
        if (active) setReport(nextReport);
      })
      .catch(() => {
        if (active) setError(true);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => { active = false; };
  }, [auth, channelId, inboxId, operatorId, periodDays, rating, reloadToken]);

  const summaryMetrics = useMemo<QualitySummaryMetric[]>(() => report ? [
    {
      id: "average-rating",
      label: t("quality.averageRating"),
      value: report.summary.average_rating === null ? "—" : report.summary.average_rating.toFixed(2),
      note: t("quality.ratingsCount", { count: report.summary.rated_resolutions }),
      signal: { kind: "matrix", value: report.summary.average_rating, maximum: 5 },
    },
    {
      id: "csat",
      label: t("quality.csat"),
      value: report.summary.csat_percent === null ? "—" : `${Math.round(report.summary.csat_percent)}%`,
      note: t("quality.csatNote"),
      signal: {
        kind: "series",
        values: report.trend.map((point) => point.csat_percent),
        minimum: 0,
        maximum: 100,
      },
    },
    {
      id: "response-rate",
      label: t("quality.responseRate"),
      value: `${Math.round(report.summary.response_rate_percent)}%`,
      note: t("quality.resolutionsCount", { count: report.summary.total_resolutions }),
      signal: { kind: "matrix", value: report.summary.response_rate_percent, maximum: 100 },
    },
    {
      id: "first-response",
      label: t("quality.firstResponse"),
      value: formatDuration(report.summary.average_first_response_seconds, t),
      note: t("quality.averageForPeriod"),
      signal: {
        kind: "series",
        values: report.trend.map((point) => point.average_first_response_seconds),
        minimum: 0,
      },
    },
    {
      id: "resolution-time",
      label: t("quality.resolutionTime"),
      value: formatDuration(report.summary.average_resolution_seconds, t),
      note: t("quality.averageForPeriod"),
      signal: {
        kind: "series",
        values: report.trend.map((point) => point.average_resolution_seconds),
        minimum: 0,
      },
    },
    {
      id: "reopened",
      label: t("quality.reopened"),
      value: `${Math.round(report.summary.reopened_rate_percent)}%`,
      note: t("quality.reopenedNote"),
      signal: { kind: "matrix", value: report.summary.reopened_rate_percent, maximum: 100 },
    },
  ] : [], [report, t]);

  const normalizedHistorySearch = historySearch.trim().toLocaleLowerCase();
  const filteredItems = useMemo(() => report?.items.filter((item) => {
    if (historyState === "reopened" && item.reopened_at === null) return false;
    if (historyState === "closed" && item.reopened_at !== null) return false;
    if (!normalizedHistorySearch) return true;

    const searchableValues = [
      item.conversation_subject,
      item.contact_name,
      item.conversation_id,
      shortConversationId(item.conversation_id),
      item.responsible_operator_name,
      item.responsible_ai_profile_name,
      item.inbox_name,
      item.channel_name,
      item.comment,
      item.rating === null ? t("quality.notRated") : String(item.rating),
      ...item.reasons.map((reason) => reasonKeys[reason] ? t(reasonKeys[reason]) : reason),
    ];

    return searchableValues.some((value) => value?.toLocaleLowerCase().includes(normalizedHistorySearch));
  }) ?? [], [historyState, normalizedHistorySearch, report, t]);
  const hasLocalHistoryFilters = normalizedHistorySearch !== "" || historyState !== "all";
  const hasHistoryFilters = hasLocalHistoryFilters || rating !== "";

  const clearHistoryFilters = () => {
    setHistorySearch("");
    setHistoryState("all");
    if (rating !== "") updateReportFilter(() => setRating(""));
  };

  return (
    <div className="page quality-page">
      <header className="page-toolbar quality-page-header">
        <div>
          <h1>{t("quality.title")}</h1>
          <p>{t("quality.description")}</p>
        </div>
        <button className="secondary-button" type="button" disabled={loading} onClick={() => updateReportFilter(() => setReloadToken((value) => value + 1))}>
          <RefreshCw className={loading ? "spin" : undefined} size={15} />
          {t("quality.refresh")}
        </button>
      </header>

      <section className="quality-filters" aria-label={t("quality.filters")}>
        <label>{t("quality.period")}
          <select value={periodDays} onChange={(event) => updateReportFilter(() => setPeriodDays(Number(event.target.value)))}>
            <option value={7}>{t("quality.period7")}</option>
            <option value={30}>{t("quality.period30")}</option>
            <option value={90}>{t("quality.period90")}</option>
            <option value={365}>{t("quality.period365")}</option>
          </select>
        </label>
        <label>{t("common.inbox")}
          <select value={inboxId} onChange={(event) => updateReportFilter(() => setInboxId(event.target.value))}>
            <option value="">{t("quality.allInboxes")}</option>
            {inboxes.map((inbox) => <option value={inbox.id} key={inbox.id}>{inbox.name}</option>)}
          </select>
        </label>
        <label>{t("quality.operator")}
          <select value={operatorId} onChange={(event) => updateReportFilter(() => setOperatorId(event.target.value))}>
            <option value="">{t("quality.allOperators")}</option>
            {report?.available_filters.operators.map((option) => <option value={option.id} key={option.id}>{option.name}</option>)}
          </select>
        </label>
        <label>{t("quality.widget")}
          <select value={channelId} onChange={(event) => updateReportFilter(() => setChannelId(event.target.value))}>
            <option value="">{t("quality.allWidgets")}</option>
            {report?.available_filters.channels.map((option) => <option value={option.id} key={option.id}>{option.name}</option>)}
          </select>
        </label>
      </section>

      {error && !report ? (
        <div className="admin-notice admin-notice--error quality-load-error" role="alert">
          <span>{t("quality.loadError")}</span>
          <button className="secondary-button" type="button" onClick={() => updateReportFilter(() => setReloadToken((value) => value + 1))}>{t("quality.retry")}</button>
        </div>
      ) : report ? (
        <>
          <section className={`quality-summary${loading ? " quality-summary--loading" : ""}`} aria-label={t("quality.summary")} aria-busy={loading}>
            {summaryMetrics.map((metric) => (
              <div
                className={`quality-summary-card quality-summary-card--${metric.id}`}
                role="group"
                aria-labelledby={`quality-summary-${metric.id}`}
                key={metric.id}
              >
                <span id={`quality-summary-${metric.id}`}>{metric.label}</span>
                <strong>{metric.value}</strong>
                <small>{metric.note}</small>
                <QualityMicroChart id={metric.id} signal={metric.signal} />
              </div>
            ))}
          </section>

          <div className="quality-insights">
            <QualityTrend report={report} />
            <RatingDistribution report={report} />
          </div>

          <section className="quality-feedback-section" aria-labelledby={`${instanceId}-quality-feedback-title`}>
            <header>
              <div>
                <h2 id={`${instanceId}-quality-feedback-title`}>{t("quality.feedbackTitle")}</h2>
                <p>{t("quality.feedbackDescription")}</p>
              </div>
              <span aria-live="polite">
                {hasLocalHistoryFilters
                  ? t("quality.filteredCount", { shown: filteredItems.length, total: report.items.length })
                  : t("quality.shownCount", { count: report.items.length })}
              </span>
            </header>
            <div className="quality-table-toolbar" aria-label={t("quality.historyFilters")}>
              <label className="quality-history-search">
                <span>{t("quality.historySearch")}</span>
                <span className="quality-history-search-control">
                  <Search aria-hidden="true" size={15} />
                  <input
                    type="search"
                    value={historySearch}
                    autoComplete="off"
                    placeholder={t("quality.historySearchPlaceholder")}
                    onChange={(event) => setHistorySearch(event.target.value)}
                  />
                </span>
              </label>
              <label>
                <span>{t("quality.rating")}</span>
                <select value={rating} onChange={(event) => updateReportFilter(() => setRating(event.target.value))}>
                  <option value="">{t("common.all")}</option>
                  {[5, 4, 3, 2, 1].map((value) => <option value={value} key={value}>{value}</option>)}
                  <option value="0">{t("quality.notRated")}</option>
                </select>
              </label>
              <label>
                <span>{t("quality.cycleStatus")}</span>
                <select value={historyState} onChange={(event) => setHistoryState(event.target.value as HistoryStateFilter)}>
                  <option value="all">{t("quality.allCycleStatuses")}</option>
                  <option value="reopened">{t("quality.reopenedOnly")}</option>
                  <option value="closed">{t("quality.closedOnly")}</option>
                </select>
              </label>
              <button className="secondary-button quality-table-reset" type="button" disabled={!hasHistoryFilters} onClick={clearHistoryFilters}>
                {t("quality.clearHistoryFilters")}
              </button>
            </div>
            <div className="quality-table">
              <table>
                <thead>
                  <tr>
                    <th>{t("quality.feedback")}</th>
                    <th>{t("quality.conversation")}</th>
                    <th>{t("quality.responsible")}</th>
                    <th>{t("quality.source")}</th>
                    <th>{t("quality.timing")}</th>
                    <th>{t("quality.closedAt")}</th>
                    <th><span className="visually-hidden">{t("common.actions")}</span></th>
                  </tr>
                </thead>
                <tbody>
                  {filteredItems.map((item) => (
                    <tr key={item.resolution_id}>
                      <td><QualityFeedbackCell item={item} /></td>
                      <td>
                        <div className="quality-conversation-cell">
                          <strong>{item.conversation_subject || item.contact_name || t("quality.anonymousConversation")}</strong>
                          <span>#{shortConversationId(item.conversation_id)} · {t("quality.cycle", { number: item.cycle_number })}</span>
                          {item.reopened_at && <small>{t("quality.reopenedLabel")}</small>}
                        </div>
                      </td>
                      <td>
                        <div className="quality-responsible-cell">
                          <strong>{item.responsible_operator_name || item.responsible_ai_profile_name || t("quality.unassigned")}</strong>
                        </div>
                      </td>
                      <td>
                        <div className="quality-source-cell">
                          <strong>{item.channel_name}</strong>
                          <span>{item.inbox_name}</span>
                        </div>
                      </td>
                      <td>
                        <dl className="quality-timing-cell">
                          <div><dt>{t("quality.firstResponseShort")}</dt><dd>{formatDuration(item.first_response_seconds, t)}</dd></div>
                          <div><dt>{t("quality.resolutionShort")}</dt><dd>{formatDuration(item.resolution_seconds, t)}</dd></div>
                        </dl>
                      </td>
                      <td className="quality-date-cell">
                        <time dateTime={item.resolved_at}>{formatDate(item.resolved_at, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</time>
                      </td>
                      <td className="table-action-cell">
                        <button className="table-link" type="button" onClick={() => onOpenConversation(item.conversation_id, item.inbox_id)}>
                          <MessageSquare size={14} />{t("quality.openConversation")}
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {filteredItems.length === 0 && <div className="empty-state">{t("quality.empty")}</div>}
            </div>
          </section>
        </>
      ) : (
        <div className="empty-state quality-loading">{t("quality.loading")}</div>
      )}
    </div>
  );
}
