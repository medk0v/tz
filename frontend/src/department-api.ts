import { managementApiRequest, type ActorContext, type OperatorAuth } from "./api";

export interface SidebarCategory {
  id: string;
  name: string;
  items: string[];
}

export interface DepartmentInput {
  name: string;
  icon?: string;
  sidebar_items?: string[];
  sidebar_categories?: SidebarCategory[];
  default_page?: string;
  show_default_channels?: boolean;
  inbox_ids?: string[];
}

export interface Department {
  id: string;
  name: string;
  icon: string;
  sidebar_items: string[];
  sidebar_categories?: SidebarCategory[];
  default_page: string;
  show_default_channels?: boolean;
  position: number;
  inbox_ids: string[];
  member_count: number;
}

export interface DirectorMenu {
  sidebar_items: string[];
  sidebar_categories?: SidebarCategory[];
  default_page: string;
  show_default_channels: boolean;
}

export interface DepartmentList {
  director_menu?: DirectorMenu;
  items: Department[];
  default_department_id: string | null;
  director_enabled: boolean;
  /** Always returned by this API version: members who never chose a workspace open the control center. */
  default_director_workspace?: boolean;
  can_access_director: boolean;
  inbox_options?: Array<{ id: string; name: string }>;
}

export interface DepartmentOverviewCounts {
  open_conversations: number;
  conversation_count: number;
  member_count: number;
  inbox_count: number;
}

export interface DirectorOverviewData {
  project_id: string;
  departments: Array<DepartmentOverviewCounts & { id: string; name: string; icon: string }>;
  totals: DepartmentOverviewCounts;
}

const projectPath = (projectId: string) => `/api/v1/projects/${encodeURIComponent(projectId)}`;

export function listDepartments(auth: OperatorAuth, projectId: string, signal?: AbortSignal): Promise<DepartmentList> {
  return managementApiRequest(auth, `${projectPath(projectId)}/departments`, { signal });
}

export function createDepartment(auth: OperatorAuth, projectId: string, input: DepartmentInput): Promise<Department> {
  return managementApiRequest(auth, `${projectPath(projectId)}/departments`, {
    method: "POST", body: JSON.stringify(input),
  });
}

export function updateDepartment(auth: OperatorAuth, projectId: string, departmentId: string, input: Partial<DepartmentInput> & { position?: number }): Promise<Department> {
  return managementApiRequest(auth, `${projectPath(projectId)}/departments/${encodeURIComponent(departmentId)}`, {
    method: "PATCH", body: JSON.stringify(input),
  });
}

export function getDirectorOverview(auth: OperatorAuth, projectId: string): Promise<DirectorOverviewData> {
  return managementApiRequest(auth, `${projectPath(projectId)}/overview`);
}

export function deleteDepartment(auth: OperatorAuth, projectId: string, departmentId: string): Promise<DepartmentList> {
  return managementApiRequest(auth, `${projectPath(projectId)}/departments/${encodeURIComponent(departmentId)}`, { method: "DELETE" });
}

export function selectDepartment(auth: OperatorAuth, departmentId: string | null): Promise<ActorContext> {
  return managementApiRequest(auth, "/api/v1/auth/department", {
    method: "POST", body: JSON.stringify({ department_id: departmentId }),
  });
}
