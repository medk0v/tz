import { createContext } from "react";

export const ResourceVisibilityPermissionContext = createContext<boolean | null>(null);

export interface ResourceVisibility {
  project_ids: string[];
  department_ids: string[];
}

export const allWorkspaces: ResourceVisibility = { project_ids: [], department_ids: [] };

export function visibilityEqual(left?: ResourceVisibility, right?: ResourceVisibility): boolean {
  return JSON.stringify({ project_ids: [...(left?.project_ids ?? [])].sort(), department_ids: [...(left?.department_ids ?? [])].sort() })
    === JSON.stringify({ project_ids: [...(right?.project_ids ?? [])].sort(), department_ids: [...(right?.department_ids ?? [])].sort() });
}
