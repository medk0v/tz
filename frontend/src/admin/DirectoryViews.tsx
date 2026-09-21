import { useEffect, useId, useMemo, useState } from "react";
import { useContentMotion } from "./useContentMotion";
import { ChevronLeft, ChevronRight, Search } from "lucide-react";
import {
  listContacts,
  type Contact,
  type ContactPeriod,
  type ContactStatistics,
  type OperatorAuth,
} from "../api";
import { channelKindLabel, useI18n, type MessageKey } from "../i18n";
import { ContactBlockButton } from "./ContactBlockButton";
import { ContactInfo } from "./ContactInfo";
import { DateTimeField } from "./DateTimeField";
import { useCanonicalPageRoute, usePageRoute } from "./page-route";
import { countryCodeToFlag } from "./visitor-location";
import "./DirectoryWorkbench.css";

interface AuthProps {
  auth: OperatorAuth;
  canManage?: boolean;
}

const CONTACTS_PER_PAGE = 10;

type ContactsTab = "contacts" | "statistics";
/** Contacts URLs: `/contacts` and `/contacts/statistics`. */
function contactsSegments(tab: ContactsTab): string[] {
  return tab === "statistics" ? [tab] : [];
}

function visiblePageNumbers(page: number, totalPages: number): Array<number | string> {
  const pages = new Set([1, totalPages, page - 1, page, page + 1]);
  const visible = [...pages]
    .filter((value) => value >= 1 && value <= totalPages)
    .sort((left, right) => left - right);
  const result: Array<number | string> = [];
  visible.forEach((value, index) => {
    const previous = visible[index - 1];
    if (previous !== undefined && value - previous > 1) {
      result.push(`ellipsis-${previous}`);
    }
    result.push(value);
  });
  return result;
}

export function ContactsView({ auth, canManage = false }: AuthProps) {
  const instanceId = useId();
  const { t, formatDate, locale } = useI18n();
  const route = usePageRoute();
  const navigateRoute = route.navigate;
  const tab: ContactsTab = route.segments[0] === "statistics" ? "statistics" : "contacts";
  const [period, setPeriod] = useState<ContactPeriod>("day");
  const [customFrom, setCustomFrom] = useState("");
  const [customTo, setCustomTo] = useState("");
  const [statistics, setStatistics] = useState<ContactStatistics | null>(null);
  const [contacts, setContacts] = useState<Contact[]>([]);
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [page, setPage] = useState(1);
  const [total, setTotal] = useState(0);
  const [totalPages, setTotalPages] = useState(0);
  const [loading, setLoading] = useState(true);
  const [now, setNow] = useState(() => Date.now());
  const [error, setError] = useState<MessageKey | null>(null);
  const [resultSelection, setResultSelection] = useState("loading");
  const pageNumbers = useMemo(
    () => visiblePageNumbers(page, totalPages),
    [page, totalPages],
  );

  const from = customFrom ? new Date(`${customFrom}T00:00:00`) : null;
  const to = customTo ? new Date(`${customTo}T00:00:00`) : null;
  if (to) to.setDate(to.getDate() + 1);
  const customValid = !!from && !!to && Number.isFinite(from.getTime()) && Number.isFinite(to.getTime()) && from < to;
  const rangeFrom = period === "custom" && customValid ? from?.toISOString() : undefined;
  const rangeTo = period === "custom" && customValid ? to?.toISOString() : undefined;
  const canLoad = period !== "custom" || customValid;
  const contentMotion = useContentMotion<HTMLElement>(`${tab}:${resultSelection}`);
  useCanonicalPageRoute(route, contactsSegments(tab));

  useEffect(() => {
    const nextSearch = search.trim();
    if (nextSearch === debouncedSearch) return;
    const timeout = window.setTimeout(() => {
      setLoading(true);
      setDebouncedSearch(nextSearch);
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [debouncedSearch, search]);

  useEffect(() => {
    if (!canLoad) return;
    let active = true;
    let requestId = 0;
    const load = () => {
      const currentRequest = ++requestId;
      return listContacts(auth, {
        page,
        per_page: CONTACTS_PER_PAGE,
        search: debouncedSearch || undefined,
        period, from: rangeFrom, to: rangeTo,
      })
        .then((result) => {
          if (!active || currentRequest !== requestId) return;
          if (result.total_pages > 0 && page > result.total_pages) {
            setPage(result.total_pages);
            return;
          }
          setStatistics(result.statistics);
          setContacts(result.items);
          setResultSelection(`${period}:${page}:${rangeFrom ?? ""}:${rangeTo ?? ""}`);
          setTotal(result.total);
          setTotalPages(result.total_pages);
          setError(null);
        })
        .catch(() => {
          if (active && currentRequest === requestId) setError("contacts.loadError");
        })
        .finally(() => {
          if (active && currentRequest === requestId) setLoading(false);
        });
    };
    void load();
    const interval = window.setInterval(() => { setNow(Date.now()); void load(); }, 30_000);
    return () => { active = false; window.clearInterval(interval); };
  }, [auth, canLoad, debouncedSearch, page, period, rangeFrom, rangeTo]);

  const firstResult = total === 0 ? 0 : (page - 1) * CONTACTS_PER_PAGE + 1;
  const lastResult = Math.min(page * CONTACTS_PER_PAGE, total);

  return (
    <div className="page directory-page contacts-page">
      <header className="page-toolbar directory-page-toolbar">
        <h1>{t("contacts.title")}</h1>
        <div className="contacts-filters">
          <label className="contacts-period-field">
            <span className="visually-hidden">{t("contacts.period")}</span>
            <select aria-label={t("contacts.period")} value={period} onChange={(event) => {
              setPeriod(event.target.value as ContactPeriod); setPage(1); setLoading(true);
            }}>
              {(["day", "week", "month", "custom", "all"] as const).map((value) => <option key={value} value={value}>{t(`contacts.period.${value}`)}</option>)}
            </select>
          </label>
          {period === "custom" && <div className="contacts-date-range">
            <div className="contacts-date-field"><span id={`${instanceId}-contacts-from-label`}>{t("contacts.from")}</span><DateTimeField mode="date" locale={locale} aria-labelledby={`${instanceId}-contacts-from-label`} value={customFrom} max={customTo || undefined} onChange={(value) => { setCustomFrom(value); setPage(1); setLoading(true); }} /></div>
            <div className="contacts-date-field"><span id={`${instanceId}-contacts-to-label`}>{t("contacts.to")}</span><DateTimeField mode="date" locale={locale} aria-labelledby={`${instanceId}-contacts-to-label`} value={customTo} min={customFrom || undefined} onChange={(value) => { setCustomTo(value); setPage(1); setLoading(true); }} /></div>
          </div>}
          <label className="search-field">
            <Search size={16} />
            <span className="visually-hidden">{t("contacts.search")}</span>
            <input
              value={search}
              placeholder={t("contacts.search")}
              onChange={(event) => {
                setSearch(event.target.value);
                if (page !== 1) setLoading(true);
                setPage(1);
              }}
            />
          </label>
        </div>
      </header>
      <div className="contacts-tabs" role="tablist" aria-label={t("contacts.title")}>
        {(["contacts", "statistics"] as const).map((value) => <button type="button" role="tab" id={`${instanceId}-contacts-tab-${value}`} aria-controls={`${instanceId}-contacts-panel-${value}`} aria-selected={tab === value} tabIndex={tab === value ? 0 : -1} key={value} onClick={() => void navigateRoute(contactsSegments(value))} onKeyDown={(event) => {
          if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
            event.preventDefault();
            const next = event.key === "Home" ? "contacts" : event.key === "End" ? "statistics" : tab === "contacts" ? "statistics" : "contacts";
            void navigateRoute(contactsSegments(next)); document.getElementById(`${instanceId}-contacts-tab-${next}`)?.focus();
          }
        }}>{t(value === "contacts" ? "contacts.title" : "contacts.statistics")}</button>)}
      </div>
      {!canLoad && <p className="contacts-range-notice" role="status">{t("contacts.chooseRange")}</p>}
      {error && <div className="admin-notice admin-notice--error" role="alert">{t(error)}</div>}
      {canLoad && tab === "statistics" && <section ref={contentMotion} id={`${instanceId}-contacts-panel-statistics`} role="tabpanel" aria-labelledby={`${instanceId}-contacts-tab-statistics`} className="contacts-statistics" aria-busy={loading}>
        {statistics && <>
          <div className="table-panel"><table>
            <thead><tr><th>{t("contacts.metric")}</th><th>{t("contacts.count")}</th></tr></thead>
            <tbody>{(["contacts", "new_contacts", "returning_contacts", "conversations", "widget_sessions", "widget_contacts", "telegram_contacts"] as const).map((key) => <tr key={key}><th scope="row">{t(`contacts.metric.${key}`)}</th><td>{statistics[key]}</td></tr>)}</tbody>
          </table></div>
          <div className="table-panel contacts-country-statistics"><table>
            <thead><tr><th>{t("contacts.country")}</th><th>{t("contacts.metric.contacts")}</th></tr></thead>
            <tbody>{statistics.countries.map((country) => <tr key={country.country_code ?? "unknown"}><th scope="row"><span aria-hidden="true">{countryCodeToFlag(country.country_code)}</span> {country.country || country.country_code || t("contacts.unknownCountry")}</th><td>{country.contacts}</td></tr>)}</tbody>
          </table></div>
          <p className="contacts-statistics-note">{t("contacts.statisticsNote")}</p>
        </>}
      </section>}
      {canLoad && tab === "contacts" && <div ref={contentMotion} id={`${instanceId}-contacts-panel-contacts`} role="tabpanel" aria-labelledby={`${instanceId}-contacts-tab-contacts`} className="table-panel directory-page-table contacts-table" aria-busy={loading}>
        <div className="contacts-table-scroll">
          <table>
            <colgroup>
              <col style={{ width: "18%" }} /><col style={{ width: "11%" }} />
              <col style={{ width: "14%" }} /><col style={{ width: "17%" }} />
              <col style={{ width: "13%" }} /><col style={{ width: "11%" }} />
              <col style={{ width: "12%" }} /><col style={{ width: "4%" }} />
            </colgroup>
            <thead><tr><th>{t("contacts.name")}</th><th>{t("contacts.channel")}</th><th>{t("contacts.ip")}</th><th>{t("contacts.country")}</th><th>{t("contacts.browser")}</th><th>{t("contacts.localTime")}</th><th>{t("contacts.lastActivity")}</th><th><span className="visually-hidden">{t("contacts.info")}</span></th></tr></thead>
            <tbody>
              {contacts.map((contact) => {
                const parsed = contact.user_agent_details;
                const browser = parsed
                  ? [parsed.browser, parsed.browser_version].filter(Boolean).join(" ")
                  : "";
                const operatingSystem = parsed
                  ? [parsed.operating_system, parsed.operating_system_version].filter(Boolean).join(" ")
                  : "";
                const country = contact.geo_ip?.country || contact.geo_ip?.country_code;
                const flag = countryCodeToFlag(contact.geo_ip?.country_code);
                const channelLabels = contact.channel_kinds.map((kind) => kind === "telegram_bot" ? t("channels.type.telegram") : channelKindLabel(t, kind));
                const channels = channelLabels.join(", ");
                const timezone = contact.browser_timezone;
                let localTime = "—";
                if (timezone) {
                  try { localTime = formatDate(new Date(now).toISOString(), { timeZone: timezone, hour: "2-digit", minute: "2-digit" }); }
                  catch { /* Older clients may have supplied an unsupported timezone. */ }
                }
                const activity = contact.last_activity_at;
                const info = [
                  `${t("contacts.name")}: ${contact.display_name || t("contacts.anonymous")}`,
                  contact.email,
                  `${t("contacts.contactId")}: ${contact.id}`,
                  `${t("contacts.projectId")}: ${contact.project_id}`,
                  `${t("contacts.channel")}: ${channels || "—"}`,
                  `${t("contacts.browser")}: ${browser || "—"}`,
                  `${t("contacts.operatingSystem")}: ${operatingSystem || "—"}`,
                  `${t("visitor.timezone")}: ${timezone || "—"}`,
                  `${t("visitor.geoTimezone")}: ${contact.geo_ip?.time_zone || "—"}`,
                ].filter(Boolean).join("\n");
                return (
                  <tr key={contact.id}>
                    <td>
                      <div className="contact-identity">
                        <strong title={contact.display_name || t("contacts.anonymous")}>{contact.display_name || t("contacts.anonymous")}</strong>
                        {contact.email && <span title={contact.email}>{contact.email}</span>}
                        {contact.is_blocked && <span className="contact-blocked-badge">{t("contacts.blocked")}</span>}
                        {canManage && <ContactBlockButton auth={auth} contactId={contact.id} blocked={contact.is_blocked} compact onUpdated={(blocked) => setContacts((items) => items.map((item) => item.id === contact.id ? { ...item, is_blocked: blocked } : item))} />}
                      </div>
                    </td>
                    <td title={channels}><div className="contact-identity contact-channels">{channelLabels.length ? channelLabels.map((label) => <span key={label}>{label}</span>) : "—"}</div></td>
                    <td className="mono-cell" title={contact.client_ip ?? undefined}>{contact.client_ip ?? "—"}</td>
                    <td>
                      <div className="contact-identity">
                        <strong title={country || undefined}>{flag && <span className="contact-country-flag" aria-hidden="true">{flag} </span>}{country || "—"}</strong>
                        {contact.geo_ip?.city && <span title={contact.geo_ip.city}>{contact.geo_ip.city}</span>}
                      </div>
                    </td>
                    <td>
                      <div className="contact-identity">
                        <strong title={browser}>{browser || "—"}</strong>
                        <span title={operatingSystem}>{operatingSystem || "—"}</span>
                      </div>
                    </td>
                    <td title={timezone || undefined}>{localTime}</td>
                    <td>
                      <div className="contact-identity">
                        <strong>{formatDate(activity, { hour: "2-digit", minute: "2-digit" })}</strong>
                        <span>{formatDate(activity, { year: "numeric", month: "short", day: "numeric" })}</span>
                      </div>
                    </td>
                    <td className="contact-info-cell">
                      <ContactInfo label={t("contacts.info")} text={info} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {!loading && contacts.length === 0 && <div className="empty-state">{t("contacts.empty")}</div>}
        </div>
        {totalPages > 0 && (
          <nav className="table-pagination" aria-label={t("contacts.pagination")}>
            <span className="table-pagination-count">
              {t("contacts.results", { from: firstResult, to: lastResult, total })}
            </span>
            <div className="table-pagination-controls">
              <button
                type="button"
                className="pagination-arrow"
                aria-label={t("contacts.previousPage")}
                disabled={page === 1 || loading}
                onClick={() => {
                  setLoading(true);
                  setPage((current) => Math.max(1, current - 1));
                }}
              >
                <ChevronLeft size={16} />
              </button>
              {pageNumbers.map((item) => typeof item === "number" ? (
                <button
                  type="button"
                  className="pagination-page"
                  key={item}
                  aria-label={t("contacts.page", { page: item })}
                  aria-current={item === page ? "page" : undefined}
                  disabled={loading}
                  onClick={() => {
                    if (item === page) return;
                    setLoading(true);
                    setPage(item);
                  }}
                >
                  {item}
                </button>
              ) : (
                <span className="pagination-ellipsis" key={item} aria-hidden="true">…</span>
              ))}
              <button
                type="button"
                className="pagination-arrow"
                aria-label={t("contacts.nextPage")}
                disabled={page === totalPages || loading}
                onClick={() => {
                  setLoading(true);
                  setPage((current) => Math.min(totalPages, current + 1));
                }}
              >
                <ChevronRight size={16} />
              </button>
            </div>
          </nav>
        )}
      </div>}
    </div>
  );
}
