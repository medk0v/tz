import { countryCodeToFlag } from "./visitor-location";
import type { VisitorIntelligence } from "../api";
import { useI18n, type MessageKey } from "../i18n";

interface VisitorIntelligenceDetailsProps {
  intelligence: VisitorIntelligence | null;
  status: "loading" | "ready" | "unavailable";
}

interface DetailRow {
  key: string;
  label: MessageKey;
  prefix?: string;
  value: string | number | null | undefined;
}

const DEVICE_CATEGORY_LABELS: Record<string, MessageKey> = {
  pc: "visitor.deviceCategory.desktop",
  smartphone: "visitor.deviceCategory.mobile",
  mobilephone: "visitor.deviceCategory.mobile",
  appliance: "visitor.deviceCategory.appliance",
  crawler: "visitor.deviceCategory.crawler",
  misc: "visitor.deviceCategory.other",
};

function present(value: string | number | null | undefined): boolean {
  return value !== null && value !== undefined && value !== "";
}


export function VisitorIntelligenceDetails({
  intelligence,
  status,
}: VisitorIntelligenceDetailsProps) {
  const { t, formatDate } = useI18n();
  if (status === "loading") {
    return <p className="visitor-intelligence-state">{t("visitor.loading")}</p>;
  }
  if (status === "unavailable" || !intelligence) {
    return <p className="visitor-intelligence-state">{t("visitor.unavailable")}</p>;
  }

  const latest = intelligence.observations[0] ?? null;
  const context = latest?.client_context ?? null;
  const parsedUserAgent = latest?.user_agent_details ?? null;
  const booleanLabel = (value: boolean | null | undefined) => (
    value == null ? undefined : t(value ? "common.yes" : "common.no")
  );
  const browserHints = context?.browser.brands
    ?.map(({ brand, version }) => `${brand} ${version}`)
    .join(", ");
  const location = latest?.geo_ip
    ? [
        latest.geo_ip.city,
        latest.geo_ip.country
          ? `${latest.geo_ip.country}${latest.geo_ip.country_code ? ` (${latest.geo_ip.country_code})` : ""}`
          : latest.geo_ip.country_code,
      ].filter(Boolean).join(", ")
    : undefined;
  const parsedBrowser = parsedUserAgent
    ? [parsedUserAgent.browser, parsedUserAgent.browser_version].filter(Boolean).join(" ")
    : undefined;
  const operatingSystem = parsedUserAgent
    ? [
        parsedUserAgent.operating_system,
        parsedUserAgent.operating_system_version,
      ].filter(Boolean).join(" ")
    : undefined;
  const deviceCategory = parsedUserAgent?.device_category
    ? t(DEVICE_CATEGORY_LABELS[parsedUserAgent.device_category] ?? "visitor.deviceCategory.other")
    : undefined;
  const screen = context
    ? [
        `${context.display.screen_width} × ${context.display.screen_height}`,
        context.display.color_depth == null
          ? undefined
          : `${context.display.color_depth}-bit`,
        context.display.device_pixel_ratio == null
          ? undefined
          : `${context.display.device_pixel_ratio}×`,
      ].filter(Boolean).join(" · ")
    : undefined;
  const viewport = context
    ? `${context.display.viewport_width} × ${context.display.viewport_height}`
    : undefined;
  const device = context?.device
    ? [
        context.device.memory_gb == null
          ? undefined
          : `${context.device.memory_gb} GB`,
        context.device.logical_processors == null
          ? undefined
          : t("visitor.logicalProcessorsValue", {
              count: context.device.logical_processors,
            }),
        context.device.max_touch_points == null
          ? undefined
          : t("visitor.touchPointsValue", {
              count: context.device.max_touch_points,
            }),
      ].filter(Boolean).join(" · ")
    : undefined;
  const network = context?.connection
    ? [
        context.connection.effective_type,
        context.connection.downlink_mbps == null
          ? undefined
          : `${context.connection.downlink_mbps} Mbps`,
        context.connection.rtt_ms == null
          ? undefined
          : `${context.connection.rtt_ms} ms RTT`,
        context.connection.save_data == null
          ? undefined
          : `${t("visitor.saveData")}: ${booleanLabel(context.connection.save_data)}`,
      ].filter(Boolean).join(" · ")
    : undefined;
  const privacy = context
    ? [
        `${t("visitor.cookies")}: ${booleanLabel(context.browser.cookie_enabled)}`,
        context.browser.do_not_track
          ? `DNT: ${context.browser.do_not_track}`
          : undefined,
        context.browser.global_privacy_control == null
          ? undefined
          : `GPC: ${booleanLabel(context.browser.global_privacy_control)}`,
      ].filter(Boolean).join(" · ")
    : undefined;
  const preferences = context?.preferences
    ? [
        context.preferences.color_scheme,
        context.preferences.reduced_motion == null
          ? undefined
          : `${t("visitor.reducedMotion")}: ${booleanLabel(context.preferences.reduced_motion)}`,
      ].filter(Boolean).join(" · ")
    : undefined;
  const attribution = context?.attribution
    ? Object.entries(context.attribution)
        .filter((entry): entry is [string, string] => (
          typeof entry[1] === "string" && entry[1].length > 0
        ))
        .map(([key, value]) => `${key}=${value}`)
        .join(" · ")
    : undefined;
  const userLanguage = context
    ? [context.locale.language, ...context.locale.languages]
        .filter((value, index, values) => value && values.indexOf(value) === index)
        .join(", ")
    : undefined;
  const ipSource = latest?.client_ip_source === "trusted_proxy"
    ? t("visitor.source.trustedProxy")
    : latest?.client_ip_source === "peer"
      ? t("visitor.source.peer")
      : undefined;

  const rows: DetailRow[] = [
    { key: "ip", label: "visitor.ip", value: latest?.client_ip },
    { key: "ip-source", label: "visitor.ipSource", value: ipSource },
    {
      key: "location",
      label: "visitor.location",
      prefix: countryCodeToFlag(latest?.geo_ip?.country_code),
      value: location,
    },
    { key: "geo-timezone", label: "visitor.geoTimezone", value: latest?.geo_ip?.time_zone },
    { key: "origin", label: "visitor.origin", value: latest?.origin },
    { key: "page-title", label: "visitor.pageTitle", value: context?.page.title },
    { key: "page-url", label: "visitor.pageUrl", value: context?.page.url },
    { key: "referrer", label: "visitor.referrer", value: context?.page.referrer },
    {
      key: "first-seen",
      label: "visitor.firstSeen",
      value: formatDate(intelligence.first_seen_at, {
        year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
      }),
    },
    {
      key: "last-seen",
      label: "visitor.lastSeen",
      value: formatDate(intelligence.last_seen_at, {
        year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
      }),
    },
    { key: "sessions", label: "visitor.sessions", value: intelligence.session_count },
    { key: "browser", label: "visitor.browser", value: parsedBrowser },
    { key: "operating-system", label: "visitor.operatingSystem", value: operatingSystem },
    { key: "device-category", label: "visitor.deviceCategory", value: deviceCategory },
    { key: "user-agent", label: "visitor.userAgent", value: latest?.user_agent },
    { key: "brands", label: "visitor.browserHints", value: browserHints },
    { key: "platform", label: "visitor.platform", value: context?.browser.platform },
    { key: "device", label: "visitor.device", value: device },
    { key: "screen", label: "visitor.screen", value: screen },
    { key: "viewport", label: "visitor.viewport", value: viewport },
    { key: "user-language", label: "visitor.userLanguage", value: userLanguage || latest?.accept_language },
    { key: "widget-language", label: "visitor.widgetLanguage", value: latest?.widget_language },
    { key: "timezone", label: "visitor.timezone", value: context?.locale.timezone },
    { key: "network", label: "visitor.network", value: network },
    { key: "preferences", label: "visitor.preferences", value: preferences },
    { key: "privacy", label: "visitor.privacy", value: privacy },
    { key: "attribution", label: "visitor.attribution", value: attribution },
  ];

  return (
    <dl className="visitor-intelligence-list">
      {rows.map((row) => (
        <div key={row.key}>
          <dt>{t(row.label)}</dt>
          <dd>
            {row.prefix ? (
              <>
                <span aria-hidden="true">{row.prefix}</span>{" "}
              </>
            ) : null}
            {present(row.value) ? row.value : "—"}
          </dd>
        </div>
      ))}
    </dl>
  );
}
