ALTER TABLE projects
    ADD COLUMN default_director_workspace boolean NOT NULL DEFAULT false,
    ADD CONSTRAINT projects_default_workspace_check CHECK (
        NOT default_director_workspace OR director_enabled
    );

-- The workspace a member last opened in a project. A new session lands there instead of
-- the project-wide default; the project default only covers members who never chose one.
CREATE TABLE user_project_workspaces (
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    user_id uuid NOT NULL REFERENCES users (id),
    project_id uuid NOT NULL,
    department_id uuid,
    director_mode boolean NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, user_id, project_id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id, department_id)
        REFERENCES departments (tenant_id, project_id, id),
    CHECK (director_mode <> (department_id IS NOT NULL))
);
