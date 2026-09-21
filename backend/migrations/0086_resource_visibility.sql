-- Resource ownership remains stable; visibility controls where it is listed.
ALTER TABLE knowledge_bases ADD COLUMN visibility jsonb NOT NULL DEFAULT '{"project_ids":[],"department_ids":[]}';
ALTER TABLE ai_tasks ADD COLUMN visibility jsonb NOT NULL DEFAULT '{"project_ids":[],"department_ids":[]}';
ALTER TABLE ai_profiles ADD COLUMN visibility jsonb NOT NULL DEFAULT '{"project_ids":[],"department_ids":[]}';
ALTER TABLE reply_templates ADD COLUMN visibility jsonb NOT NULL DEFAULT '{"project_ids":[],"department_ids":[]}';
ALTER TABLE channel_connections ADD COLUMN visibility jsonb NOT NULL DEFAULT '{"project_ids":[],"department_ids":[]}';

CREATE FUNCTION resource_visible(scope jsonb, selected_project uuid, selected_department uuid)
RETURNS boolean LANGUAGE sql STABLE AS $$
    SELECT selected_project IS NOT NULL
      AND (scope->'project_ids' = '[]'::jsonb OR scope->'project_ids' ? selected_project::text)
      AND (scope->'department_ids' = '[]'::jsonb OR EXISTS (
          SELECT 1 FROM departments
          WHERE project_id = selected_project
            AND scope->'department_ids' ? id::text
            AND (selected_department IS NULL OR id = selected_department)
      ));
$$;

-- A shared agent may configure a channel owned by another project in the tenant.
ALTER TABLE custom_ai_channel_configs DROP CONSTRAINT custom_ai_channel_configs_tenant_id_project_id_ai_profile__fkey;
ALTER TABLE custom_ai_channel_configs ADD CONSTRAINT custom_ai_channel_profile_fk
    FOREIGN KEY (tenant_id, ai_profile_id) REFERENCES ai_profiles (tenant_id, id)
    ON DELETE SET NULL (ai_profile_id);
