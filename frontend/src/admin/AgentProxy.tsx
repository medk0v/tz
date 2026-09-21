import { useEffect, useState } from "react";
import { type OperatorAuth } from "../api";
import { getAgentProxy, saveAgentProxy, type AgentProxySettings } from "../agent-proxy-api";
import { useI18n } from "../i18n";
import { DemoActionButton } from "./DemoReadOnly";
import "./AgentProxy.css";

const labels = {
  ru: {
    title: "Прокси", enabled: "Подключить прокси", help: "Браузер агента получает один прокси на открытие страницы. При ошибке прямое подключение не используется.",
    url: "URL получения прокси", urlHelp: "Полный HTTPS-адрес метода, например https://proxy.example/api/proxies/random. Без параметров региона и страны.",
    token: "Bearer-ключ", configured: "Ключ сохранён. Оставьте пустым, чтобы сохранить его.", remove: "Удалить сохранённый ключ", selection: "Выбор прокси",
    any: "Любой случайный", region: "По региону", country: "По стране", regionValue: "Регион", countryValue: "Код страны",
    save: "Сохранить прокси", saving: "Сохранение…", saved: "Настройки прокси сохранены", loading: "Загрузка настроек прокси…", loadError: "Не удалось загрузить настройки прокси", saveError: "Не удалось сохранить настройки прокси",
    retry: "Повторить", createFirst: "Сначала сохраните агента, затем подключите прокси.", changedUrl: "Для другого URL введите ключ заново или удалите сохранённый.", required: "Укажите URL, ключ и выбранный регион или двухбуквенный код страны.",
  },
  en: {
    title: "Proxy", enabled: "Connect a proxy", help: "The agent browser obtains one proxy per page load. A proxy failure does not fall back to a direct connection.",
    url: "Proxy discovery URL", urlHelp: "Full HTTPS endpoint, for example https://proxy.example/api/proxies/random. Omit region and country parameters.",
    token: "Bearer token", configured: "Token saved. Leave blank to keep it.", remove: "Remove saved token", selection: "Proxy selection",
    any: "Any random proxy", region: "By region", country: "By country", regionValue: "Region", countryValue: "Country code",
    save: "Save proxy", saving: "Saving…", saved: "Proxy settings saved", loading: "Loading proxy settings…", loadError: "Could not load proxy settings", saveError: "Could not save proxy settings",
    retry: "Retry", createFirst: "Save the agent before connecting a proxy.", changedUrl: "Enter a new token or remove the saved token when changing the URL.", required: "Enter the URL, token, and selected region or two-letter country code.",
  },
  ro: {
    title: "Proxy", enabled: "Conectează un proxy", help: "Browserul agentului obține un proxy pentru fiecare pagină. La eroare nu se utilizează o conexiune directă.",
    url: "URL pentru obținerea proxy-ului", urlHelp: "Adresa HTTPS completă, de exemplu https://proxy.example/api/proxies/random. Fără parametrii de regiune și țară.",
    token: "Cheie Bearer", configured: "Cheia este salvată. Lasă gol pentru a o păstra.", remove: "Șterge cheia salvată", selection: "Selectarea proxy-ului",
    any: "Orice proxy aleatoriu", region: "După regiune", country: "După țară", regionValue: "Regiune", countryValue: "Codul țării",
    save: "Salvează proxy-ul", saving: "Se salvează…", saved: "Setările proxy-ului au fost salvate", loading: "Se încarcă setările proxy-ului…", loadError: "Setările proxy-ului nu au putut fi încărcate", saveError: "Setările proxy-ului nu au putut fi salvate",
    retry: "Reîncearcă", createFirst: "Salvează agentul înainte de a conecta un proxy.", changedUrl: "Introdu o cheie nouă sau șterge cheia salvată când schimbi URL-ul.", required: "Introdu URL-ul, cheia și regiunea selectată sau codul țării din două litere.",
  },
};

interface Props { auth: OperatorAuth; profileId?: string; disabled?: boolean; }

export function AgentProxy({ auth, profileId, disabled }: Props) {
  const { locale } = useI18n();
  const text = labels[locale];
  const [saved, setSaved] = useState<AgentProxySettings | null>(null);
  const [draft, setDraft] = useState<AgentProxySettings | null>(null);
  const [mode, setMode] = useState<"any" | "region" | "country">("any");
  const [token, setToken] = useState("");
  const [clearToken, setClearToken] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [retry, setRetry] = useState(0);
  const [error, setError] = useState<"loadError" | "saveError" | "changedUrl" | "required" | null>(null);
  const [notice, setNotice] = useState(false);

  useEffect(() => {
    if (!profileId) return;
    const controller = new AbortController();
    void getAgentProxy(auth, profileId, controller.signal).then(value => {
      if (controller.signal.aborted) return;
      setSaved(value); setDraft(value);
      setMode(value.country ? "country" : value.region ? "region" : "any");
      setToken(""); setClearToken(false);
    }).catch(() => { if (!controller.signal.aborted) setError("loadError"); })
      .finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => controller.abort();
  }, [auth, profileId, retry]);

  async function save() {
    if (!draft || !saved || !profileId || saving) return;
    setNotice(false); setError(null);
    if (saved.token_configured && saved.service_url !== draft.service_url.trim() && !token.trim() && !clearToken) { setError("changedUrl"); return; }
    const region = mode === "region" ? (draft.region ?? "").trim().toLowerCase() : null;
    const country = mode === "country" ? (draft.country ?? "").trim().toUpperCase() : null;
    if ((draft.enabled && (!draft.service_url.trim() || (!token.trim() && (!saved.token_configured || clearToken))))
        || (mode === "region" && !/^[a-z][a-z0-9_-]{0,63}$/.test(region ?? ""))
        || (mode === "country" && !/^[A-Z]{2}$/.test(country ?? ""))) { setError("required"); return; }
    setSaving(true);
    try {
      const value = await saveAgentProxy(auth, profileId, {
        enabled: draft.enabled, service_url: draft.service_url.trim(), region, country,
        ...(token.trim() ? { token: token.trim() } : {}), clear_token: clearToken,
      });
      setSaved(value); setDraft(value); setToken(""); setClearToken(false); setNotice(true);
    } catch { setError("saveError"); } finally { setSaving(false); }
  }

  return <fieldset className="ai-credentials agent-proxy-settings" disabled={disabled || saving}>
    <legend>{text.title}</legend>
    {!profileId ? <p>{text.createFirst}</p> : loading ? <p role="status">{text.loading}</p> : <>
      {error && <p className="form-error" role="alert">{text[error]}</p>}
      {error === "loadError" ? <button type="button" className="secondary-button" onClick={() => { setLoading(true); setError(null); setRetry(value => value + 1); }}>{text.retry}</button> : draft && <>
        <label><input type="checkbox" checked={draft.enabled} onChange={event => { setDraft({ ...draft, enabled: event.target.checked }); setNotice(false); if (event.target.checked) setClearToken(false); }} /><span>{text.enabled}</span></label>
        <p>{text.help}</p>
        <label><span>{text.url}</span><input type="url" value={draft.service_url} maxLength={2048} placeholder="https://proxy.example/api/proxies/random" onChange={event => { setDraft({ ...draft, service_url: event.target.value }); setNotice(false); }} /><small>{text.urlHelp}</small></label>
        <label><span>{text.token}</span><input type="password" autoComplete="new-password" value={token} maxLength={4096} onChange={event => { setToken(event.target.value); setClearToken(false); setNotice(false); }} />{saved?.token_configured && <small>{text.configured}</small>}</label>
        {saved?.token_configured && <label><input type="checkbox" checked={clearToken} onChange={event => { setClearToken(event.target.checked); setNotice(false); if (event.target.checked) { setToken(""); setDraft({ ...draft, enabled: false }); } }} /><span>{text.remove}</span></label>}
        <label><span>{text.selection}</span><select value={mode} onChange={event => { setMode(event.target.value as typeof mode); setNotice(false); }}><option value="any">{text.any}</option><option value="region">{text.region}</option><option value="country">{text.country}</option></select></label>
        {mode === "region" && <label><span>{text.regionValue}</span><input value={draft.region ?? ""} maxLength={64} placeholder="europe" onChange={event => { setDraft({ ...draft, region: event.target.value }); setNotice(false); }} /></label>}
        {mode === "country" && <label><span>{text.countryValue}</span><input value={draft.country ?? ""} maxLength={2} placeholder="DE" onChange={event => { setDraft({ ...draft, country: event.target.value }); setNotice(false); }} /></label>}
        <div className="ai-actions"><DemoActionButton type="button" className="secondary-button" disabled={saving} onClick={() => void save()}>{saving ? text.saving : text.save}</DemoActionButton></div>
        {notice && <p role="status">{text.saved}</p>}
      </>}
    </>}
  </fieldset>;
}
