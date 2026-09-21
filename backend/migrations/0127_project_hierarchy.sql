ALTER TABLE projects
    ADD COLUMN parent_project_id uuid,
    ADD CONSTRAINT projects_parent_project_fk
        FOREIGN KEY (tenant_id, parent_project_id) REFERENCES projects (tenant_id, id),
    ADD CONSTRAINT projects_parent_not_self_check
        CHECK (parent_project_id IS NULL OR parent_project_id <> id);

CREATE INDEX projects_parent_project_idx
    ON projects (tenant_id, parent_project_id)
    WHERE parent_project_id IS NOT NULL;
