import type { Project } from "../api";

export interface ProjectGroup {
  project: Project;
  children: Project[];
}

/** Keep accessible subprojects visible even when their parent is not in this list. */
export function groupProjects(projects: readonly Project[]): ProjectGroup[] {
  const visibleRoots = new Set(projects.filter((project) => !project.parent_project_id).map((project) => project.id));
  const children = new Map<string, Project[]>();
  for (const project of projects) {
    if (project.parent_project_id && visibleRoots.has(project.parent_project_id)) {
      const siblings = children.get(project.parent_project_id) ?? [];
      siblings.push(project);
      children.set(project.parent_project_id, siblings);
    }
  }
  return projects
    .filter((project) => !project.parent_project_id || !visibleRoots.has(project.parent_project_id))
    .map((project) => ({ project, children: children.get(project.id) ?? [] }));
}

export function projectHierarchyRows(projects: readonly Project[]) {
  return groupProjects(projects).flatMap(({ project, children }) => [
    { project, depth: 0 as const },
    ...children.map((child) => ({ project: child, depth: 1 as const })),
  ]);
}

export function parentProjectOptions(projects: readonly Project[], projectId?: string): Project[] {
  return projects.filter((project) => project.id !== projectId && !project.parent_project_id
    && project.status === "active" && project.role === "admin");
}
