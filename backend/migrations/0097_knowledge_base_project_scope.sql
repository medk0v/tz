-- Knowledge bases belong to one project, including records created when empty
-- visibility meant tenant-wide sharing. Keep department restrictions in that project.
UPDATE knowledge_bases AS base
SET visibility = jsonb_build_object(
    'project_ids', jsonb_build_array(base.project_id),
    'department_ids', COALESCE((
        SELECT jsonb_agg(department.id ORDER BY department.id)
        FROM departments AS department
        WHERE department.tenant_id = base.tenant_id
          AND department.project_id = base.project_id
          AND base.visibility->'department_ids' ? department.id::text
    ), '[]'::jsonb)
);
