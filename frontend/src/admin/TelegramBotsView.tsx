import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useState, type FormEvent } from "react";
import { Plus, Send, Trash2 } from "lucide-react";
import {
  ApiRequestError,
  createTelegramBotChannel,
  deleteChannel,
  listChannels,
  type ChannelConnection,
  type Inbox,
  type OperatorAuth,
} from "../api";
import { channelStatusLabel, useI18n } from "../i18n";

interface TelegramBotsViewProps {
  auth: OperatorAuth;
  inboxes: Inbox[];
  bots: ChannelConnection[];
  canManage: boolean;
  onBack: () => void;
  onCreated: (channel: ChannelConnection) => void;
  onDeleted: (channelId: string) => void;
  onReloaded: (channels: ChannelConnection[]) => void;
  onBlacklist?: (channelId: string) => void;
}

export function TelegramBotsView({
  auth,
  inboxes,
  bots,
  canManage,
  onBack,
  onCreated,
  onDeleted,
  onReloaded,
  onBlacklist,
}: TelegramBotsViewProps) {
  const { t, formatDate } = useI18n();
  const [name, setName] = useState("");
  const [inboxId, setInboxId] = useState(inboxes[0]?.id ?? "");
  const [botToken, setBotToken] = useState("");
  const [saving, setSaving] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ kind: "success" | "error"; text: string } | null>(null);
  const hasConnectingBots = bots.some((bot) => bot.status === "connecting");

  useEffect(() => {
    if (!hasConnectingBots) return;
    let active = true;
    const refresh = () => {
      void listChannels(auth)
        .then((channels) => {
          if (active) onReloaded(channels);
        })
        .catch(() => undefined);
    };
    refresh();
    const timer = window.setInterval(refresh, 2_500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [auth, hasConnectingBots, onReloaded]);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canManage || saving || !name.trim() || !inboxId || !botToken.trim()) return;
    setSaving(true);
    setNotice(null);
    try {
      const created = await createTelegramBotChannel(auth, {
        inbox_id: inboxId,
        name: name.trim(),
        bot_token: botToken.trim(),
      });
      onCreated(created);
      setName("");
      setBotToken("");
      setNotice({ kind: "success", text: t("telegramBots.created") });
    } catch (cause) {
      const text = cause instanceof ApiRequestError && cause.status === 503
        ? t("telegramBots.serverNotConfigured")
        : cause instanceof ApiRequestError && cause.status === 401
          ? t("telegramBots.passwordSessionRequired")
          : t("telegramBots.createError");
      setNotice({ kind: "error", text });
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(bot: ChannelConnection) {
    if (!window.confirm(t("telegramBots.deleteConfirm", { name: bot.name }))) return;
    setDeletingId(bot.id);
    setNotice(null);
    try {
      await deleteChannel(auth, bot.id);
      onDeleted(bot.id);
      setNotice({ kind: "success", text: t("telegramBots.deleted") });
    } catch (cause) {
      setNotice({
        kind: "error",
        text: cause instanceof ApiRequestError && cause.status === 401
          ? t("telegramBots.passwordSessionRequired")
          : t("telegramBots.deleteError"),
      });
    } finally {
      setDeletingId(null);
    }
  }

  return (
    <div className="page channels-page channels-page--settings">
      <button className="back-button" type="button" onClick={onBack}>{t("channels.backToChannels")}</button>
      <header className="page-toolbar">
        <div><h1>{t("telegramBots.title")}</h1><p>{t("telegramBots.description")}</p></div>
      </header>
      {notice && (
        <div className={`admin-notice admin-notice--${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>
          {notice.text}
        </div>
      )}
      {canManage && (
        <section className="settings-panel email-settings-panel">
          <form onSubmit={(event) => void handleSubmit(event)}>
            <header><h2><Send size={17} />{t("telegramBots.connectTitle")}</h2></header>
            <div className="settings-form email-settings-form">
              <div className="email-settings-grid">
                <label><span>{t("telegramBots.connectionName")}</span><input required maxLength={200} value={name} onChange={(event) => setName(event.currentTarget.value)} /></label>
                <label><span>{t("channels.inbox")}</span><select required value={inboxId} onChange={(event) => setInboxId(event.currentTarget.value)}>{inboxes.map((inbox) => <option key={inbox.id} value={inbox.id}>{inbox.name}</option>)}</select></label>
                <label className="email-settings-wide"><span>{t("telegramBots.token")}</span><input required type="password" maxLength={256} autoCapitalize="none" autoComplete="new-password" value={botToken} onChange={(event) => setBotToken(event.currentTarget.value)} /><small>{t("telegramBots.tokenHelp")}</small></label>
              </div>
              <p className="field-help">{t("telegramBots.setupHelp")}</p>
              <DemoActionButton className="primary-button" type="submit" disabled={saving || !inboxId}><Plus size={16} />{t(saving ? "telegramBots.connecting" : "telegramBots.connect")}</DemoActionButton>
            </div>
          </form>
        </section>
      )}
      <div className="table-panel widgets-table">
        <table>
          <thead><tr><th>{t("telegramBots.connectionName")}</th><th>{t("telegramBots.bot")}</th><th>{t("channels.inbox")}</th><th>{t("channels.catalogStatus")}</th><th>{t("channels.updated")}</th><th><span className="visually-hidden">{t("common.actions")}</span></th></tr></thead>
          <tbody>
            {bots.map((bot) => (
              <tr key={bot.id}>
                <td>{bot.name}</td>
                <td>{bot.telegram_bot_username ? `@${bot.telegram_bot_username}` : t("telegramBots.awaitingIdentity")}</td>
                <td>{inboxes.find((inbox) => inbox.id === bot.inbox_id)?.name ?? bot.inbox_id}</td>
                <td><span className={`status-text status-text--${bot.status}`}>{channelStatusLabel(t, bot.status)}</span></td>
                <td>{formatDate(bot.updated_at, { year: "numeric", month: "short", day: "numeric" })}</td>
                <td className="table-action-cell">
                  {onBlacklist && <button className="table-link" type="button" onClick={() => onBlacklist(bot.id)}>{t("channels.blacklist")}</button>}
                  {canManage && <DemoActionButton className="table-link table-danger-link" type="button" disabled={deletingId !== null} onClick={() => void handleDelete(bot)}><Trash2 size={15} />{t(deletingId === bot.id ? "channels.deleting" : "channels.delete")}</DemoActionButton>}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {bots.length === 0 && <div className="empty-state">{t("telegramBots.empty")}</div>}
      </div>
    </div>
  );
}
