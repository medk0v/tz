import { useState } from "react";
import { Shield } from "lucide-react";
import type { OperatorAuth } from "../api";
import { useI18n } from "../i18n";
import { ProjectAccess, type ProjectAccessNotice } from "./ProjectAccess";
import { usersText } from "./users-i18n";

interface UsersViewProps {
  auth: OperatorAuth;
  actorId: string;
  projectId: string;
  onChanged: () => void | Promise<void>;
  onManageRoles: () => void;
}

export function UsersView({ auth, actorId, projectId, onChanged, onManageRoles }: UsersViewProps) {
  const { t, locale } = useI18n();
  const u = usersText(locale);
  const [notice, setNotice] = useState<ProjectAccessNotice | null>(null);

  return (
    <div className="page users-page">
      <header className="page-toolbar">
        <div><h1>{u("title")}</h1><p>{u("description")}</p></div>
        <button className="secondary-button" type="button" onClick={onManageRoles}>
          <Shield size={16} />{u("manageRoles")}
        </button>
      </header>
      {notice && <div className={`admin-notice admin-notice--${notice.kind}`} role={notice.kind === "error" ? "alert" : "status"}>{t(notice.message)}</div>}
      <ProjectAccess
        key={projectId}
        auth={auth}
        project={{ id: projectId, current: true }}
        actorId={actorId}
        onChanged={onChanged}
        onNotice={setNotice}
        allowCreate
      />
    </div>
  );
}
