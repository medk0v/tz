CREATE TABLE ai_tasks (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL REFERENCES tenants (id),
    project_id uuid NOT NULL,
    text text NOT NULL CHECK (length(trim(text)) BETWEEN 1 AND 50000),
    schedule jsonb NOT NULL CHECK (
        jsonb_typeof(schedule) = 'object'
        AND COALESCE(
            schedule->>'kind' IN ('once', 'daily', 'weekly', 'monthly', 'yearly'),
            false
        )
    ),
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, id),
    FOREIGN KEY (tenant_id, project_id) REFERENCES projects (tenant_id, id)
);

CREATE INDEX ai_tasks_project_created_idx
    ON ai_tasks (tenant_id, project_id, created_at DESC, id DESC);
