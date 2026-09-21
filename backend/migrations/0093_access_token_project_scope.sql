-- A role source distinguishes new role-managed project/all-project tokens from
-- legacy tokens, which retain their stored permission and Inbox restrictions.
ALTER TABLE api_keys
    ADD COLUMN role_project_id uuid,
    ADD CONSTRAINT api_keys_role_project_fk
        FOREIGN KEY (tenant_id, role_project_id) REFERENCES projects (tenant_id, id),
    ADD CONSTRAINT api_keys_role_project_scope_check CHECK (
        role_project_id IS NULL OR project_id IS NULL OR role_project_id = project_id
    ),
    ADD CONSTRAINT api_keys_all_projects_inbox_scope_check CHECK (
        role_project_id IS NULL OR project_id IS NOT NULL OR inbox_scope IS NULL
    );

CREATE INDEX api_keys_role_source_idx ON api_keys (tenant_id, role_project_id, role)
    WHERE role_project_id IS NOT NULL;
