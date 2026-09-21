import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Building2,
  FolderKanban,
  LogOut,
  RefreshCw,
} from "lucide-react";
import {
  listProjects,
  type OperatorAuth,
  type Project,
} from "../api";
import { roleLabel, useI18n, type MessageKey } from "../i18n";
import { adminPagePath } from "../entry-mode";

interface ProjectSelectionScreenProps {
  auth: OperatorAuth;
  branded: boolean;
  headerActions: ReactNode;
  onProjectChange: (projectId: string) => Promise<void>;
  onSignOut: () => Promise<void>;
  onBack?: () => void;
}

interface Notice {
  kind: "error" | "success";
  message: MessageKey;
}

type GateOperation =
  | { kind: "opening"; projectId: string }
  | { kind: "retrying" }
  | { kind: "signing_out" }
  | null;


export function ProjectSelectionScreen({
  auth,
  branded,
  headerActions,
  onProjectChange,
  onSignOut,
  onBack,
}: ProjectSelectionScreenProps) {
  const { t } = useI18n();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const [projects, setProjects] = useState<Project[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadFailed, setLoadFailed] = useState(false);
  const [operation, setOperation] = useState<GateOperation>(null);
  const [notice, setNotice] = useState<Notice | null>(null);

  useEffect(() => {
    headingRef.current?.focus();
  }, []);

  useEffect(() => {
    let active = true;
    listProjects(auth)
      .then((items) => {
        if (!active) return;
        setProjects(items);
        
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
  }, [auth]);

  async function reloadProjects() {
    if (operation) return;
    setOperation({ kind: "retrying" });
    setNotice(null);
    try {
      const items = await listProjects(auth);
      setProjects(items);
      setLoadFailed(false);
      
    } catch {
      setLoadFailed(true);
    } finally {
      setOperation(null);
    }
  }

  async function openProject(projectId: string) {
    if (operation) return;
    setOperation({ kind: "opening", projectId });
    setNotice(null);
    try {
      await onProjectChange(projectId);
    } catch {
      setNotice({ kind: "error", message: "projects.switchError" });
      setOperation(null);
    }
  }

  async function signOut() {
    if (operation) return;
    setOperation({ kind: "signing_out" });
    await onSignOut();
  }

  return (
    <main className={`connection-page project-gate${branded ? "" : " connection-page--neutral"}`}>
      <section className="project-gate-panel" aria-labelledby="project-gate-heading">
        <header className="connection-brand project-gate-brand">
          {branded ? (
            <div className="connection-brand-copy">
              <span>{t("login.operatorWorkspace")}</span>
            </div>
          ) : <span aria-hidden="true" />}
          {headerActions}
        </header>

        {onBack && (
          <button className="back-button" type="button" disabled={operation !== null} onClick={onBack}>
            <ArrowLeft size={16} />
            {t("projectGate.back")}
          </button>
        )}

        <div className="project-gate-heading">
          <h1 id="project-gate-heading" ref={headingRef} tabIndex={-1}>{t("projectGate.title")}</h1>
          <p>{t("projectGate.description")}</p>
        </div>

        {notice && (
          <div
            className={`admin-notice admin-notice--${notice.kind}`}
            role={notice.kind === "error" ? "alert" : "status"}
          >
            {t(notice.message)}
          </div>
        )}

        {loading ? (
          <div className="project-gate-state" role="status">{t("projects.loading")}</div>
        ) : loadFailed ? (
          <div className="project-gate-state project-gate-state--error" role="alert">
            <strong>{t("projectGate.loadError")}</strong>
            <button className="secondary-button" type="button" disabled={operation !== null} onClick={() => void reloadProjects()}>
              <RefreshCw size={16} />
              {operation?.kind === "retrying" ? t("projectGate.retrying") : t("projectGate.retry")}
            </button>
          </div>
        ) : projects.length > 0 ? (
          <div className="project-gate-list" role="group" aria-label={t("projectGate.listLabel")}>
            {projects.map((project) => {
              const disabled = project.status !== "active" || operation !== null;
              const switching = operation?.kind === "opening"
                && operation.projectId === project.id;
              return (
                <a
                  className="project-gate-row"
                  key={project.id}
                  href={disabled ? undefined : adminPagePath("conversations", project.id)}
                  role="link"
                  aria-disabled={disabled || undefined}
                  tabIndex={disabled ? -1 : undefined}
                  aria-label={t("projectGate.openProject", { name: project.name })}
                  onClick={(event) => {
                    if (disabled) { event.preventDefault(); return; }
                    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
                    event.preventDefault();
                    void openProject(project.id);
                  }}
                >
                  {project.project_kind === "company" ? <Building2 size={19} /> : <FolderKanban size={19} />}
                  <span className="project-gate-project">
                    <strong>{project.name}</strong>
                    <small>
                      {project.slug} · {roleLabel(t, project.role)} · {t(
                        project.status === "active"
                          ? "projects.statusActive"
                          : "projects.statusDisabled",
                      )}
                    </small>
                    {project.current && <span className="project-gate-current">{t("projects.currentShort")}</span>}
                  </span>
                  <span className="project-gate-open">
                    {switching ? t("projects.switching") : t("projects.openProject")}
                    {!switching && <ArrowRight size={16} />}
                  </span>
                </a>
              );
            })}
          </div>
        ) : (
          <div className="project-gate-state">
            <strong>{t("projects.empty")}</strong>
            <span>{t("projectGate.emptyMember")}</span>
          </div>
        )}

        

        <button className="project-gate-signout" type="button" disabled={operation !== null} onClick={() => void signOut()}>
          <LogOut size={16} />
          {operation?.kind === "signing_out" ? t("projectGate.signingOut") : t("nav.signOut")}
        </button>
      </section>
    </main>
  );
}

export function ProjectScopedTokenRequiredScreen({
  branded,
  headerActions,
  onDisconnect,
}: {
  branded: boolean;
  headerActions: ReactNode;
  onDisconnect: () => Promise<void>;
}) {
  const { t } = useI18n();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const [disconnecting, setDisconnecting] = useState(false);

  useEffect(() => {
    headingRef.current?.focus();
  }, []);

  return (
    <main className={`connection-page project-gate${branded ? "" : " connection-page--neutral"}`}>
      <section className="project-gate-panel project-gate-panel--compact" aria-labelledby="project-token-heading">
        <header className="connection-brand project-gate-brand">
          {branded ? (
            <div className="connection-brand-copy">
              <span>{t("login.operatorWorkspace")}</span>
            </div>
          ) : <span aria-hidden="true" />}
          {headerActions}
        </header>
        <div className="project-gate-heading">
          <h1 id="project-token-heading" ref={headingRef} tabIndex={-1}>{t("projectGate.tokenTitle")}</h1>
          <p>{t("projectGate.tokenDescription")}</p>
        </div>
        <button
          className="secondary-button project-gate-token-action"
          type="button"
          disabled={disconnecting}
          onClick={() => {
            setDisconnecting(true);
            void onDisconnect();
          }}
        >
          <LogOut size={16} />
          {disconnecting ? t("projectGate.disconnecting") : t("nav.disconnect")}
        </button>
      </section>
    </main>
  );
}
