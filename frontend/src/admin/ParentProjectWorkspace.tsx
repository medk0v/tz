import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { getCurrentActor, listChannels, listInboxes, type ActorContext, type ChannelConnection, type Inbox, type OperatorAuth, type Project, type RealtimeEvent } from "../api";
import { type Department } from "../department-api";
import type { AdminPage, AdminPageSegments } from "../entry-mode";
import { useI18n } from "../i18n";
import { productNamespace } from "../product-edition";
import { MemoryPageRoute } from "./MemoryPageRoute";
import { ProjectScopePicker } from "./ProjectScopePicker";
import { projectHierarchyText } from "./project-hierarchy-i18n";
import { useDepartmentWorkspace } from "./useDepartmentWorkspace";
import { useOperatorRealtime } from "./useOperatorRealtime";
import "./ParentProjectWorkspace.css";

export type WorkspaceDirtySection = "routing" | "templates" | "knowledge" | "notes";
type Navigate = (page: AdminPage, projectId?: string | null, segments?: AdminPageSegments) => boolean;
export interface ProjectWorkspaceScope {
  actor: ActorContext;
  auth: OperatorAuth;
  inboxes: Inbox[];
  channels?: ChannelConnection[];
  activeInboxId: string | null;
  realtimeEvent: RealtimeEvent | null;
  realtimeSyncRevision: number;
  selectInbox: (id: string) => void;
  refresh: () => Promise<void>;
  department: Department | null;
  navigate: Navigate;
  changeDepartment: (id: string) => Promise<void>;
  dirtyChange: (name: WorkspaceDirtySection, dirty: boolean) => void;
}
interface Props {
  scopeContainer?: HTMLElement | null;
  enabled: boolean;
  auth: OperatorAuth;
  actor: ActorContext;
  page: AdminPage | undefined;
  projects: readonly Project[];
  canShowPage: (actor: ActorContext, department: Department | null) => boolean;
  onNavigate: Navigate;
  onBeforeChange: () => boolean;
  onDepartmentSelect: (projectId: string, departmentId: string) => Promise<void>;
  onDirtyChange: (name: WorkspaceDirtySection, dirty: boolean) => void;
  renderWorkspace: (scope?: ProjectWorkspaceScope) => ReactNode;
}

function readProjectSelection(key: string, currentProject: string) {
  try { return localStorage.getItem(key) ?? currentProject; }
  catch { return currentProject; }
}

function selectedProjectIds(value: string, projects: readonly Project[], parentId: string): string[] {
  if (value === "all") return projects.map((project) => project.id);
  if (projects.some((project) => project.id === value)) return [value];
  try {
    const ids: unknown = JSON.parse(value);
    if (Array.isArray(ids) && ids.every((id) => typeof id === "string")) {
      const available = projects.filter((project) => ids.includes(project.id)).map((project) => project.id);
      if (!ids.length || available.length) return available;
    }
  } catch { /* Old or unavailable project selection. */ }
  return [parentId];
}

/** Each section uses the project's existing API authorization, including its department and token scope. */
export function ParentProjectWorkspace(props: Props) {
  const { actor, enabled, projects: accessibleProjects, renderWorkspace, onBeforeChange, onDirtyChange } = props;
  const { locale } = useI18n();
  const text = projectHierarchyText(locale);
  const selectionKey = `${productNamespace}-admin-project-scope:${actor.tenant_id}:${actor.actor_id}:${actor.project_id}:${props.page ?? "default"}`;
  const [selected, setSelected] = useState(() => ({ key: selectionKey, value: readProjectSelection(selectionKey, actor.project_id ?? "") }));
  if (selected.key !== selectionKey) setSelected({ key: selectionKey, value: readProjectSelection(selectionKey, actor.project_id ?? "") });
  const dirtySections = useRef(new Map<WorkspaceDirtySection, Set<string>>());
  const dirtyHandler = useRef(onDirtyChange);
  useEffect(() => { dirtyHandler.current = onDirtyChange; }, [onDirtyChange]);
  const dirtyChange = useCallback((projectId: string, name: WorkspaceDirtySection, dirty: boolean) => {
    const projects = dirtySections.current.get(name) ?? new Set<string>();
    if (dirty) projects.add(projectId); else projects.delete(projectId);
    dirtySections.current.set(name, projects);
    dirtyHandler.current(name, projects.size > 0);
  }, []);
  useEffect(() => {
    const sections = dirtySections.current;
    return () => { for (const name of sections.keys()) dirtyHandler.current(name, false); };
  }, []);
  const children = accessibleProjects.filter((project) => project.parent_project_id === actor.project_id && project.status === "active");
  if (!enabled || !children.length) return <>{renderWorkspace()}</>;
  const parent = accessibleProjects.find((project) => project.id === actor.project_id);
  if (!parent) return <>{renderWorkspace()}</>;
  const projects = [parent, ...children];
  const selection = selectedProjectIds(selected.value, projects, parent.id);
  const visibleProjects = projects.filter((project) => selection.includes(project.id));
  const singleProject = visibleProjects.length === 1 ? visibleProjects[0] : null;

  const selector = <div className="department-switcher project-scope-control">
      <span>{text("dataScope")}</span>
      <ProjectScopePicker projects={projects} parentId={parent.id} selectedIds={selection} onChange={(ids) => {
        if (!onBeforeChange()) return;
        const value = ids.length === projects.length ? "all" : ids.length === 1 ? ids[0] : JSON.stringify(ids);
        setSelected({ key: selectionKey, value });
        try { localStorage.setItem(selectionKey, value); } catch { /* Preferences are optional. */ }
      }} />
      {singleProject && singleProject.id !== parent.id && <button type="button" className="project-scope-open" onClick={() => props.page && props.onNavigate(props.page, singleProject.id)}>{text("openProject")}</button>}
    </div>;

  return <>
    {props.scopeContainer && createPortal(selector, props.scopeContainer)}
    {!visibleProjects.length ? <p className="empty-state">{text("selectProjectsHint")}</p> : singleProject ? <MemoryPageRoute key={singleProject.id}>
      <ScopedProjectWorkspace {...props} project={singleProject} dirtyChange={dirtyChange} />
    </MemoryPageRoute> : <div className="parent-project-workspace">
    {visibleProjects.map((project) =>
      <section className="parent-project-section" key={project.id} aria-label={project.name}>
        <header className="parent-project-heading">
          <h2>{project.name}</h2>
          <span>{text(project.id === parent.id ? "parent" : "subproject")}</span>
          {project.id !== parent.id && <button type="button" className="secondary-button" onClick={() => props.page && props.onNavigate(props.page, project.id)}>{text("openProject")}</button>}
        </header>
        <MemoryPageRoute>
          <ScopedProjectWorkspace {...props} project={project} dirtyChange={dirtyChange} />
        </MemoryPageRoute>
      </section>,
    )}
    </div>}
  </>;
}

function ScopedProjectWorkspace({ project, auth: parentAuth, actor: parentActor, page, canShowPage, onNavigate, onDepartmentSelect, renderWorkspace, dirtyChange }: Props & {
  project: Project;
  dirtyChange: (projectId: string, name: WorkspaceDirtySection, dirty: boolean) => void;
}) {
  const { t } = useI18n();
  const auth = useMemo<OperatorAuth>(() => ({ ...parentAuth, projectId: project.id }), [parentAuth, project.id]);
  const [revision, setRevision] = useState(0);
  const [result, setResult] = useState<{ actor: ActorContext; inboxes: Inbox[]; channels: ChannelConnection[]; auth: OperatorAuth } | null>(null);
  const [error, setError] = useState(false);
  const [selectedInboxId, setSelectedInboxId] = useState<string | null>(null);
  const [realtimeEvent, setRealtimeEvent] = useState<RealtimeEvent | null>(null);
  const [realtimeSyncRevision, setRealtimeSyncRevision] = useState(0);
  const sync = useCallback(() => setRealtimeSyncRevision((value) => value + 1), []);
  const departments = useDepartmentWorkspace(auth, result?.actor ?? null);
  const department = departments.data?.items.find((item) => item.id === result?.actor.department_id) ?? null;
  const activeInboxId = result?.inboxes.some((inbox) => inbox.id === selectedInboxId) ? selectedInboxId : result?.inboxes[0]?.id ?? null;
  useOperatorRealtime(page === "conversations" || page === "online_visitors" ? auth : null, activeInboxId, setRealtimeEvent, sync);
  useEffect(() => {
    let active = true;
    async function load() {
      const actor = project.id === parentActor.project_id && revision === 0 ? parentActor : await getCurrentActor(auth);
      if (actor.project_id !== project.id) throw new Error("Project unavailable");
      const inboxes = ["conversations:read", "routing:manage", "reply_templates:manage"].some((permission) => actor.permissions.includes(permission))
        ? await listInboxes(auth) : [];
      const channels = page === "conversations" && actor.permissions.includes("channels:read") ? await listChannels(auth) : [];
      if (active) { setResult({ actor, inboxes, channels, auth }); setError(false); }
    }
    void load().catch(() => { if (active) { setResult(null); setError(true); } });
    return () => { active = false; };
  }, [auth, page, parentActor, project.id, revision]);
  const markDirty = useCallback((name: WorkspaceDirtySection, dirty: boolean) => dirtyChange(project.id, name, dirty), [dirtyChange, project.id]);
  useEffect(() => () => {
    for (const name of ["routing", "templates", "knowledge", "notes"] as const) markDirty(name, false);
  }, [markDirty]);
  async function refresh() { setRevision((value) => value + 1); departments.reload(); }
  if (error || departments.error) return <div className="admin-notice admin-notice--error" role="alert">
    {t("projects.loadError")} <button type="button" onClick={() => void refresh()}>{t("projectGate.retry")}</button>
  </div>;
  if (!result || result.auth !== auth || departments.loading) return <p role="status">{t("projects.loading")}</p>;
  if (!canShowPage(result.actor, department)) return <p className="empty-state">{t("departments.noModules")}</p>;
  return renderWorkspace({
    actor: result.actor, auth, inboxes: result.inboxes, channels: result.channels, activeInboxId, realtimeEvent, realtimeSyncRevision,
    selectInbox: setSelectedInboxId, refresh, department,
    navigate: (nextPage, projectId, segments) => onNavigate(nextPage, projectId ?? project.id, segments),
    changeDepartment: (id) => onDepartmentSelect(project.id, id),
    dirtyChange: markDirty,
  });
}
