import { useEffect, useRef, useState } from "react";
import { listProjects, PROJECTS_CHANGED_EVENT, type OperatorAuth, type Project } from "../api";
import { useI18n } from "../i18n";
import { PreferenceDropdown } from "./PreferenceDropdown";
import { projectHierarchyRows } from "./project-hierarchy";
import "./ProjectHierarchy.css";

interface Props {
  auth: OperatorAuth;
  projectId: string;
  projectName: string;
  disabled: boolean;
  allProjects?: boolean;
  allowAllProjects?: boolean;
  onProjectsLoaded?: (projects: Project[]) => void;
  onChange: (projectId: string) => Promise<void>;
}

export function ProjectSwitcher({ auth, projectId, projectName, disabled, allProjects = false, allowAllProjects = false, onChange, onProjectsLoaded }: Props) {
  const { t } = useI18n();
  const onProjectsLoadedRef = useRef(onProjectsLoaded);
  useEffect(() => { onProjectsLoadedRef.current = onProjectsLoaded; }, [onProjectsLoaded]);
  const [revision, setRevision] = useState(0);
  const [result, setResult] = useState<{ auth: OperatorAuth; revision: number; items?: Project[]; error?: boolean } | null>(null);
  const [switchFailed, setSwitchFailed] = useState(false);
  const current = result?.auth === auth && result.revision === revision ? result : null;
  const projects = current?.items?.filter((project) => project.status === "active");

  useEffect(() => {
    const refresh = () => setRevision((value) => value + 1);
    window.addEventListener(PROJECTS_CHANGED_EVENT, refresh);
    return () => window.removeEventListener(PROJECTS_CHANGED_EVENT, refresh);
  }, []);

  useEffect(() => {
    let active = true;
    void listProjects(auth).then(
      (items) => { if (active) { setResult({ auth, revision, items }); onProjectsLoadedRef.current?.(items); } },
      () => { if (active) { setResult({ auth, revision, error: true }); onProjectsLoadedRef.current?.([]); } },
    );
    return () => { active = false; };
  }, [auth, revision]);

  async function changeProject(nextId: string) {
    setSwitchFailed(false);
    try {
      await onChange(nextId);
    } catch {
      setSwitchFailed(true);
    }
  }

  return <div className="project-switcher" aria-busy={!current || disabled}>
    <PreferenceDropdown
      label={t("nav.projects")}
      value={allProjects ? "all-projects" : projectId}
      placeholder={allProjects ? t("projects.all") : projectName}
      options={[
        ...(allowAllProjects && projects?.length ? [{ value: "all-projects", label: t("projects.all") }] : []),
        ...projectHierarchyRows(projects ?? []).map(({ project, depth }) => ({ value: project.id, label: project.name, depth })),
      ]}
      disabled={disabled || !current}
      onChange={(value) => void changeProject(value)}
    />
    {!current && <div className="project-switcher-notice" role="status">{t("projects.loading")}</div>}
    {current?.error && <div className="project-switcher-notice" role="alert">
      {t("projects.loadError")}
      <button type="button" disabled={disabled} onClick={() => setRevision((value) => value + 1)}>{t("projectGate.retry")}</button>
    </div>}
    {projects?.length === 0 && <div className="project-switcher-notice" role="status">{t("projects.empty")}</div>}
    {switchFailed && <div className="project-switcher-notice" role="alert">{t("projects.switchError")}</div>}
  </div>;
}
