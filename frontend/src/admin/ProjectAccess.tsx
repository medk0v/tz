import { useEffect, useState } from "react";
import { Pencil, Trash2, UserPlus } from "lucide-react";
import {
  ApiRequestError,
  createProjectUser,
  grantProjectMember,
  listProjectMembers,
  listRoles,
  revokeProjectMember,
  updateProjectMember,
  type OperatorAuth,
  type Project,
  type ProjectMember,
  type RoleDefinition,
} from "../api";
import { useI18n, type MessageKey } from "../i18n";
import { DemoActionButton } from "./DemoReadOnly";
import { OperatorAvatar } from "./OperatorAvatar";
import { OperatorProfileDialog } from "./OperatorProfileDialog";
import { usersText } from "./users-i18n";
import "./ProjectAccess.css";
import "./UsersView.css";

export interface ProjectAccessNotice {
  kind: "success" | "error";
  message: MessageKey;
}

export function ProjectAccess({
  auth,
  project,
  actorId,
  onChanged,
  onNotice,
  departmentRevision = 0,
  onDepartmentsChanged,
  allowCreate = false,
}: {
  auth: OperatorAuth;
  project: Pick<Project, "id" | "current">;
  actorId: string;
  onChanged: () => void | Promise<void>;
  onNotice: (notice: ProjectAccessNotice) => void;
  departmentRevision?: number;
  onDepartmentsChanged?: () => void;
  allowCreate?: boolean;
}) {
  const { t, locale } = useI18n();
  const u = usersText(locale);
  const [accountMode, setAccountMode] = useState<"new" | "existing">(allowCreate ? "new" : "existing");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [created, setCreated] = useState(false);
  const passwordTooLong = new TextEncoder().encode(password).length > 128;
  const validNewAccount = Boolean(displayName.trim()) && Array.from(password).length >= 8 && !passwordTooLong;
  const [members, setMembers] = useState<ProjectMember[]>([]);
  const [roles, setRoles] = useState<RoleDefinition[]>([]);
  const [email, setEmail] = useState("");
  const [roleId, setRoleId] = useState("");
  const [departmentId, setDepartmentId] = useState("");
  const grantAdmin = roles.find((role) => role.id === roleId)?.base_role === "admin";
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const [loadRevision, setLoadRevision] = useState(0);
  const [saving, setSaving] = useState(false);
  const [changingId, setChangingId] = useState<string | null>(null);
  const [profileMember, setProfileMember] = useState<ProjectMember | null>(null);
  const [accessError, setAccessError] = useState<{ membershipId: string | null; message: string } | null>(null);

  useEffect(() => {
    let active = true;
    Promise.all([listProjectMembers(auth, project.id), listRoles(auth)])
      .then(([items, availableRoles]) => {
        if (!active) return;
        setMembers(items);
        setRoles(availableRoles);
        setLoadError(false);
        setRoleId((current) => availableRoles.some((item) => item.id === current)
          ? current
          : availableRoles.find((item) => item.base_role === "operator")?.id || availableRoles.find((item) => item.base_role !== "admin")?.id || "");
      })
      .catch(() => {
        if (active) setLoadError(true);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [auth, project.id, departmentRevision, loadRevision]);

  async function notifyChanged() {
    onDepartmentsChanged?.();
    try {
      await onChanged();
    } catch {
      onNotice({ kind: "error", message: "projects.loadError" });
    }
  }

  async function handleGrant(event: React.FormEvent) {
    event.preventDefault();
    if (auth.isDemo || !email.trim() || !roleId || saving || changingId || loading || loadError || (accountMode === "new" && !validNewAccount)) return;
    setSaving(true);
    setAccessError(null);
    setCreated(false);
    try {
      const access = { email: email.trim(), role_id: roleId, department_id: grantAdmin ? null : departmentId || null, director_access: grantAdmin };
      const member = accountMode === "new"
        ? await createProjectUser(auth, project.id, { ...access, display_name: displayName.trim(), password })
        : await grantProjectMember(auth, project.id, access);
      setMembers((current) => [...current.filter((item) => item.membership_id !== member.membership_id), member]
        .sort((left, right) => left.display_name.localeCompare(right.display_name)));
      setEmail("");
      setDisplayName("");
      setPassword("");
      if (accountMode === "new") setCreated(true);
      else onNotice({ kind: "success", message: "projects.memberGranted" });
      await notifyChanged();
    } catch (error) {
      const message = accountMode === "new"
        ? u(error instanceof ApiRequestError && error.status === 409 ? "accountExists" : "createError")
        : error instanceof Error ? error.message : t("projects.memberGrantError");
      setAccessError({ membershipId: null, message });
    } finally {
      setSaving(false);
    }
  }

  async function handleMemberChange(member: ProjectMember, patch: { role_id?: string; department_id?: string | null; director_access?: boolean }) {
    if (auth.isDemo || changingId || saving || member.user_id === actorId) return;
    setChangingId(member.membership_id);
    setCreated(false);
    setAccessError(null);
    try {
      const updated = await updateProjectMember(
        auth,
        project.id,
        member.membership_id,
        patch,
      );
      setMembers((current) => current.map((item) => (
        item.membership_id === updated.membership_id ? updated : item
      )));
      onNotice({ kind: "success", message: "projects.memberUpdated" });
      await notifyChanged();
    } catch (error) {
      setAccessError({ membershipId: member.membership_id, message: error instanceof Error ? error.message : t("projects.memberUpdateError") });
      onNotice({ kind: "error", message: "projects.memberUpdateError" });
    } finally {
      setChangingId(null);
    }
  }

  async function handleRevoke(member: ProjectMember) {
    if (auth.isDemo || changingId || saving || member.user_id === actorId) return;
    setChangingId(member.membership_id);
    setCreated(false);
    setAccessError(null);
    try {
      await revokeProjectMember(auth, project.id, member.membership_id);
      setMembers((current) => current.filter(
        (item) => item.membership_id !== member.membership_id,
      ));
      onNotice({ kind: "success", message: "projects.memberRevoked" });
      await notifyChanged();
    } catch {
      onNotice({ kind: "error", message: "projects.memberRevokeError" });
    } finally {
      setChangingId(null);
    }
  }

  return (
    <section className="project-section project-access-section">
      <header>
        <div><h3>{t("projects.accessTitle")}</h3><p>{t("projects.accessDescription")}</p></div>
      </header>
      {loadError && <div className="admin-notice admin-notice--error project-access-feedback project-access-retry" role="alert">
        <p>{t("projects.membersLoadError")}</p>
        <button className="secondary-button" type="button" disabled={loading} onClick={() => { setLoading(true); setLoadRevision((current) => current + 1); }}>{u("retry")}</button>
      </div>}
      {allowCreate && <fieldset className="project-user-mode" disabled={saving || Boolean(changingId) || auth.isDemo}>
        <legend>{u("accountAction")}</legend>
        {(["new", "existing"] as const).map((mode) => <label key={mode}>
          <input type="radio" name={`account-mode-${project.id}`} value={mode} checked={accountMode === mode} onChange={() => { setAccountMode(mode); setPassword(""); setCreated(false); setAccessError(null); }} />
          <span>{u(mode === "new" ? "newAccount" : "existingAccount")}</span>
        </label>)}
      </fieldset>}
      <form className={`project-member-form project-member-form--departments ${accountMode === "new" ? "project-member-form--create" : ""}`} onSubmit={handleGrant} aria-busy={saving}>
        {accountMode === "new" && <label>
          <span>{u("displayName")}</span>
          <input value={displayName} maxLength={200} required autoComplete="name" disabled={saving || auth.isDemo} onChange={(event) => setDisplayName(event.target.value)} />
        </label>}
        <label>
          <span>{t("projects.memberEmail")}</span>
          <input
            type="email"
            value={email}
            maxLength={320}
            required
            autoComplete="off"
            disabled={saving || auth.isDemo}
            placeholder={t("projects.memberEmailPlaceholder")}
            onChange={(event) => setEmail(event.target.value)}
          />
        </label>
        {accountMode === "new" && <label>
          <span>{u("password")}</span>
          <input type="password" value={password} minLength={8} maxLength={128} required autoComplete="new-password" disabled={saving || auth.isDemo} aria-describedby={`user-password-help-${project.id}`} aria-invalid={passwordTooLong || undefined} onChange={(event) => setPassword(event.target.value)} />
        </label>}
        <label>
          <span>{t("projects.memberRole")}</span>
          <select value={roleId} required disabled={saving || loading || loadError || auth.isDemo} onChange={(event) => { setRoleId(event.target.value); if (roles.find((role) => role.id === event.target.value)?.base_role === "admin") setDepartmentId(""); }}>
            <option value="" disabled>{u("chooseRole")}</option>
            {roles.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}
          </select>
        </label>
        
        
        <DemoActionButton className="primary-button" type="submit" disabled={saving || loading || loadError || auth.isDemo || Boolean(changingId) || !email.trim() || !roleId || (accountMode === "new" && !validNewAccount)}>
          <UserPlus size={16} />
          {accountMode === "new" ? u(saving ? "creating" : "create") : t(saving ? "projects.granting" : "projects.grantAccess")}
        </DemoActionButton>
        {accountMode === "new" && <p id={`user-password-help-${project.id}`} className="project-password-help">{u(passwordTooLong ? "passwordTooLong" : "passwordHelp")}</p>}
      </form>
      {created && <p className="admin-notice admin-notice--success project-access-feedback" role="status">{u("created")}</p>}
      {accessError?.membershipId === null && <p className="admin-notice admin-notice--error project-access-feedback" role="alert">{accessError.message}</p>}
      {allowCreate && <p className="department-help project-access-help">{u(grantAdmin ? "adminHelp" : "roleHelpLite")}</p>}
      
      <div className="project-member-table">
        <table>
          <thead>
            <tr>
              <th>{t("projects.member")}</th>
              <th>{t("projects.memberRole")}</th>
              
              
              <th><span className="visually-hidden">{t("common.actions")}</span></th>
            </tr>
          </thead>
          <tbody>
            {members.map((member) => {
              const ownMembership = member.user_id === actorId;
              return (
                <tr key={member.membership_id}>
                  <td>
                    <div className="project-member-identity">
                      <OperatorAvatar displayName={member.chat_display_name} avatarUrl={member.avatar_url} />
                      <div>
                        <strong>{member.display_name}</strong>
                        <span>{member.chat_display_name === member.display_name ? member.email : `${member.chat_display_name} · ${member.email}`}</span>
                      </div>
                    </div>
                  </td>
                  <td>
                    <select
                      aria-label={t("projects.changeMemberRole", { name: member.display_name })}
                      aria-describedby={accessError?.membershipId === member.membership_id ? `member-access-error-${member.membership_id}` : undefined}
                      value={member.role_id}
                      disabled={auth.isDemo || ownMembership || saving || Boolean(changingId)}
                      title={ownMembership ? u("ownAccess") : undefined}
                      onChange={(event) => void handleMemberChange(member, { role_id: event.target.value, ...(roles.find((role) => role.id === event.target.value)?.base_role === "admin" ? { department_id: null, director_access: true } : {}) })}
                    >
                      {roles.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}
                    </select>
                    {accessError?.membershipId === member.membership_id && <p id={`member-access-error-${member.membership_id}`} className="department-access-error" role="alert">{accessError.message}</p>}
                  </td>
                  
                  
                  <td className="table-action-cell">
                    <div className="project-member-actions">
                      {project.current && (
                        <button
                          className="icon-button"
                          type="button"
                          aria-label={t("projects.editChatProfile", { name: member.display_name })}
                          title={t("projects.editChatProfile", { name: member.display_name })}
                          disabled={saving || Boolean(changingId)}
                          onClick={() => setProfileMember(member)}
                        >
                          <Pencil size={15} />
                        </button>
                      )}
                      <DemoActionButton
                        className="danger-icon-button"
                        type="button"
                        aria-label={t("projects.revokeMember", { name: member.display_name })}
                        disabled={auth.isDemo || ownMembership || saving || Boolean(changingId)}
                        title={ownMembership ? u("ownAccess") : undefined}
                        onClick={() => void handleRevoke(member)}
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
        {!loading && !loadError && members.length === 0 && <div className="empty-state">{t("projects.noMembers")}</div>}
        {loading && <div className="empty-state" role="status">{t("projects.loadingMembers")}</div>}
      </div>
      {profileMember && (
        <OperatorProfileDialog
          auth={auth}
          operatorId={profileMember.user_id}
          displayName={profileMember.chat_display_name}
          avatarUrl={profileMember.avatar_url}
          mode="managed"
          onClose={() => setProfileMember(null)}
          onSaved={(saved) => {
            setMembers((current) => current.map((member) => member.user_id === saved.user_id
              ? {
                  ...member,
                  chat_display_name: saved.display_name,
                  avatar_url: saved.avatar_url,
                }
              : member));
            setProfileMember(null);
          }}
        />
      )}
    </section>
  );
}
