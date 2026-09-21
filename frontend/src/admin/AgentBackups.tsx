import { AnimatedDisclosure } from "../AnimatedDisclosure";
import { DemoActionButton } from "./DemoReadOnly";
import { useState } from "react";
import { Archive, RotateCcw, Trash2 } from "lucide-react";
import { createAiProfileBackup, deleteAiProfileBackup, listAiProfileBackups, restoreAiProfileBackup, type AiProfile, type AiProfileBackup, type OperatorAuth } from "../api";
import { useI18n, type MessageKey } from "../i18n";
import { adminPagePath } from "../entry-mode";
import "./AgentBackups.css";

interface Props {
  auth: OperatorAuth;
  profile: AiProfile;
  dirty: boolean;
  busy: boolean;
  onBusyChange: (busy: boolean) => void;
  onRestored: (profile: AiProfile) => void;
}

export function AgentBackups({ auth, profile, dirty, busy, onBusyChange, onRestored }: Props) {
  const { t, formatDate } = useI18n();
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState<AiProfileBackup[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [createBeforeRestore, setCreateBeforeRestore] = useState(true);
  const [restoredBaseIds, setRestoredBaseIds] = useState<string[]>([]);
  const [notice, setNotice] = useState<{ error: boolean; key: MessageKey } | null>(null);

  async function refresh() {
    onBusyChange(true);
    setNotice(null);
    try {
      setItems(await listAiProfileBackups(auth, profile.id));
      setLoaded(true);
    } catch {
      setNotice({ error: true, key: "ai.backups.loadError" });
    } finally {
      onBusyChange(false);
    }
  }

  async function create() {
    if (busy || dirty) return;
    onBusyChange(true);
    setNotice(null);
    try {
      await createAiProfileBackup(auth, profile.id);
      setNotice({ error: false, key: "ai.backups.created" });
    } catch {
      setNotice({ error: true, key: "ai.backups.createError" });
      onBusyChange(false);
      return;
    }
    try {
      setItems(await listAiProfileBackups(auth, profile.id));
      setLoaded(true);
    } catch {
      setNotice({ error: true, key: "ai.backups.createdRefreshError" });
    } finally {
      onBusyChange(false);
    }
  }

  async function restore(backup: AiProfileBackup) {
    if (busy || !window.confirm(t("ai.backups.confirm", {
      date: date(backup.created_at),
      backup: t(createBeforeRestore ? "ai.backups.confirmWithBackup" : "ai.backups.confirmWithoutBackup"),
    }))) return;
    onBusyChange(true);
    setNotice(null);
    setRestoredBaseIds([]);
    try {
      const restored = await restoreAiProfileBackup(auth, profile.id, backup.id, { create_backup: createBeforeRestore });
      onRestored(restored);
      setRestoredBaseIds(restored.knowledge_base_ids);
      setNotice({ error: false, key: createBeforeRestore ? "ai.backups.restoredWithBackup" : "ai.backups.restored" });
    } catch {
      setNotice({ error: true, key: "ai.backups.restoreError" });
      onBusyChange(false);
      return;
    }
    try {
      setItems(await listAiProfileBackups(auth, profile.id));
      setLoaded(true);
    } catch {
      setNotice({ error: true, key: "ai.backups.restoredRefreshError" });
    } finally {
      onBusyChange(false);
    }
  }

  async function remove(backup: AiProfileBackup) {
    if (busy || !window.confirm(t("ai.backups.deleteConfirm", { date: date(backup.created_at) }))) return;
    onBusyChange(true);
    setNotice(null);
    try {
      await deleteAiProfileBackup(auth, profile.id, backup.id);
      setItems((current) => current.filter((item) => item.id !== backup.id));
      setNotice({ error: false, key: "ai.backups.deleted" });
    } catch {
      setNotice({ error: true, key: "ai.backups.deleteError" });
      onBusyChange(false);
      return;
    }
    try {
      setItems(await listAiProfileBackups(auth, profile.id));
      setLoaded(true);
    } catch {
      setNotice({ error: true, key: "ai.backups.deletedRefreshError" });
    } finally {
      onBusyChange(false);
    }
  }

  function date(value: string) {
    return formatDate(value, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  return <section className="agent-backups" aria-label={t("ai.backups.title")}>
    <button type="button" className="secondary-button" disabled={busy} aria-expanded={open} onClick={() => {
      setOpen(!open);
      if (!open) void refresh();
    }}><Archive size={16} />{t("ai.backups.title")}</button>
    <AnimatedDisclosure open={open} className="agent-backups-content">
      <p>{t("ai.backups.help")}</p>
      <div className="ai-actions">
        <DemoActionButton type="button" className="secondary-button" disabled={busy || dirty} onClick={() => void create()}>{t("ai.backups.create")}</DemoActionButton>
        <button type="button" className="secondary-button" disabled={busy} onClick={() => void refresh()}>{t("ai.backups.refresh")}</button>
      </div>
      {dirty && <p>{t("ai.backups.saveFirst")}</p>}
      <label className="agent-backups-option"><input type="checkbox" checked={createBeforeRestore} disabled={busy} onChange={(event) => setCreateBeforeRestore(event.target.checked)} /><span>{t("ai.backups.createBeforeRestore")}</span></label>
      {notice && <p role={notice.error ? "alert" : "status"}>{t(notice.key)}</p>}
      {restoredBaseIds.length > 0 && <div className="agent-backups-restored-links">{restoredBaseIds.map((id, index) => <a key={id} href={adminPagePath("knowledge_base", auth.projectId, [id])} target="_blank" rel="noreferrer">{restoredBaseIds.length === 1 ? t("ai.backups.openRestoredBase") : t("ai.backups.openRestoredBaseNumber", { number: index + 1 })}</a>)}</div>}
      {busy && <p role="status">{t("ai.backups.working")}</p>}
      {loaded && items.length === 0 && <p>{t("ai.backups.empty")}</p>}
      <ul className="agent-backups-list">{items.map((backup) => <li key={backup.id}>
        <div className="agent-backups-details"><time dateTime={backup.created_at}>{date(backup.created_at)}</time>
          <span>{t(backup.reason === "before_restore" ? "ai.backups.automatic" : "ai.backups.manual")}</span>
          <span>{t("ai.backups.counts", { bases: backup.knowledge_base_count, articles: backup.article_count })}</span>
        </div>
        <div className="agent-backups-actions">
          <DemoActionButton type="button" className="secondary-button" disabled={busy} onClick={() => void restore(backup)} aria-label={t("ai.backups.restoreDate", { date: date(backup.created_at) })}><RotateCcw size={16} aria-hidden="true" />{t("ai.backups.restore")}</DemoActionButton>
          <DemoActionButton type="button" className="secondary-button table-danger-link" disabled={busy} onClick={() => void remove(backup)} aria-label={t("ai.backups.deleteDate", { date: date(backup.created_at) })}><Trash2 size={16} aria-hidden="true" />{t("ai.backups.delete")}</DemoActionButton>
        </div>
      </li>)}</ul>
    </AnimatedDisclosure>
  </section>;
}
