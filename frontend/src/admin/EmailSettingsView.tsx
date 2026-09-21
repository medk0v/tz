import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useId, useState, type FormEvent } from "react";
import { ArrowLeft, Mail, Save } from "lucide-react";
import {
  getEmailSettings,
  updateEmailSettings,
  type EmailSettings,
  type EmailSettingsInput,
  type OperatorAuth,
} from "../api";
import { useI18n } from "../i18n";

interface EmailSettingsViewProps {
  auth: OperatorAuth;
  canManage: boolean;
  onBack: () => void;
}

interface EmailSettingsDraft {
  enabled: boolean;
  smtpHost: string;
  smtpPort: string;
  smtpSecurity: "tls" | "starttls";
  smtpUsername: string;
  smtpPassword: string;
  clearSmtpPassword: boolean;
  fromName: string;
  fromEmail: string;
  replyToEmail: string;
  ratingPageUrl: string;
}

function toDraft(settings: EmailSettings): EmailSettingsDraft {
  return {
    enabled: settings.enabled,
    smtpHost: settings.smtp_host,
    smtpPort: String(settings.smtp_port),
    smtpSecurity: settings.smtp_security,
    smtpUsername: settings.smtp_username ?? "",
    smtpPassword: "",
    clearSmtpPassword: false,
    fromName: settings.from_name,
    fromEmail: settings.from_email,
    replyToEmail: settings.reply_to_email ?? "",
    ratingPageUrl: settings.rating_page_url || `${window.location.origin}/rate-chat`,
  };
}

export function EmailSettingsView({ auth, canManage, onBack }: EmailSettingsViewProps) {
  const { t } = useI18n();
  const fieldId = useId();
  const [settings, setSettings] = useState<EmailSettings | null>(null);
  const [draft, setDraft] = useState<EmailSettingsDraft | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    let active = true;
    getEmailSettings(auth)
      .then((value) => {
        if (!active) return;
        setSettings(value);
        setDraft(toDraft(value));
      })
      .catch(() => { if (active) setError(true); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [auth]);

  function updateDraft(patch: Partial<EmailSettingsDraft>) {
    setDraft((current) => current ? { ...current, ...patch } : current);
    setSaved(false);
    setError(false);
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!draft || !canManage || saving) return;
    setSaved(false);
    const smtpPort = Number(draft.smtpPort);
    if (!Number.isInteger(smtpPort) || smtpPort < 1 || smtpPort > 65_535) {
      setError(true);
      return;
    }
    const input: EmailSettingsInput = {
      enabled: draft.enabled,
      smtp_host: draft.smtpHost.trim(),
      smtp_port: smtpPort,
      smtp_security: draft.smtpSecurity,
      smtp_username: draft.smtpUsername.trim() || null,
      smtp_password: draft.smtpPassword || null,
      clear_smtp_password: draft.clearSmtpPassword || !draft.smtpUsername.trim(),
      from_name: draft.fromName.trim(),
      from_email: draft.fromEmail.trim(),
      reply_to_email: draft.replyToEmail.trim() || null,
      rating_page_url: draft.ratingPageUrl.trim(),
    };
    setSaving(true);
    setError(false);
    try {
      const updated = await updateEmailSettings(auth, input);
      setSettings(updated);
      setDraft(toDraft(updated));
      setSaved(true);
    } catch {
      setError(true);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="page channels-page channels-page--settings">
      <button className="back-button" type="button" onClick={onBack}><ArrowLeft size={16} aria-hidden="true" />{t("channels.backToChannels")}</button>
      <header className="page-toolbar">
        <div><h1>{t("emailSettings.title")}</h1><p>{t("emailSettings.description")}</p></div>
      </header>
      {error && <div className="admin-notice admin-notice--error" role="alert">{t("emailSettings.error")}</div>}
      {saved && <div className="admin-notice admin-notice--success" role="status">{t("emailSettings.saved")}</div>}
      {loading || !draft ? (
        <div className="empty-state">{t("emailSettings.loading")}</div>
      ) : (
        <section className="settings-panel email-settings-panel">
          <form onSubmit={(event) => void handleSubmit(event)}>
            <header><h2><Mail size={17} />{t("emailSettings.smtpTitle")}</h2><span>{settings?.enabled ? t("emailSettings.enabled") : t("emailSettings.disabled")}</span></header>
            <div className="settings-form email-settings-form">
              <label className="widget-notification-setting">
                <input type="checkbox" checked={draft.enabled} disabled={!canManage} onChange={(event) => updateDraft({ enabled: event.currentTarget.checked })} />
                <span><strong>{t("emailSettings.sendInvitations")}</strong><span>{t("emailSettings.sendInvitationsHelp")}</span></span>
              </label>
              <div className="email-settings-columns">
                <fieldset className="email-settings-section">
                  <legend>{t("emailSettings.connectionTitle")}</legend>
                  <div className="email-settings-grid">
                    <label className="email-settings-wide"><span>{t("emailSettings.smtpHost")}</span><input required maxLength={253} readOnly={!canManage} autoCapitalize="none" spellCheck={false} value={draft.smtpHost} placeholder="smtp.example.com" onChange={(event) => updateDraft({ smtpHost: event.currentTarget.value })} /></label>
                    <label><span>{t("emailSettings.smtpPort")}</span><input required type="number" min={1} max={65_535} readOnly={!canManage} value={draft.smtpPort} onChange={(event) => updateDraft({ smtpPort: event.currentTarget.value })} /></label>
                    <label><span>{t("emailSettings.security")}</span><select disabled={!canManage} value={draft.smtpSecurity} onChange={(event) => updateDraft({ smtpSecurity: event.currentTarget.value as "tls" | "starttls" })}><option value="starttls">STARTTLS</option><option value="tls">TLS</option></select></label>
                    <label><span>{t("emailSettings.username")}</span><input maxLength={320} readOnly={!canManage} autoCapitalize="none" autoComplete="off" value={draft.smtpUsername} onChange={(event) => updateDraft({ smtpUsername: event.currentTarget.value, clearSmtpPassword: event.currentTarget.value.trim() ? draft.clearSmtpPassword : true })} /></label>
                    <label><span>{t("emailSettings.password")}</span><input type="password" maxLength={1_000} readOnly={!canManage} autoComplete="new-password" aria-describedby={`${fieldId}-password-help`} value={draft.smtpPassword} placeholder={settings?.password_configured ? t("emailSettings.passwordStored") : ""} onChange={(event) => updateDraft({ smtpPassword: event.currentTarget.value, clearSmtpPassword: false })} /></label>
                    <p className="email-settings-help email-settings-wide" id={`${fieldId}-password-help`}>{t("emailSettings.passwordHelp")}</p>
                  </div>
                </fieldset>
                <fieldset className="email-settings-section">
                  <legend>{t("emailSettings.senderTitle")}</legend>
                  <div className="email-settings-grid">
                    <label><span>{t("emailSettings.fromName")}</span><input required maxLength={120} readOnly={!canManage} value={draft.fromName} onChange={(event) => updateDraft({ fromName: event.currentTarget.value })} /></label>
                    <label><span>{t("emailSettings.fromEmail")}</span><input required type="email" maxLength={320} readOnly={!canManage} value={draft.fromEmail} onChange={(event) => updateDraft({ fromEmail: event.currentTarget.value })} /></label>
                    <label className="email-settings-wide"><span>{t("emailSettings.replyTo")}</span><input type="email" maxLength={320} readOnly={!canManage} value={draft.replyToEmail} onChange={(event) => updateDraft({ replyToEmail: event.currentTarget.value })} /></label>
                    <label className="email-settings-wide"><span>{t("emailSettings.ratingPageUrl")}</span><input required type="url" maxLength={2_000} readOnly={!canManage} aria-describedby={`${fieldId}-rating-help`} value={draft.ratingPageUrl} placeholder={`${window.location.origin}/rate-chat`} onChange={(event) => updateDraft({ ratingPageUrl: event.currentTarget.value })} /></label>
                    <div className="email-settings-help email-settings-wide" id={`${fieldId}-rating-help`}><p>{t("emailSettings.ratingPageUrlHelp")}</p><p>{t("emailSettings.senderHelp")}</p></div>
                  </div>
                </fieldset>
              </div>
            </div>
            {canManage && <footer className="email-settings-actions"><DemoActionButton className="primary-button" type="submit" disabled={saving}><Save size={16} />{saving ? t("emailSettings.saving") : t("emailSettings.save")}</DemoActionButton></footer>}
          </form>
        </section>
      )}
    </div>
  );
}
