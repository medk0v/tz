import { useCallback, useEffect, useState } from "react";
import type { ActorContext, OperatorAuth } from "../api";
import { listDepartments, type DepartmentList } from "../department-api";

export function useDepartmentWorkspace(auth: OperatorAuth | null, actor: ActorContext | null) {
  const [revision, setRevision] = useState(0);
  const [result, setResult] = useState<{ key: string; data?: DepartmentList; error?: unknown } | null>(null);
  const projectId = actor?.project_id;
  const enabled = Boolean(auth && projectId && !actor?.is_demo
    && (auth.kind === "session" || actor?.department_id)
    && (actor?.department_id !== undefined || actor?.can_access_director !== undefined));
  const key = JSON.stringify([actor?.actor_id, projectId, actor?.department_id, actor?.is_director,
    actor?.auth_method, actor?.department_restricted, actor?.inbox_scope]);
  useEffect(() => {
    if (!enabled || !auth || !projectId) return;
    let active = true;
    void listDepartments(auth, projectId).then(
      (data) => { if (active) setResult({ key, data }); },
      (error: unknown) => { if (active) setResult((previous) => ({ ...(previous?.key === key ? previous : {}), key, error })); },
    );
    return () => { active = false; };
  }, [auth, enabled, key, projectId, revision]);
  const reload = useCallback(() => setRevision((current) => current + 1), []);
  const current = result?.key === key ? result : null;
  return {
    enabled,
    loading: enabled && current === null,
    data: enabled ? current?.data ?? null : null,
    error: enabled ? current?.error : undefined,
    reload,
  };
}
