import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useState } from "react";
import { LockKeyhole, Plus, Save, ShieldCheck, Trash2 } from "lucide-react";
import {
  createRole,
  deleteRole,
  listRoles,
  updateRole,
  type OperatorAuth,
  type RoleDefinition,
  type RolePermission,
} from "../api";
import { useI18n, type MessageKey } from "../i18n";

interface RolesViewProps {
  auth: OperatorAuth;
}

interface RoleDraft {
  name: string;
  base_role: "manager" | "operator";
  permissions: RolePermission[];
}

interface PermissionDefinition {
  permission: RolePermission;
  label: MessageKey;
  description: MessageKey;
  editable: boolean;
}

const permissionDefinitions: PermissionDefinition[] = [
  { permission: "projects:read", label: "roles.permission.projectsRead", description: "roles.permission.projectsReadDescription", editable: true },
  { permission: "projects:manage", label: "roles.permission.projectsManage", description: "roles.permission.projectsManageDescription", editable: false },
  { permission: "conversations:read", label: "roles.permission.conversationsRead", description: "roles.permission.conversationsReadDescription", editable: true },
  { permission: "conversations:reply", label: "roles.permission.conversationsReply", description: "roles.permission.conversationsReplyDescription", editable: true },
  { permission: "conversations:close", label: "roles.permission.conversationsClose", description: "roles.permission.conversationsCloseDescription", editable: true },
  { permission: "contacts:read", label: "roles.permission.contactsRead", description: "roles.permission.contactsReadDescription", editable: true },
  { permission: "contacts:manage", label: "roles.permission.contactsManage", description: "roles.permission.contactsManageDescription", editable: true },
  { permission: "channels:read", label: "roles.permission.channelsRead", description: "roles.permission.channelsReadDescription", editable: true },
  { permission: "channels:manage", label: "roles.permission.channelsManage", description: "roles.permission.channelsManageDescription", editable: true },
  { permission: "access_tokens:manage", label: "roles.permission.tokensManage", description: "roles.permission.tokensManageDescription", editable: false },
  { permission: "visitor_network:read", label: "roles.permission.visitorNetworkRead", description: "roles.permission.visitorNetworkReadDescription", editable: true },
  { permission: "quality:read", label: "roles.permission.qualityRead", description: "roles.permission.qualityReadDescription", editable: true },
  { permission: "quality:read_all", label: "roles.permission.qualityReadAll", description: "roles.permission.qualityReadAllDescription", editable: true },
  { permission: "routing:manage", label: "roles.permission.routingManage", description: "roles.permission.routingManageDescription", editable: true },
  { permission: "ai:manage", label: "roles.permission.aiManage", description: "roles.permission.aiManageDescription", editable: true },
  { permission: "notes:read", label: "roles.permission.notesRead", description: "roles.permission.notesReadDescription", editable: true },
  { permission: "notes:write", label: "roles.permission.notesWrite", description: "roles.permission.notesWriteDescription", editable: true },
  { permission: "knowledge:manage", label: "roles.permission.knowledgeManage", description: "roles.permission.knowledgeManageDescription", editable: true },
  { permission: "reply_templates:manage", label: "roles.permission.replyTemplatesManage", description: "roles.permission.replyTemplatesManageDescription", editable: true },
  { permission: "processes:read", label: "roles.permission.processesRead", description: "roles.permission.processesReadDescription", editable: true },
  { permission: "processes:edit", label: "roles.permission.processesEdit", description: "roles.permission.processesEditDescription", editable: true },
  { permission: "processes:approve", label: "roles.permission.processesApprove", description: "roles.permission.processesApproveDescription", editable: true },
  { permission: "tasks:own", label: "roles.permission.tasksOwn", description: "roles.permission.tasksOwnDescription", editable: true },
  { permission: "tasks:manage", label: "roles.permission.tasksManage", description: "roles.permission.tasksManageDescription", editable: true },
  { permission: "tasks:configure", label: "roles.permission.tasksConfigure", description: "roles.permission.tasksConfigureDescription", editable: true },
  { permission: "integrations:manage", label: "roles.permission.integrationsManage", description: "roles.permission.integrationsManageDescription", editable: true },
  { permission: "roles:manage", label: "roles.permission.rolesManage", description: "roles.permission.rolesManageDescription", editable: false },
];
const defaultPermissions: RolePermission[] = [
  "projects:read",
  "conversations:read",
  "conversations:reply",
  "conversations:close",
  "contacts:read",
  "contacts:manage",
  "channels:read",
  "quality:read",
  "tasks:own",
];
const accessTokenDeniedPermissions = new Set<RolePermission>([
  "projects:manage",
  "access_tokens:manage",
  "roles:manage",
]);
const visiblePermissions = new Set(permissionDefinitions.map(({ permission }) => permission));

function normalizePermissions(permissions: RolePermission[]): RolePermission[] {
  const orderedVisible = permissionDefinitions
    .map(({ permission }) => permission)
    .filter((permission) => permissions.includes(permission));
  const hiddenLegacy = permissions.filter((permission) => !visiblePermissions.has(permission));
  return [...orderedVisible, ...hiddenLegacy];
}

function togglePermission(permissions: RolePermission[], permission: RolePermission): RolePermission[] {
  const next = new Set(permissions);
  if (next.has(permission)) {
    next.delete(permission);
    if (permission === "conversations:read") {
      next.delete("conversations:reply");
      next.delete("conversations:close");
    }
    if (permission === "notes:read") next.delete("notes:write");
    if (permission === "contacts:read") next.delete("contacts:manage");
    if (permission === "channels:read") next.delete("channels:manage");
    if (permission === "quality:read") next.delete("quality:read_all");
    if (permission === "processes:read") {
      next.delete("processes:edit");
      next.delete("processes:approve");
    }
    if (permission === "tasks:own") next.delete("tasks:manage");
    if (permission === "tasks:own" || permission === "tasks:manage") next.delete("tasks:configure");
  } else {
    next.add(permission);
    if (permission === "conversations:reply" || permission === "conversations:close") {
      next.add("conversations:read");
    }
    if (permission === "notes:write") next.add("notes:read");
    if (permission === "contacts:manage") next.add("contacts:read");
    if (permission === "channels:manage") next.add("channels:read");
    if (permission === "quality:read_all") next.add("quality:read");
    if (["processes:edit", "processes:approve"].includes(permission)) next.add("processes:read");
    if (permission === "tasks:manage" || permission === "tasks:configure") next.add("tasks:own");
    if (permission === "tasks:configure") next.add("tasks:manage");
  }
  return normalizePermissions([...next]);
}

function roleDraft(role: RoleDefinition): RoleDraft {
  return {
    name: role.name,
    base_role: role.base_role === "operator" ? "operator" : "manager",
    permissions: role.permissions,
  };
}

export function RolesView({ auth }: RolesViewProps) {
  const { t } = useI18n();
  const [roles, setRoles] = useState<RoleDefinition[]>([]);
  const [drafts, setDrafts] = useState<Record<string, RoleDraft>>({});
  const [newName, setNewName] = useState("");
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<MessageKey | null>(null);

  async function reload() {
    const items = await listRoles(auth);
    setRoles(items);
    setDrafts(Object.fromEntries(items.map((item) => [item.id, roleDraft(item)])));
  }

  useEffect(() => {
    let active = true;
    listRoles(auth)
      .then((items) => {
        if (!active) return;
        setRoles(items);
        setDrafts(Object.fromEntries(items.map((item) => [item.id, roleDraft(item)])));
      })
      .catch(() => {
        if (active) setError("roles.loadError");
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => { active = false; };
  }, [auth]);

  function isDirty(role: RoleDefinition): boolean {
    const draft = drafts[role.id];
    return Boolean(draft) && JSON.stringify(roleDraft(role)) !== JSON.stringify(draft);
  }

  async function saveRole(role: RoleDefinition) {
    const draft = drafts[role.id];
    if (!draft || busyId || !isDirty(role)) return;
    setBusyId(role.id);
    setError(null);
    try {
      const updated = await updateRole(auth, role.id, draft);
      setRoles((current) => current.map((item) => item.id === role.id ? updated : item));
      setDrafts((current) => ({ ...current, [role.id]: roleDraft(updated) }));
    } catch {
      setError("roles.saveError");
    } finally {
      setBusyId(null);
    }
  }

  async function handleCreate(event: React.FormEvent) {
    event.preventDefault();
    if (!newName.trim() || creating) return;
    setCreating(true);
    setError(null);
    try {
      await createRole(auth, {
        name: newName.trim(),
        base_role: "operator",
        permissions: defaultPermissions,
      });
      setNewName("");
      await reload();
    } catch {
      setError("roles.createError");
    } finally {
      setCreating(false);
    }
  }

  async function handleDelete(role: RoleDefinition) {
    if (busyId || !window.confirm(t("roles.deleteConfirm", { name: role.name }))) return;
    setBusyId(role.id);
    setError(null);
    try {
      await deleteRole(auth, role.id);
      await reload();
    } catch {
      setError("roles.deleteError");
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="page roles-page">
      <header className="page-toolbar">
        <div><h1>{t("roles.title")}</h1><p>{t("roles.description")}</p></div>
      </header>

      {error && <div className="admin-notice admin-notice--error" role="alert">{t(error)}</div>}
      <form className="role-create-form" onSubmit={handleCreate}>
        <label>
          <span>{t("roles.name")}</span>
          <input value={newName} maxLength={100} placeholder={t("roles.namePlaceholder")} onChange={(event) => setNewName(event.target.value)} />
        </label>
        <DemoActionButton className="primary-button" type="submit" disabled={creating || !newName.trim()}>
          <Plus size={16} />{creating ? t("roles.creating") : t("roles.create")}
        </DemoActionButton>
      </form>

      <div className="role-settings-grid">
        {roles.map((role) => {
          const draft = drafts[role.id] ?? roleDraft(role);
          const usage = role.member_count + role.active_token_count;
          return (
            <section className="settings-panel role-settings-card" key={role.id}>
              <header>
                <div className="role-settings-heading"><ShieldCheck size={18} /><h2>{role.name}</h2></div>
                <span className={role.is_system ? "role-settings-badge role-settings-badge--locked" : "role-settings-badge"}>
                  {role.is_system ? t("roles.systemRole") : t("roles.editable")}
                </span>
              </header>
              <div className="role-identity-fields">
                <label>
                  <span>{t("roles.name")}</span>
                  <input
                    value={draft.name}
                    maxLength={100}
                    disabled={role.is_system || busyId === role.id}
                    onChange={(event) => setDrafts((current) => ({ ...current, [role.id]: { ...draft, name: event.target.value } }))}
                  />
                </label>
              </div>
              <p className="role-settings-summary">
                {role.is_system ? t("roles.adminDescription") : t("roles.usage", { members: role.member_count, tokens: role.active_token_count })}
              </p>
              <div className="role-permission-list">
                {permissionDefinitions
                  .filter((item) => !["projects:", "processes:"].some((prefix) => item.permission.startsWith(prefix)))
                  .filter((item) => !role.is_system ? item.editable : role.permissions.includes(item.permission))
                  .map((item) => {
                    const tokenRestricted = draft.permissions.includes(item.permission) && accessTokenDeniedPermissions.has(item.permission);
                    return (
                      <label className="role-permission-row" key={item.permission}>
                        <input
                          type="checkbox"
                          checked={draft.permissions.includes(item.permission)}
                          disabled={role.is_system || busyId === role.id}
                          onChange={() => setDrafts((current) => ({ ...current, [role.id]: { ...draft, permissions: togglePermission(draft.permissions, item.permission) } }))}
                        />
                        <span><strong>{t(item.label)}</strong><small>{t(item.description)}</small>{tokenRestricted && <em>{t("roles.sessionOnly")}</em>}</span>
                      </label>
                    );
                  })}
              </div>
              <footer>
                {role.is_system && <span><LockKeyhole size={14} />{t("roles.adminLocked")}</span>}
                {!role.is_system && (
                  <div className="role-card-actions">
                    <DemoActionButton className="primary-button" type="button" disabled={!isDirty(role) || busyId !== null || !draft.name.trim()} onClick={() => void saveRole(role)}>
                      <Save size={16} />{busyId === role.id ? t("roles.saving") : t("roles.save")}
                    </DemoActionButton>
                    <DemoActionButton className="danger-icon-button" type="button" aria-label={t("roles.delete", { name: role.name })} title={usage > 0 ? t("roles.deleteBlocked") : t("roles.delete", { name: role.name })} disabled={usage > 0 || busyId !== null} onClick={() => void handleDelete(role)}>
                      <Trash2 size={16} />
                    </DemoActionButton>
                  </div>
                )}
              </footer>
            </section>
          );
        })}
      </div>
      {!loading && roles.length === 0 && !error && <div className="empty-state">{t("roles.empty")}</div>}
      {loading && <div className="empty-state">{t("roles.loading")}</div>}
      <p className="role-settings-note">{t("roles.propagationNote")}</p>
    </div>
  );
}
