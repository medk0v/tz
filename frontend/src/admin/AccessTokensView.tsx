import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useState } from "react";
import { Ban, Copy, KeyRound, Plus, Trash2, X } from "lucide-react";
import {
  createAccessToken,
  deleteAccessToken,
  listAccessTokenManagement,
  listProjects,
  listRoles,
  revokeAccessToken,
  type AccessToken,
  type OperatorAuth,
  type Project,
  type RoleDefinition,
} from "../api";
import { roleLabel, useI18n, type MessageKey } from "../i18n";
import { tokenProjectText } from "./token-project-i18n";

interface AccessTokensViewProps {
  auth: OperatorAuth;
  projectId: string;
}

const expiryOptions = [1, 7, 30, 90, 180, 365];

function tokenStatus(token: AccessToken): "active" | "expired" | "revoked" {
  if (token.revoked_at) return "revoked";
  return new Date(token.expires_at).getTime() <= Date.now() ? "expired" : "active";
}

const tokenStatusKeys: Record<ReturnType<typeof tokenStatus>, MessageKey> = {
  active: "tokens.status.active",
  expired: "tokens.status.expired",
  revoked: "tokens.status.revoked",
};

export function AccessTokensView({ auth, projectId }: AccessTokensViewProps) {
  const { t, locale, formatDate } = useI18n();
  const p = tokenProjectText(locale);
  const projectSelection = projectId;
  const rolesProjectId = projectSelection === "all" ? projectId : projectSelection;
  const [roleState, setRoleState] = useState<{ projectId: string; items: RoleDefinition[]; failed: boolean } | null>(null);
  const rolesLoading = roleState?.projectId !== rolesProjectId;
  const roles = rolesLoading ? [] : roleState?.items ?? [];
  const [projects, setProjects] = useState<Project[]>([]);
  const availableProjects = projects.filter((project) => project.role === "admin" && project.status === "active");
  const [canCreateAllProjects, setCanCreateAllProjects] = useState(false);
  const [tokens, setTokens] = useState<AccessToken[]>([]);
  const [name, setName] = useState("");
  const [roleId, setRoleId] = useState("");
  const [expiresInDays, setExpiresInDays] = useState(30);
  const [createdSecret, setCreatedSecret] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [loadRevision, setLoadRevision] = useState(0);
  const [saving, setSaving] = useState(false);
  const [revokingId, setRevokingId] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [error, setError] = useState<MessageKey | null>(null);
  const selectedRole = roles.find((item) => item.id === roleId) ?? null;
  const scopeAvailable = projectSelection === "all"
    ? canCreateAllProjects && availableProjects.some((project) => project.id === rolesProjectId)
    : availableProjects.some((project) => project.id === projectSelection);

  useEffect(() => {
    let active = true;
    Promise.all([listProjects(auth), listAccessTokenManagement(auth)])
      .then(([availableProjects, management]) => {
        if (!active) return;
        setProjects(availableProjects);
        setTokens(management.items);
        setCanCreateAllProjects(management.can_create_all_projects);
        setLoadFailed(false);
      })
      .catch(() => {
        if (active) setLoadFailed(true);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [auth, loadRevision]);

  useEffect(() => {
    let active = true;
    listRoles(auth, rolesProjectId)
      .then((availableRoles) => {
        if (!active) return;
        const tokenRoles = availableRoles.filter((item) => item.base_role !== "admin");
        setRoleState({ projectId: rolesProjectId, items: tokenRoles, failed: false });
        setRoleId((current) => tokenRoles.some((item) => item.id === current)
          ? current
          : tokenRoles.find((item) => item.base_role === "operator")?.id ?? tokenRoles[0]?.id ?? "");
      })
      .catch(() => {
        if (active) setRoleState({ projectId: rolesProjectId, items: [], failed: true });
      });
    return () => { active = false; };
  }, [auth, rolesProjectId, loadRevision]);

  async function handleCreate(event: React.FormEvent) {
    event.preventDefault();
    if (auth.isDemo || !name.trim() || !selectedRole || !scopeAvailable || loading || loadFailed || rolesLoading || saving || revokingId || deletingId) return;
    setSaving(true);
    setError(null);
    setCreatedSecret(null);
    try {
      const created = await createAccessToken(auth, {
        name: name.trim(),
        role_id: roleId,
        expires_in_days: expiresInDays,
        project_scope: projectSelection === "all" ? "all" : "project",
        project_id: rolesProjectId,
      });
      const { secret, ...token } = created;
      setCreatedSecret(secret);
      setName("");
      setTokens((current) => [token, ...current]);
    } catch {
      setError("tokens.createError");
    } finally {
      setSaving(false);
    }
  }

  async function handleRevoke(tokenId: string) {
    if (auth.isDemo || saving || revokingId || deletingId) return;
    setRevokingId(tokenId);
    setError(null);
    try {
      await revokeAccessToken(auth, tokenId);
      setTokens((current) => current.map((item) => item.id === tokenId ? { ...item, revoked_at: new Date().toISOString() } : item));
    } catch {
      setError("tokens.revokeError");
    } finally {
      setRevokingId(null);
    }
  }

  async function handleDelete(token: AccessToken) {
    if (auth.isDemo || saving || revokingId || deletingId || !window.confirm(t("tokens.deleteConfirm", { name: token.name }))) return;
    setDeletingId(token.id);
    setError(null);
    try {
      await deleteAccessToken(auth, token.id);
      setTokens((current) => current.filter((item) => item.id !== token.id));
    } catch {
      setError("tokens.deleteError");
    } finally {
      setDeletingId(null);
    }
  }

  return (
    <div className="page access-tokens-page">
      <header className="page-toolbar">
        <div>
          <h1>{t("tokens.title")}</h1>
          <p>{t("tokens.description")}</p>
        </div>
      </header>

      {error && <div className="admin-notice admin-notice--error" role="alert">{t(error)}</div>}
      {(loadFailed || (!rolesLoading && roleState?.failed)) && <div className="admin-notice admin-notice--error" role="alert">
        {t("tokens.loadError")} <button className="secondary-button" type="button" disabled={loading} onClick={() => { setLoading(true); setLoadRevision((current) => current + 1); }}>{p("retry")}</button>
      </div>}
      {createdSecret && (
        <section className="token-secret" aria-live="polite">
          <div>
            <KeyRound size={19} />
            <span><strong>{t("tokens.copyNow")}</strong> {t("tokens.copyWarning")}</span>
          </div>
          <div className="token-secret-value">
            <code>{createdSecret}</code>
            <button type="button" aria-label={t("tokens.copy")} onClick={() => void navigator.clipboard.writeText(createdSecret)}><Copy size={16} /></button>
            <button type="button" aria-label={t("tokens.dismiss")} onClick={() => setCreatedSecret(null)}><X size={16} /></button>
          </div>
        </section>
      )}

      <div className="access-token-layout">
        <form className="access-token-form" onSubmit={handleCreate} aria-busy={saving}>
          <header><Plus size={17} /><strong>{t("tokens.new")}</strong></header>
          <div className="access-token-fields">
            <label>
              {t("tokens.name")}
              <input value={name} maxLength={100} required disabled={saving || auth.isDemo} placeholder={t("tokens.namePlaceholder")} onChange={(event) => setName(event.target.value)} />
            </label>
            
            <label>
              {t("tokens.role")}
              <select value={selectedRole?.id ?? ""} required disabled={rolesLoading || saving || auth.isDemo} onChange={(event) => setRoleId(event.target.value)}>
                <option value="" disabled>{p("chooseRole")}</option>
                {roles.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}
              </select>
            </label>
            <label>
              {t("tokens.expiresAfter")}
              <select value={expiresInDays} disabled={saving || auth.isDemo} onChange={(event) => setExpiresInDays(Number(event.target.value))}>
                {expiryOptions.map((days) => <option value={days} key={days}>{t(days === 1 ? "tokens.day" : "tokens.days", { count: days })}</option>)}
              </select>
            </label>
            <p className="department-help">{p("roleHelp")}{projectSelection === "all" && <> {p("roleSource", { project: projects.find((project) => project.id === rolesProjectId)?.name ?? p("unavailableProject") })}</>}</p>
            {!rolesLoading && !roleState?.failed && roles.length === 0 && <p className="department-help">{p("noRoles")}</p>}
            {!loading && !loadFailed && availableProjects.length === 0 && <p className="department-help">{p("noProjects")}</p>}
            <DemoActionButton className="primary-button" type="submit" disabled={saving || loading || loadFailed || rolesLoading || auth.isDemo || Boolean(revokingId) || Boolean(deletingId) || !name.trim() || !selectedRole || !scopeAvailable}>
              <KeyRound size={16} />{saving ? t("tokens.creating") : t("tokens.create")}
            </DemoActionButton>
          </div>
        </form>

        <section className="table-panel access-token-table">
          <div className="table-panel-header"><strong>{t("tokens.issued")}</strong><span>{tokens.length}</span></div>
          <table>
            <thead><tr><th>{t("tokens.name")}</th><th>{t("tokens.role")}</th><th>{t("tokens.scope")}</th><th>{t("tokens.expires")}</th><th>{t("tokens.status")}</th><th><span className="visually-hidden">{t("common.actions")}</span></th></tr></thead>
            <tbody>
              {tokens.map((token) => {
                const status = tokenStatus(token);
                return (
                  <tr key={token.id}>
                    <td><strong>{token.name}</strong><small>{t("tokens.created", { date: formatDate(token.created_at, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) })}</small></td>
                    <td className="role-cell">{token.role_name ?? roleLabel(t, token.role)}</td>
                    <td>
                      {t("login.operatorWorkspace")}
                      {token.inbox_scope?.length ? <><br /><small>{p("legacyInboxRestriction", { count: token.inbox_scope.length })}</small></> : null}
                    </td>
                    <td>{token.expires_at ? formatDate(token.expires_at, { year: "numeric", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : t("common.never")}</td>
                    <td><span className={`token-status token-status--${status}`}>{t(tokenStatusKeys[status])}</span></td>
                    <td>
                      <div className="token-row-actions">
                        {status === "active" && (
                          <DemoActionButton
                            className="secondary-icon-button"
                            type="button"
                            aria-label={t("tokens.revoke", { name: token.name })}
                            title={t("tokens.revoke", { name: token.name })}
                            disabled={auth.isDemo || saving || revokingId !== null || deletingId !== null}
                            onClick={() => void handleRevoke(token.id)}
                          >
                            <Ban size={16} />
                          </DemoActionButton>
                        )}
                        <DemoActionButton
                          className="danger-icon-button"
                          type="button"
                          aria-label={t("tokens.delete", { name: token.name })}
                          title={t("tokens.delete", { name: token.name })}
                          disabled={auth.isDemo || saving || deletingId !== null || revokingId !== null}
                          onClick={() => void handleDelete(token)}
                        >
                          <Trash2 size={16} />
                        </DemoActionButton>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {!loading && !loadFailed && tokens.length === 0 && <div className="empty-state">{t("tokens.empty")}</div>}
          {loading && <div className="empty-state" role="status">{t("tokens.loading")}</div>}
        </section>
      </div>
    </div>
  );
}
