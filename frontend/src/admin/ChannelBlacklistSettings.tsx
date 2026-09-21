import { DemoActionButton } from "./DemoReadOnly";
import { useState, type FormEvent } from "react";
import { Plus, Save, Trash2 } from "lucide-react";
import { updateChannelBlacklist, type ChannelConnection, type OperatorAuth } from "../api";
import { useI18n } from "../i18n";

export function ChannelBlacklistSettings({ auth, channel, canManage, onUpdated, onBack }: {
  auth: OperatorAuth;
  channel: ChannelConnection;
  canManage: boolean;
  onUpdated: (channel: ChannelConnection) => void;
  onBack: () => void;
}) {
  const { t } = useI18n();
  const [defaultLanguage, setDefaultLanguage] = useState(channel.blacklist_reply.default_language);
  const [translations, setTranslations] = useState(() => Object.entries(channel.blacklist_reply.translations)
    .map(([code, message]) => ({ id: code, code, message })));
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<"invalid" | "saved" | "error" | null>(null);

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canManage || saving) return;
    const entries = translations.map(({ code, message }) => [code.trim().toLowerCase(), message.trim()] as const);
    if (entries.length === 0 || new Set(entries.map(([code]) => code)).size !== entries.length
      || entries.some(([code, message]) => !/^[a-z]{2,8}(?:-[a-z0-9]{1,8})*$/.test(code) || code.length > 35 || !message || Array.from(message).length > 4000)
      || !entries.some(([code]) => code === defaultLanguage)) {
      setNotice("invalid");
      return;
    }
    setSaving(true);
    setNotice(null);
    try {
      const updated = await updateChannelBlacklist(auth, channel.id, { default_language: defaultLanguage, translations: Object.fromEntries(entries) });
      onUpdated(updated);
      setNotice("saved");
    } catch {
      setNotice("error");
    } finally {
      setSaving(false);
    }
  }

  return <div className="page channels-page channels-page--settings">
    <button className="back-button" type="button" onClick={onBack}>{t("channels.backToChannels")}</button>
    <header className="page-toolbar"><div><h1>{t("channels.blacklistSettings")} · {channel.name}</h1><p>{t("channels.blacklistHelp")}</p></div></header>
    {notice && <div className={`admin-notice admin-notice--${notice === "saved" ? "success" : "error"}`} role={notice === "saved" ? "status" : "alert"}>
      {t(notice === "invalid" ? "channels.blacklistInvalid" : notice === "saved" ? "channels.blacklistSaved" : "channels.blacklistSaveError")}
    </div>}
    <section className="settings-panel blacklist-settings-panel">
      <form className="settings-form" onSubmit={(event) => void save(event)}>
        <fieldset disabled={!canManage || saving}>
          <label><span>{t("channels.blacklistDefault")}</span><select value={defaultLanguage} onChange={(event) => { setDefaultLanguage(event.target.value); setNotice(null); }}>
            {translations.filter(({ code }) => code.trim()).map(({ id, code }) => <option key={id} value={code}>{code}</option>)}
          </select></label>
          <p className="field-help">{t("channels.blacklistFallback")}</p>
          {translations.map((translation) => <div className="blacklist-translation" key={translation.id}>
            <div className="blacklist-translation-language">
              <label><span>{t("channels.blacklistLanguage")}</span><input aria-label={`${t("channels.blacklistLanguage")} ${translation.code}`} value={translation.code} maxLength={35} required autoCapitalize="none" spellCheck={false} onChange={(event) => {
                const code = event.target.value.toLowerCase().replaceAll("_", "-");
                setTranslations((items) => items.map((item) => item.id === translation.id ? { ...item, code } : item));
                if (defaultLanguage === translation.code) setDefaultLanguage(code);
                setNotice(null);
              }} /></label>
              <button className="icon-button" type="button" aria-label={t("channels.blacklistRemoveLanguage", { language: translation.code })} disabled={translations.length <= 1 || defaultLanguage === translation.code} onClick={() => { setTranslations((items) => items.filter((item) => item.id !== translation.id)); setNotice(null); }}><Trash2 size={16} aria-hidden="true" /></button>
            </div>
            <label><span>{t("channels.blacklistMessage")} ({translation.code || "…"})</span><textarea rows={3} required maxLength={4000} value={translation.message} onChange={(event) => { setTranslations((items) => items.map((item) => item.id === translation.id ? { ...item, message: event.target.value } : item)); setNotice(null); }} /></label>
          </div>)}
          {canManage && <div className="blacklist-settings-actions">
            <button className="secondary-button" type="button" disabled={translations.length >= 32} onClick={() => { setTranslations((items) => [...items, { id: crypto.randomUUID(), code: "", message: "" }]); setNotice(null); }}><Plus size={16} aria-hidden="true" />{t("channels.blacklistAddLanguage")}</button>
            <DemoActionButton className="primary-button" type="submit"><Save size={16} aria-hidden="true" />{t(saving ? "channels.blacklistSaving" : "channels.blacklistSave")}</DemoActionButton>
          </div>}
        </fieldset>
      </form>
    </section>
  </div>;
}
